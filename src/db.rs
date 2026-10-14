use std::path::Path;

use anyhow::{Result, bail};
use chrono::Local;
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::models::{Link, LinkKind, NewTodo, Priority, Status, Todo};

const MIGRATIONS: &[&str] = &[r#"
    CREATE TABLE todos (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        title TEXT NOT NULL,
        description TEXT NOT NULL DEFAULT '',
        priority INTEGER NOT NULL DEFAULT 3,
        status TEXT NOT NULL DEFAULT 'open',
        position INTEGER NOT NULL DEFAULT 0,
        raw_input TEXT NOT NULL DEFAULT '',
        created_at TEXT NOT NULL,
        done_at TEXT
    );
    CREATE TABLE links (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        todo_id INTEGER NOT NULL REFERENCES todos(id) ON DELETE CASCADE,
        url TEXT NOT NULL,
        kind TEXT NOT NULL,
        label TEXT NOT NULL DEFAULT ''
    );
    CREATE INDEX links_todo_id ON links(todo_id);
"#];

const SELECT_TODO: &str = "SELECT id, title, description, priority, status, position, raw_input, created_at, done_at FROM todos";

const ORDER_BY: &str = "ORDER BY CASE status WHEN 'open' THEN 0 ELSE 1 END, \
     CASE status WHEN 'done' THEN done_at END DESC, priority, position, created_at DESC, id DESC";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}

pub struct Database {
    connection: Connection,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        Self::setup(Connection::open(path)?)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        Self::setup(Connection::open_in_memory()?)
    }

    fn setup(connection: Connection) -> Result<Self> {
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let database = Database { connection };
        database.migrate()?;
        Ok(database)
    }

    fn migrate(&self) -> Result<()> {
        let version: usize = self
            .connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))?
            as usize;
        for (index, migration) in MIGRATIONS.iter().enumerate().skip(version) {
            self.connection.execute_batch(migration)?;
            self.connection
                .pragma_update(None, "user_version", (index + 1) as i64)?;
        }
        Ok(())
    }

    pub fn insert(&mut self, todo: &NewTodo) -> Result<i64> {
        let transaction = self.connection.transaction()?;
        let top_position: i64 = transaction.query_row(
            "SELECT COALESCE(MIN(position), 1) - 1 FROM todos WHERE status = 'open'",
            [],
            |row| row.get(0),
        )?;
        transaction.execute(
            "INSERT INTO todos (title, description, priority, status, position, raw_input, created_at) \
             VALUES (?1, ?2, ?3, 'open', ?4, ?5, ?6)",
            params![
                todo.title,
                todo.description,
                todo.priority.rank(),
                top_position,
                todo.raw_input,
                now()
            ],
        )?;
        let id = transaction.last_insert_rowid();
        for link in &todo.links {
            transaction.execute(
                "INSERT INTO links (todo_id, url, kind, label) VALUES (?1, ?2, ?3, ?4)",
                params![id, link.url, link.kind.as_str(), link.label],
            )?;
        }
        transaction.commit()?;
        Ok(id)
    }

    pub fn list(&self, include_done: bool) -> Result<Vec<Todo>> {
        let filter = if include_done { "" } else { "WHERE status = 'open'" };
        let sql = format!("{SELECT_TODO} {filter} {ORDER_BY}");
        let mut statement = self.connection.prepare(&sql)?;
        let mut todos = statement
            .query_map([], todo_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for todo in &mut todos {
            todo.links = self.links_for(todo.id)?;
        }
        Ok(todos)
    }

    pub fn get(&self, id: i64) -> Result<Todo> {
        let sql = format!("{SELECT_TODO} WHERE id = ?1");
        let todo = self
            .connection
            .query_row(&sql, [id], todo_from_row)
            .optional()?;
        match todo {
            Some(mut todo) => {
                todo.links = self.links_for(id)?;
                Ok(todo)
            }
            None => bail!("todo #{id} não existe"),
        }
    }

    fn links_for(&self, todo_id: i64) -> Result<Vec<Link>> {
        let mut statement = self
            .connection
            .prepare("SELECT url, kind, label FROM links WHERE todo_id = ?1 ORDER BY id")?;
        let links = statement
            .query_map([todo_id], |row| {
                Ok(Link {
                    url: row.get(0)?,
                    kind: LinkKind::parse(&row.get::<_, String>(1)?),
                    label: row.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(links)
    }

    pub fn set_status(&self, id: i64, status: Status) -> Result<()> {
        let done_at = (status == Status::Done).then(now);
        let changed = self.connection.execute(
            "UPDATE todos SET status = ?1, done_at = ?2 WHERE id = ?3",
            params![status.as_str(), done_at, id],
        )?;
        ensure_changed(changed, id)
    }

    pub fn set_priority(&self, id: i64, priority: Priority) -> Result<()> {
        let changed = self.connection.execute(
            "UPDATE todos SET priority = ?1 WHERE id = ?2",
            params![priority.rank(), id],
        )?;
        ensure_changed(changed, id)
    }

    pub fn update_text(&self, id: i64, title: &str, description: &str) -> Result<()> {
        let changed = self.connection.execute(
            "UPDATE todos SET title = ?1, description = ?2 WHERE id = ?3",
            params![title, description, id],
        )?;
        ensure_changed(changed, id)
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        let changed = self
            .connection
            .execute("DELETE FROM todos WHERE id = ?1", [id])?;
        ensure_changed(changed, id)
    }

    pub fn move_todo(&mut self, id: i64, direction: Direction) -> Result<()> {
        let mut open = self.list(false)?;
        let Some(index) = open.iter().position(|todo| todo.id == id) else {
            bail!("todo #{id} não está aberto");
        };
        let neighbor_index = match direction {
            Direction::Up if index > 0 => index - 1,
            Direction::Down if index + 1 < open.len() => index + 1,
            _ => return Ok(()),
        };

        let neighbor_priority = open[neighbor_index].priority;
        if open[index].priority != neighbor_priority {
            open[index].priority = neighbor_priority;
        } else {
            open.swap(index, neighbor_index);
        }

        let transaction = self.connection.transaction()?;
        for (position, todo) in open.iter().enumerate() {
            transaction.execute(
                "UPDATE todos SET position = ?1, priority = ?2 WHERE id = ?3",
                params![position as i64, todo.priority.rank(), todo.id],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }
}

fn todo_from_row(row: &Row) -> rusqlite::Result<Todo> {
    Ok(Todo {
        id: row.get(0)?,
        title: row.get(1)?,
        description: row.get(2)?,
        priority: Priority::from_rank(row.get(3)?),
        status: Status::parse(&row.get::<_, String>(4)?),
        position: row.get(5)?,
        raw_input: row.get(6)?,
        created_at: row.get(7)?,
        done_at: row.get(8)?,
        links: Vec::new(),
    })
}

fn ensure_changed(changed: usize, id: i64) -> Result<()> {
    if changed == 0 {
        bail!("todo #{id} não existe");
    }
    Ok(())
}

fn now() -> String {
    Local::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_todo(title: &str, priority: Priority) -> NewTodo {
        NewTodo {
            title: title.into(),
            description: String::new(),
            priority,
            raw_input: title.into(),
            links: Vec::new(),
        }
    }

    fn titles(database: &Database) -> Vec<String> {
        database
            .list(false)
            .unwrap()
            .into_iter()
            .map(|todo| todo.title)
            .collect()
    }

    #[test]
    fn inserts_with_links_and_reads_back() {
        let mut database = Database::open_in_memory().unwrap();
        let mut todo = new_todo("revisar PR", Priority::High);
        todo.links.push(Link {
            url: "https://github.com/org/repo/pull/1".into(),
            kind: LinkKind::GithubPr,
            label: "PR #1".into(),
        });
        let id = database.insert(&todo).unwrap();
        let stored = database.get(id).unwrap();
        assert_eq!(stored.title, "revisar PR");
        assert_eq!(stored.priority, Priority::High);
        assert_eq!(stored.links.len(), 1);
        assert_eq!(stored.links[0].kind, LinkKind::GithubPr);
    }

    #[test]
    fn orders_by_priority_then_newest_first() {
        let mut database = Database::open_in_memory().unwrap();
        database.insert(&new_todo("a", Priority::Normal)).unwrap();
        database.insert(&new_todo("b", Priority::Urgent)).unwrap();
        database.insert(&new_todo("c", Priority::Normal)).unwrap();
        database.insert(&new_todo("d", Priority::Low)).unwrap();
        assert_eq!(titles(&database), vec!["b", "c", "a", "d"]);
    }

    #[test]
    fn done_todos_are_hidden_unless_requested() {
        let mut database = Database::open_in_memory().unwrap();
        let id = database.insert(&new_todo("a", Priority::Normal)).unwrap();
        database.insert(&new_todo("b", Priority::Normal)).unwrap();
        database.set_status(id, Status::Done).unwrap();
        assert_eq!(titles(&database), vec!["b"]);
        let all = database.list(true).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].status, Status::Done);
        assert!(all[1].done_at.is_some());
    }

    #[test]
    fn moves_within_priority_and_across_groups() {
        let mut database = Database::open_in_memory().unwrap();
        database.insert(&new_todo("a", Priority::High)).unwrap();
        let b = database.insert(&new_todo("b", Priority::Normal)).unwrap();
        database.insert(&new_todo("c", Priority::Normal)).unwrap();
        assert_eq!(titles(&database), vec!["a", "c", "b"]);

        database.move_todo(b, Direction::Up).unwrap();
        assert_eq!(titles(&database), vec!["a", "b", "c"]);

        database.move_todo(b, Direction::Up).unwrap();
        assert_eq!(database.get(b).unwrap().priority, Priority::High);
        assert_eq!(titles(&database), vec!["a", "b", "c"]);

        database.move_todo(b, Direction::Up).unwrap();
        assert_eq!(titles(&database), vec!["b", "a", "c"]);
    }

    #[test]
    fn delete_removes_links_and_errors_on_missing() {
        let mut database = Database::open_in_memory().unwrap();
        let mut todo = new_todo("a", Priority::Normal);
        todo.links.push(Link {
            url: "https://x.com".into(),
            kind: LinkKind::Other,
            label: String::new(),
        });
        let id = database.insert(&todo).unwrap();
        database.delete(id).unwrap();
        assert!(database.get(id).is_err());
        assert!(database.delete(id).is_err());
    }
}
