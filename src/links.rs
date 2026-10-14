use std::sync::LazyLock;

use regex::Regex;

use crate::models::LinkKind;

static URL_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"https?://[^\s<>"'()]+"#).unwrap());

static GITHUB_PR_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^https?://github\.com/([^/]+)/([^/]+)/pull/(\d+)").unwrap()
});

static CLICKUP_TASK_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^https?://app\.clickup\.com/t/(?:(\d+)/)?([A-Za-z0-9_-]+)").unwrap()
});

#[derive(Debug, Clone, PartialEq)]
pub enum ParsedLink {
    GithubPr {
        url: String,
        owner: String,
        repo: String,
        number: u64,
    },
    ClickupTask {
        url: String,
        team_id: Option<String>,
        task_id: String,
    },
    Other {
        url: String,
    },
}

impl ParsedLink {
    pub fn url(&self) -> &str {
        match self {
            ParsedLink::GithubPr { url, .. }
            | ParsedLink::ClickupTask { url, .. }
            | ParsedLink::Other { url } => url,
        }
    }

    pub fn kind(&self) -> LinkKind {
        match self {
            ParsedLink::GithubPr { .. } => LinkKind::GithubPr,
            ParsedLink::ClickupTask { .. } => LinkKind::ClickupTask,
            ParsedLink::Other { .. } => LinkKind::Other,
        }
    }
}

pub fn extract_links(text: &str) -> Vec<ParsedLink> {
    let mut links: Vec<ParsedLink> = Vec::new();
    for found in URL_PATTERN.find_iter(text) {
        let url = found.as_str().trim_end_matches(['.', ',', ';', ':', '!', '?']);
        if links.iter().any(|link| link.url() == url) {
            continue;
        }
        links.push(classify(url));
    }
    links
}

pub fn strip_links(text: &str) -> String {
    URL_PATTERN.replace_all(text, "").into_owned()
}

pub fn classify(url: &str) -> ParsedLink {
    if let Some(captures) = GITHUB_PR_PATTERN.captures(url) {
        return ParsedLink::GithubPr {
            url: url.to_string(),
            owner: captures[1].to_string(),
            repo: captures[2].to_string(),
            number: captures[3].parse().unwrap_or(0),
        };
    }
    if let Some(captures) = CLICKUP_TASK_PATTERN.captures(url) {
        return ParsedLink::ClickupTask {
            url: url.to_string(),
            team_id: captures.get(1).map(|m| m.as_str().to_string()),
            task_id: captures[2].to_string(),
        };
    }
    ParsedLink::Other {
        url: url.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_and_classifies_github_pr() {
        let links = extract_links("revisar https://github.com/org/repo/pull/123, urgente");
        assert_eq!(
            links,
            vec![ParsedLink::GithubPr {
                url: "https://github.com/org/repo/pull/123".into(),
                owner: "org".into(),
                repo: "repo".into(),
                number: 123,
            }]
        );
    }

    #[test]
    fn classifies_clickup_task_with_and_without_team() {
        assert_eq!(
            classify("https://app.clickup.com/t/86abc123"),
            ParsedLink::ClickupTask {
                url: "https://app.clickup.com/t/86abc123".into(),
                team_id: None,
                task_id: "86abc123".into(),
            }
        );
        assert_eq!(
            classify("https://app.clickup.com/t/1234/ABC-42"),
            ParsedLink::ClickupTask {
                url: "https://app.clickup.com/t/1234/ABC-42".into(),
                team_id: Some("1234".into()),
                task_id: "ABC-42".into(),
            }
        );
    }

    #[test]
    fn keeps_unknown_links_and_deduplicates() {
        let links = extract_links("ver https://example.com/a e https://example.com/a de novo");
        assert_eq!(
            links,
            vec![ParsedLink::Other {
                url: "https://example.com/a".into()
            }]
        );
    }

    #[test]
    fn returns_empty_without_links() {
        assert!(extract_links("comprar café").is_empty());
    }
}
