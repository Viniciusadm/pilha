use std::time::Duration;

use anyhow::Result;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

use crate::config::Config;
use crate::db::{Database, Direction};
use crate::models::{LinkKind, Priority, Status, Todo};
use crate::{open_url, service};

const REFRESH_INTERVAL: Duration = Duration::from_secs(2);

enum Mode {
    Normal,
    Adding(String),
    Thinking(String),
    ConfirmDelete(i64),
}

struct App<'a> {
    database: &'a mut Database,
    config: &'a Config,
    todos: Vec<Todo>,
    list_state: ListState,
    show_done: bool,
    mode: Mode,
    message: Option<String>,
    should_quit: bool,
}

pub fn run(database: &mut Database, config: &Config) -> Result<()> {
    let mut terminal = ratatui::init();
    execute!(std::io::stdout(), EnableBracketedPaste)?;
    let result = App::new(database, config).and_then(|mut app| app.run(&mut terminal));
    let _ = execute!(std::io::stdout(), DisableBracketedPaste);
    ratatui::restore();
    result
}

impl<'a> App<'a> {
    fn new(database: &'a mut Database, config: &'a Config) -> Result<Self> {
        let mut app = App {
            database,
            config,
            todos: Vec::new(),
            list_state: ListState::default(),
            show_done: false,
            mode: Mode::Normal,
            message: None,
            should_quit: false,
        };
        app.reload()?;
        Ok(app)
    }

    fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.should_quit {
            terminal.draw(|frame| self.draw(frame))?;

            if let Mode::Thinking(text) = &self.mode {
                let text = text.clone();
                self.submit(&text)?;
                continue;
            }

            if !event::poll(REFRESH_INTERVAL)? {
                if matches!(self.mode, Mode::Normal) {
                    self.reload()?;
                }
                continue;
            }
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => self.handle_key(key)?,
                Event::Paste(pasted) => {
                    if let Mode::Adding(input) = &mut self.mode {
                        input.push_str(&pasted.replace(['\n', '\r'], " "));
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn selected(&self) -> Option<&Todo> {
        self.list_state.selected().and_then(|index| self.todos.get(index))
    }

    fn selected_id(&self) -> Option<i64> {
        self.selected().map(|todo| todo.id)
    }

    fn reload(&mut self) -> Result<()> {
        let keep = self.selected_id();
        self.todos = self.database.list(self.show_done)?;
        self.select_id(keep);
        Ok(())
    }

    fn select_id(&mut self, id: Option<i64>) {
        let index = id
            .and_then(|id| self.todos.iter().position(|todo| todo.id == id))
            .or_else(|| self.list_state.selected().map(|index| index.min(self.todos.len().saturating_sub(1))))
            .or(Some(0));
        self.list_state
            .select(if self.todos.is_empty() { None } else { index });
    }

    fn submit(&mut self, text: &str) -> Result<()> {
        let outcome = service::add(self.database, self.config, text, true)?;
        self.message = outcome.warning.or_else(|| Some("adicionado".into()));
        self.mode = Mode::Normal;
        self.reload()?;
        self.select_id(Some(outcome.id));
        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return Ok(());
        }
        match &mut self.mode {
            Mode::Adding(input) => {
                match key.code {
                    KeyCode::Esc => self.mode = Mode::Normal,
                    KeyCode::Enter => {
                        let text = input.trim().to_string();
                        self.mode = if text.is_empty() { Mode::Normal } else { Mode::Thinking(text) };
                    }
                    KeyCode::Backspace => {
                        input.pop();
                    }
                    KeyCode::Char(character) => input.push(character),
                    _ => {}
                }
                return Ok(());
            }
            Mode::ConfirmDelete(id) => {
                let id = *id;
                self.mode = Mode::Normal;
                if matches!(key.code, KeyCode::Char('y') | KeyCode::Char('s')) {
                    self.database.delete(id)?;
                    self.message = Some(format!("#{id} removido"));
                    self.reload()?;
                }
                return Ok(());
            }
            Mode::Thinking(_) => return Ok(()),
            Mode::Normal => {}
        }

        self.message = None;
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Char('j') | KeyCode::Down => self.list_state.select_next(),
            KeyCode::Char('k') | KeyCode::Up => self.list_state.select_previous(),
            KeyCode::Char('g') | KeyCode::Home => self.list_state.select_first(),
            KeyCode::Char('G') | KeyCode::End => self.list_state.select_last(),
            KeyCode::Char('a') | KeyCode::Char('i') => self.mode = Mode::Adding(String::new()),
            KeyCode::Char('h') => {
                self.show_done = !self.show_done;
                self.reload()?;
            }
            KeyCode::Char('r') => self.reload()?,
            _ => self.handle_selected_key(key.code)?,
        }
        if let Some(index) = self.list_state.selected()
            && index >= self.todos.len()
        {
            self.list_state.select(self.todos.len().checked_sub(1));
        }
        Ok(())
    }

    fn handle_selected_key(&mut self, code: KeyCode) -> Result<()> {
        let Some(todo) = self.selected().cloned() else {
            return Ok(());
        };
        match code {
            KeyCode::Char('x') | KeyCode::Char(' ') | KeyCode::Enter => {
                let status = if todo.status == Status::Done { Status::Open } else { Status::Done };
                self.database.set_status(todo.id, status)?;
                self.message = Some(match status {
                    Status::Done => format!("#{} concluído (u desfaz)", todo.id),
                    Status::Open => format!("#{} reaberto", todo.id),
                });
                self.reload()?;
            }
            KeyCode::Char('u') => {
                self.database.set_status(todo.id, Status::Open)?;
                self.reload()?;
            }
            KeyCode::Char('p') => {
                self.database.set_priority(todo.id, todo.priority.next())?;
                self.reload()?;
                self.select_id(Some(todo.id));
            }
            KeyCode::Char('K') => {
                self.database.move_todo(todo.id, Direction::Up)?;
                self.reload()?;
                self.select_id(Some(todo.id));
            }
            KeyCode::Char('J') => {
                self.database.move_todo(todo.id, Direction::Down)?;
                self.reload()?;
                self.select_id(Some(todo.id));
            }
            KeyCode::Char('o') => {
                if todo.links.is_empty() {
                    self.message = Some("sem links".into());
                }
                for link in &todo.links {
                    open_url(&link.url)?;
                }
            }
            KeyCode::Char('d') | KeyCode::Delete => self.mode = Mode::ConfirmDelete(todo.id),
            _ => {}
        }
        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame) {
        let [list_area, detail_area, footer_area] = Layout::vertical([
            Constraint::Min(5),
            Constraint::Length(7),
            Constraint::Length(3),
        ])
        .areas(frame.area());

        self.draw_list(frame, list_area);
        self.draw_detail(frame, detail_area);
        self.draw_footer(frame, footer_area);
    }

    fn draw_list(&mut self, frame: &mut Frame, area: Rect) {
        let open_count = self.todos.iter().filter(|todo| todo.status == Status::Open).count();
        let title = format!(" Pilha · {open_count} aberto(s){} ", if self.show_done { " · mostrando concluídos" } else { "" });
        let items: Vec<ListItem> = self.todos.iter().map(todo_item).collect();
        let list = if items.is_empty() {
            List::new(vec![ListItem::new(Line::from("  Nada na pilha. Aperte `a` para adicionar.").dim())])
        } else {
            List::new(items)
        };
        let list = list
            .block(Block::bordered().border_type(BorderType::Rounded).title(title.bold()))
            .highlight_style(Style::new().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
            .highlight_symbol("▶ ");
        frame.render_stateful_widget(list, area, &mut self.list_state);
    }

    fn draw_detail(&self, frame: &mut Frame, area: Rect) {
        let block = Block::bordered().border_type(BorderType::Rounded).title(" Detalhes ");
        let Some(todo) = self.selected() else {
            frame.render_widget(block, area);
            return;
        };
        let mut lines = Vec::new();
        if !todo.description.is_empty() {
            lines.push(Line::from(todo.description.clone()));
        }
        for link in &todo.links {
            lines.push(Line::from(vec![
                Span::styled(format!("{} ", link.label), link_style(link.kind)),
                Span::raw(link.url.clone()).dim(),
            ]));
        }
        if lines.is_empty() {
            lines.push(Line::from(todo.raw_input.clone()).dim());
        }
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }).block(block), area);
    }

    fn draw_footer(&self, frame: &mut Frame, area: Rect) {
        let block = Block::bordered().border_type(BorderType::Rounded);
        let paragraph = match &self.mode {
            Mode::Adding(input) => {
                frame.set_cursor_position((area.x + 3 + input.chars().count() as u16, area.y + 1));
                Paragraph::new(Line::from(vec![Span::raw("> ").bold(), Span::raw(input.clone())]))
                    .block(block.title(" Novo todo (Enter salva, Esc cancela) ").yellow())
            }
            Mode::Thinking(_) => Paragraph::new(Line::from("pensando… (IA organizando o todo)").yellow()).block(block),
            Mode::ConfirmDelete(id) => {
                Paragraph::new(Line::from(format!("Remover #{id}? (y/n)")).red().bold()).block(block)
            }
            Mode::Normal => {
                let text = self.message.clone().unwrap_or_else(|| {
                    "a novo · x concluir · u desfazer · p prioridade · J/K mover · o abrir links · d remover · h concluídos · q sair".into()
                });
                Paragraph::new(Line::from(text).dim()).block(block)
            }
        };
        frame.render_widget(paragraph, area);
    }
}

fn priority_style(priority: Priority) -> Style {
    match priority {
        Priority::Urgent => Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
        Priority::High => Style::new().fg(Color::Yellow),
        Priority::Normal => Style::new().fg(Color::Cyan),
        Priority::Low => Style::new().fg(Color::DarkGray),
    }
}

fn link_style(kind: LinkKind) -> Style {
    match kind {
        LinkKind::GithubPr => Style::new().fg(Color::Magenta),
        LinkKind::ClickupTask => Style::new().fg(Color::LightBlue),
        LinkKind::Other => Style::new().fg(Color::Gray),
    }
}

fn todo_item(todo: &Todo) -> ListItem<'static> {
    let done = todo.status == Status::Done;
    let mut spans = vec![
        Span::styled(format!("{:<8}", todo.priority.label()), priority_style(todo.priority)),
        Span::raw(if done { "✔ " } else { "" }),
    ];
    let title = Span::raw(todo.title.clone());
    spans.push(if done { title.crossed_out().dim() } else { title });
    for link in &todo.links {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(format!("[{}]", link.label), link_style(link.kind)));
    }
    ListItem::new(Line::from(spans))
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::KeyEventState;

    use super::*;
    use crate::models::{Link, NewTodo};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn screen(app: &mut App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(110, 20)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn renders_todos_and_handles_keys() {
        let mut database = Database::open_in_memory().unwrap();
        database
            .insert(&NewTodo {
                title: "Revisar PR de login".into(),
                description: "Checar os testes".into(),
                priority: Priority::Urgent,
                raw_input: String::new(),
                links: vec![Link {
                    url: "https://github.com/o/api/pull/7".into(),
                    kind: LinkKind::GithubPr,
                    label: "PR #7 api".into(),
                }],
            })
            .unwrap();
        let config = Config::default();
        let mut app = App::new(&mut database, &config).unwrap();

        let output = screen(&mut app);
        assert!(output.contains("URGENTE"));
        assert!(output.contains("Revisar PR de login"));
        assert!(output.contains("[PR #7 api]"));
        assert!(output.contains("Checar os testes"));

        app.handle_key(key(KeyCode::Char('p'))).unwrap();
        assert_eq!(app.todos[0].priority, Priority::High);

        app.handle_key(key(KeyCode::Char('x'))).unwrap();
        assert!(app.todos.is_empty());
        assert!(screen(&mut app).contains("Nada na pilha"));

        app.handle_key(key(KeyCode::Char('h'))).unwrap();
        assert_eq!(app.todos[0].status, Status::Done);

        app.handle_key(key(KeyCode::Char('a'))).unwrap();
        for character in "oi".chars() {
            app.handle_key(key(KeyCode::Char(character))).unwrap();
        }
        assert!(screen(&mut app).contains("> oi"));
        app.handle_key(key(KeyCode::Esc)).unwrap();
        assert!(matches!(app.mode, Mode::Normal));
    }
}
