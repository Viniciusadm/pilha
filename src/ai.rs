use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::json;

use crate::models::Priority;

const ENDPOINT: &str = "https://api.deepseek.com/chat/completions";

const SYSTEM_PROMPT: &str = r#"Você organiza a lista de tarefas pessoal de um desenvolvedor.
Recebe o texto livre que ele digitou e, às vezes, detalhes dos links citados (PRs do GitHub, tasks do ClickUp).
Responda APENAS com um objeto JSON no formato:
{"title": string, "description": string, "priority": "urgent" | "high" | "normal" | "low"}

Regras:
- title: curto (até 70 caracteres), em português, começando com verbo no imperativo ("Revisar", "Corrigir", "Implementar"...). Cite o nome do repo/PR ou da task quando ajudar a reconhecer.
- description: 1 a 3 frases com o que precisa ser feito e o contexto útil; não repita as URLs.
- priority: extraia do texto do usuário. "urgente", "agora", "bloqueando", "asap" => urgent; "importante", "hoje", "alta" => high; "quando der", "sem pressa", "baixa", "algum dia" => low; sem indicação => normal. A prioridade escrita pelo usuário vence a prioridade do ClickUp.
- Nunca invente informações que não estão no texto nem nos detalhes."#;

#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub title: String,
    pub description: String,
    pub priority: Priority,
}

#[derive(Deserialize)]
struct RawSuggestion {
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    priority: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Deserialize)]
struct ChatMessage {
    content: String,
}

pub fn suggest(api_key: &str, model: &str, user_text: &str, link_context: &[String]) -> Result<Suggestion> {
    let mut user_message = format!(
        "Data de hoje: {}\n\nTexto do usuário:\n{user_text}",
        chrono::Local::now().format("%Y-%m-%d (%A)")
    );
    if !link_context.is_empty() {
        user_message.push_str("\n\nDetalhes dos links:\n");
        user_message.push_str(&link_context.join("\n---\n"));
    }

    let response = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?
        .post(ENDPOINT)
        .bearer_auth(api_key)
        .json(&json!({
            "model": model,
            "messages": [
                {"role": "system", "content": SYSTEM_PROMPT},
                {"role": "user", "content": user_message},
            ],
            "response_format": {"type": "json_object"},
            "temperature": 0.2,
        }))
        .send()
        .context("falha ao chamar o DeepSeek")?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        bail!("DeepSeek respondeu {status}: {}", body.chars().take(300).collect::<String>());
    }

    let chat: ChatResponse = response.json().context("resposta do DeepSeek em formato inesperado")?;
    let content = chat
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content)
        .context("DeepSeek não retornou nenhuma resposta")?;
    parse_suggestion(&content)
}

pub fn parse_suggestion(content: &str) -> Result<Suggestion> {
    let trimmed = content
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let raw: RawSuggestion = serde_json::from_str(trimmed).context("JSON inválido vindo da IA")?;
    let title = raw.title.trim().to_string();
    if title.is_empty() {
        bail!("IA devolveu título vazio");
    }
    Ok(Suggestion {
        title,
        description: raw.description.trim().to_string(),
        priority: Priority::parse(&raw.priority),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_json() {
        let suggestion = parse_suggestion(
            r#"{"title": "Revisar PR #12 do api", "description": "Checar testes.", "priority": "urgent"}"#,
        )
        .unwrap();
        assert_eq!(suggestion.title, "Revisar PR #12 do api");
        assert_eq!(suggestion.description, "Checar testes.");
        assert_eq!(suggestion.priority, Priority::Urgent);
    }

    #[test]
    fn unknown_or_missing_priority_becomes_normal() {
        let suggestion = parse_suggestion(r#"{"title": "X", "priority": "mega"}"#).unwrap();
        assert_eq!(suggestion.priority, Priority::Normal);
        let suggestion = parse_suggestion(r#"{"title": "X"}"#).unwrap();
        assert_eq!(suggestion.priority, Priority::Normal);
    }

    #[test]
    fn accepts_markdown_fenced_json() {
        let suggestion = parse_suggestion("```json\n{\"title\": \"X\", \"priority\": \"low\"}\n```").unwrap();
        assert_eq!(suggestion.priority, Priority::Low);
    }

    #[test]
    fn rejects_broken_json_and_empty_title() {
        assert!(parse_suggestion("not json").is_err());
        assert!(parse_suggestion(r#"{"title": "  "}"#).is_err());
    }
}
