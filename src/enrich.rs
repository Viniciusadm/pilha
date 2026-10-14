use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::links::ParsedLink;
use crate::models::Link;

const CLICKUP_DESCRIPTION_LIMIT: usize = 1500;

pub struct EnrichedLink {
    pub link: Link,
    pub context: String,
}

pub fn enrich(parsed: &ParsedLink, clickup_token: Option<&str>) -> EnrichedLink {
    let result = match parsed {
        ParsedLink::GithubPr { url, repo, number, .. } => github_pr(url, repo, *number),
        ParsedLink::ClickupTask { url, team_id, task_id } => match clickup_token {
            Some(token) => clickup_task(url, team_id.as_deref(), task_id, token),
            None => Err(anyhow::anyhow!("sem clickup_token configurado")),
        },
        ParsedLink::Other { .. } => Err(anyhow::anyhow!("link genérico")),
    };

    result.unwrap_or_else(|error| {
        if !matches!(parsed, ParsedLink::Other { .. }) {
            eprintln!("aviso: não consegui detalhar {}: {error:#}", parsed.url());
        }
        EnrichedLink {
            link: Link {
                url: parsed.url().to_string(),
                kind: parsed.kind(),
                label: fallback_label(parsed),
            },
            context: format!("Link: {}", parsed.url()),
        }
    })
}

fn fallback_label(parsed: &ParsedLink) -> String {
    match parsed {
        ParsedLink::GithubPr { repo, number, .. } => format!("PR #{number} {repo}"),
        ParsedLink::ClickupTask { task_id, .. } => format!("CU {task_id}"),
        ParsedLink::Other { url } => url
            .split("://")
            .nth(1)
            .and_then(|rest| rest.split('/').next())
            .unwrap_or(url)
            .to_string(),
    }
}

#[derive(Deserialize)]
struct GhPullRequest {
    title: String,
    number: u64,
    state: String,
    #[serde(rename = "isDraft")]
    is_draft: bool,
    author: GhAuthor,
    #[serde(default)]
    body: String,
}

#[derive(Deserialize)]
struct GhAuthor {
    login: String,
}

fn github_pr(url: &str, repo: &str, number: u64) -> Result<EnrichedLink> {
    let output = Command::new("gh")
        .args(["pr", "view", url, "--json", "title,number,state,isDraft,author,body"])
        .output()
        .context("não consegui executar o gh")?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    let pr: GhPullRequest = serde_json::from_slice(&output.stdout)?;
    let body: String = pr.body.chars().take(800).collect();
    Ok(EnrichedLink {
        link: Link {
            url: url.to_string(),
            kind: crate::models::LinkKind::GithubPr,
            label: format!("PR #{number} {repo}"),
        },
        context: format!(
            "Pull request do GitHub {url}\nRepositório: {repo}\nPR #{}: {}\nEstado: {}{}\nAutor: {}\nDescrição do PR: {body}",
            pr.number,
            pr.title,
            pr.state,
            if pr.is_draft { " (draft)" } else { "" },
            pr.author.login,
        ),
    })
}

#[derive(Deserialize)]
struct ClickupTask {
    name: String,
    #[serde(default)]
    text_content: Option<String>,
    status: Option<ClickupStatus>,
    list: Option<ClickupNamed>,
    priority: Option<ClickupNamedPriority>,
}

#[derive(Deserialize)]
struct ClickupStatus {
    status: String,
}

#[derive(Deserialize)]
struct ClickupNamed {
    name: String,
}

#[derive(Deserialize)]
struct ClickupNamedPriority {
    priority: String,
}

fn clickup_task(url: &str, team_id: Option<&str>, task_id: &str, token: &str) -> Result<EnrichedLink> {
    let mut request = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?
        .get(format!("https://api.clickup.com/api/v2/task/{task_id}"))
        .header("Authorization", token);
    if let Some(team_id) = team_id {
        request = request.query(&[("custom_task_ids", "true"), ("team_id", team_id)]);
    }
    let response = request.send()?;
    if !response.status().is_success() {
        bail!("ClickUp respondeu {}", response.status());
    }
    let task: ClickupTask = response.json()?;
    let description: String = task
        .text_content
        .unwrap_or_default()
        .chars()
        .take(CLICKUP_DESCRIPTION_LIMIT)
        .collect();
    let label = format!("CU {}", truncate(&task.name, 40));
    Ok(EnrichedLink {
        link: Link {
            url: url.to_string(),
            kind: crate::models::LinkKind::ClickupTask,
            label,
        },
        context: format!(
            "Task do ClickUp {url}\nNome: {}\nLista: {}\nStatus: {}\nPrioridade no ClickUp: {}\nDescrição: {description}",
            task.name,
            task.list.map(|list| list.name).unwrap_or_default(),
            task.status.map(|status| status.status).unwrap_or_default(),
            task.priority.map(|priority| priority.priority).unwrap_or_else(|| "nenhuma".into()),
        ),
    })
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max - 1).collect();
    format!("{cut}…")
}
