use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Urgent,
    High,
    Normal,
    Low,
}

impl Priority {
    pub fn rank(self) -> i64 {
        match self {
            Priority::Urgent => 1,
            Priority::High => 2,
            Priority::Normal => 3,
            Priority::Low => 4,
        }
    }

    pub fn from_rank(rank: i64) -> Self {
        match rank {
            1 => Priority::Urgent,
            2 => Priority::High,
            4 => Priority::Low,
            _ => Priority::Normal,
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim().to_lowercase().as_str() {
            "urgent" | "urgente" => Priority::Urgent,
            "high" | "alta" => Priority::High,
            "low" | "baixa" => Priority::Low,
            _ => Priority::Normal,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Priority::Urgent => Priority::High,
            Priority::High => Priority::Normal,
            Priority::Normal => Priority::Low,
            Priority::Low => Priority::Urgent,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Priority::Urgent => "URGENTE",
            Priority::High => "alta",
            Priority::Normal => "normal",
            Priority::Low => "baixa",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Open,
    Done,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Open => "open",
            Status::Done => "done",
        }
    }

    pub fn parse(value: &str) -> Self {
        if value == "done" { Status::Done } else { Status::Open }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    GithubPr,
    ClickupTask,
    Other,
}

impl LinkKind {
    pub fn as_str(self) -> &'static str {
        match self {
            LinkKind::GithubPr => "github_pr",
            LinkKind::ClickupTask => "clickup_task",
            LinkKind::Other => "other",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "github_pr" => LinkKind::GithubPr,
            "clickup_task" => LinkKind::ClickupTask,
            _ => LinkKind::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub url: String,
    pub kind: LinkKind,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Todo {
    pub id: i64,
    pub title: String,
    pub description: String,
    pub priority: Priority,
    pub status: Status,
    pub position: i64,
    pub raw_input: String,
    pub created_at: String,
    pub done_at: Option<String>,
    pub links: Vec<Link>,
}

#[derive(Debug, Clone)]
pub struct NewTodo {
    pub title: String,
    pub description: String,
    pub priority: Priority,
    pub raw_input: String,
    pub links: Vec<Link>,
}
