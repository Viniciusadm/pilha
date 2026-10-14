use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

pub const DEFAULT_MODEL: &str = "deepseek-chat";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    pub deepseek_api_key: Option<String>,
    pub deepseek_model: Option<String>,
    pub clickup_token: Option<String>,
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        let mut config: Config = if path.exists() {
            let content = fs::read_to_string(&path)
                .with_context(|| format!("não consegui ler {}", path.display()))?;
            toml::from_str(&content)
                .with_context(|| format!("config inválido em {}", path.display()))?
        } else {
            Config::default()
        };
        if let Ok(key) = std::env::var("DEEPSEEK_API_KEY") {
            config.deepseek_api_key = Some(key);
        }
        if let Ok(token) = std::env::var("CLICKUP_TOKEN") {
            config.clickup_token = Some(token);
        }
        Ok(config)
    }

    pub fn save(&self) -> Result<PathBuf> {
        let path = config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, toml::to_string_pretty(self)?)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        Ok(path)
    }

    pub fn model(&self) -> &str {
        self.deepseek_model
            .as_deref()
            .filter(|model| !model.is_empty())
            .unwrap_or(DEFAULT_MODEL)
    }
}

fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("", "", "pilha").context("não consegui descobrir o diretório home")
}

pub fn config_path() -> Result<PathBuf> {
    Ok(project_dirs()?.config_dir().join("config.toml"))
}

pub fn database_path() -> Result<PathBuf> {
    let dir = project_dirs()?.data_dir().to_path_buf();
    fs::create_dir_all(&dir)?;
    Ok(dir.join("pilha.db"))
}

pub fn run_interactive() -> Result<()> {
    let path = config_path()?;
    let mut config: Config = if path.exists() {
        toml::from_str(&fs::read_to_string(&path)?)?
    } else {
        Config::default()
    };

    println!("Enter mantém o valor atual.");
    config.deepseek_api_key = ask("Chave da API do DeepSeek", config.deepseek_api_key, true)?;
    config.deepseek_model = ask("Modelo do DeepSeek", config.deepseek_model.or(Some(DEFAULT_MODEL.into())), false)?;
    config.clickup_token = ask("Token pessoal do ClickUp (pk_...)", config.clickup_token, true)?;

    let saved = config.save()?;
    println!("Salvo em {} (permissão 600).", saved.display());
    Ok(())
}

fn ask(label: &str, current: Option<String>, secret: bool) -> Result<Option<String>> {
    let shown = match (&current, secret) {
        (Some(value), true) if !value.is_empty() => format!(" [{}]", mask(value)),
        (Some(value), false) if !value.is_empty() => format!(" [{value}]"),
        _ => String::new(),
    };
    print!("{label}{shown}: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim();
    Ok(if input.is_empty() { current } else { Some(input.to_string()) })
}

fn mask(value: &str) -> String {
    let visible: String = value.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    format!("****{visible}")
}
