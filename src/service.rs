use anyhow::Result;

use crate::ai;
use crate::config::Config;
use crate::db::Database;
use crate::enrich;
use crate::links;
use crate::models::{NewTodo, Priority};

pub struct AddOutcome {
    pub id: i64,
    pub warning: Option<String>,
}

pub fn build_todo(config: &Config, text: &str, use_ai: bool) -> (NewTodo, Option<String>) {
    let enriched: Vec<_> = links::extract_links(text)
        .iter()
        .map(|parsed| enrich::enrich(parsed, config.clickup_token.as_deref()))
        .collect();
    let contexts: Vec<String> = enriched.iter().map(|item| item.context.clone()).collect();
    let todo_links = enriched.into_iter().map(|item| item.link).collect();

    let suggestion = if !use_ai {
        Err(None)
    } else {
        match config.deepseek_api_key.as_deref().filter(|key| !key.is_empty()) {
            None => Err(Some("sem chave do DeepSeek (rode `t config`); salvei o texto como está".to_string())),
            Some(key) => ai::suggest(key, config.model(), text, &contexts)
                .map_err(|error| Some(format!("IA falhou ({error:#}); salvei o texto como está"))),
        }
    };

    match suggestion {
        Ok(suggestion) => (
            NewTodo {
                title: suggestion.title,
                description: suggestion.description,
                priority: suggestion.priority,
                raw_input: text.to_string(),
                links: todo_links,
            },
            None,
        ),
        Err(warning) => (
            NewTodo {
                title: fallback_title(text),
                description: String::new(),
                priority: guess_priority(text),
                raw_input: text.to_string(),
                links: todo_links,
            },
            warning,
        ),
    }
}

pub fn add(database: &mut Database, config: &Config, text: &str, use_ai: bool) -> Result<AddOutcome> {
    let (todo, warning) = build_todo(config, text, use_ai);
    let id = database.insert(&todo)?;
    Ok(AddOutcome { id, warning })
}

fn fallback_title(text: &str) -> String {
    let without_links = links::strip_links(text);
    let title = without_links.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() { text.trim().to_string() } else { title }
}

pub fn guess_priority(text: &str) -> Priority {
    let lowered = text.to_lowercase();
    let has_any = |words: &[&str]| words.iter().any(|word| lowered.contains(word));
    if has_any(&["urgente", "urgent", "asap", "agora", "bloqueando", "bloqueado"]) {
        Priority::Urgent
    } else if has_any(&["prioridade baixa", "baixa prioridade", "quando der", "sem pressa", "algum dia"]) {
        Priority::Low
    } else if has_any(&["importante", "hoje", "prioridade alta", "alta prioridade"]) {
        Priority::High
    } else {
        Priority::Normal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_title_removes_links() {
        assert_eq!(
            fallback_title("revisar https://github.com/o/r/pull/1  urgente"),
            "revisar urgente"
        );
        assert_eq!(fallback_title("https://example.com"), "https://example.com");
    }

    #[test]
    fn guesses_priority_from_keywords() {
        assert_eq!(guess_priority("corrigir login URGENTE"), Priority::Urgent);
        assert_eq!(guess_priority("fazer hoje"), Priority::High);
        assert_eq!(guess_priority("ler artigo quando der"), Priority::Low);
        assert_eq!(guess_priority("revisar PR"), Priority::Normal);
    }
}
