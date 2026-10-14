mod ai;
mod config;
mod db;
mod enrich;
mod links;
mod models;
mod service;
mod tui;

use std::io::IsTerminal;
use std::process::{Command, Stdio};

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

use config::Config;
use db::Database;
use models::{Priority, Status, Todo};

#[derive(Parser)]
#[command(name = "t", version, about = "Pilha de tarefas local com IA")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    #[command(visible_alias = "a", about = "Adiciona um todo a partir de texto livre")]
    Add {
        #[arg(required = true, num_args = 1..)]
        text: Vec<String>,
        #[arg(long, help = "Não chama a IA; salva o texto como título")]
        no_ai: bool,
        #[arg(short, long, help = "Força a prioridade: urgent, high, normal, low")]
        priority: Option<String>,
    },
    #[command(visible_alias = "l", about = "Lista os todos")]
    Ls {
        #[arg(long, help = "Saída em JSON")]
        json: bool,
        #[arg(short, long, help = "Inclui concluídos")]
        all: bool,
    },
    #[command(visible_alias = "d", about = "Marca como concluído")]
    Done { ids: Vec<i64> },
    #[command(about = "Reabre um todo concluído")]
    Undo { ids: Vec<i64> },
    #[command(about = "Remove um todo")]
    Rm { ids: Vec<i64> },
    #[command(visible_alias = "o", about = "Abre os links do todo no navegador")]
    Open { id: i64 },
    #[command(visible_alias = "s", about = "Mostra os detalhes de um todo")]
    Show {
        id: i64,
        #[arg(long, help = "Saída em JSON")]
        json: bool,
    },
    #[command(about = "Edita título, descrição ou prioridade")]
    Edit {
        id: i64,
        #[arg(short, long)]
        title: Option<String>,
        #[arg(short, long)]
        description: Option<String>,
        #[arg(short, long)]
        priority: Option<String>,
    },
    #[command(about = "Configura a chave do DeepSeek e o token do ClickUp")]
    Config,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("erro: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    if let Some(Commands::Config) = cli.command {
        return config::run_interactive();
    }

    let config = Config::load()?;
    let mut database = Database::open(&config::database_path()?)?;

    match cli.command {
        None => tui::run(&mut database, &config),
        Some(Commands::Config) => unreachable!(),
        Some(Commands::Add { text, no_ai, priority }) => {
            let text = text.join(" ");
            let outcome = service::add(&mut database, &config, &text, !no_ai)?;
            if let Some(priority) = priority {
                database.set_priority(outcome.id, parse_priority(&priority)?)?;
            }
            if let Some(warning) = outcome.warning {
                eprintln!("aviso: {warning}");
            }
            let todo = database.get(outcome.id)?;
            print_todo(&todo, colors_enabled());
            Ok(())
        }
        Some(Commands::Ls { json, all }) => {
            let todos = database.list(all)?;
            if json {
                println!("{}", serde_json::to_string(&todos)?);
            } else if todos.is_empty() {
                println!("Pilha vazia. Adicione com: t add \"o que fazer\"");
            } else {
                let colors = colors_enabled();
                for todo in &todos {
                    print_todo(todo, colors);
                }
            }
            Ok(())
        }
        Some(Commands::Done { ids }) => for_each_id(&ids, |id| database.set_status(id, Status::Done), "concluído"),
        Some(Commands::Undo { ids }) => for_each_id(&ids, |id| database.set_status(id, Status::Open), "reaberto"),
        Some(Commands::Rm { ids }) => for_each_id(&ids, |id| database.delete(id), "removido"),
        Some(Commands::Open { id }) => {
            let todo = database.get(id)?;
            if todo.links.is_empty() {
                bail!("todo #{id} não tem links");
            }
            for link in &todo.links {
                open_url(&link.url)?;
            }
            Ok(())
        }
        Some(Commands::Show { id, json }) => {
            let todo = database.get(id)?;
            if json {
                println!("{}", serde_json::to_string(&todo)?);
                return Ok(());
            }
            print_todo(&todo, colors_enabled());
            if !todo.description.is_empty() {
                println!("\n{}", todo.description);
            }
            for link in &todo.links {
                println!("  {} → {}", link.label, link.url);
            }
            println!("\nTexto original: {}", todo.raw_input);
            println!("Criado em: {}", todo.created_at);
            Ok(())
        }
        Some(Commands::Edit { id, title, description, priority }) => {
            let todo = database.get(id)?;
            if title.is_some() || description.is_some() {
                database.update_text(
                    id,
                    title.as_deref().unwrap_or(&todo.title),
                    description.as_deref().unwrap_or(&todo.description),
                )?;
            }
            if let Some(priority) = priority {
                database.set_priority(id, parse_priority(&priority)?)?;
            }
            print_todo(&database.get(id)?, colors_enabled());
            Ok(())
        }
    }
}

fn for_each_id(ids: &[i64], mut action: impl FnMut(i64) -> Result<()>, verb: &str) -> Result<()> {
    if ids.is_empty() {
        bail!("informe pelo menos um id");
    }
    for &id in ids {
        action(id)?;
        println!("#{id} {verb}");
    }
    Ok(())
}

fn parse_priority(value: &str) -> Result<Priority> {
    match value.trim().to_lowercase().as_str() {
        "urgent" | "urgente" | "u" | "1" => Ok(Priority::Urgent),
        "high" | "alta" | "h" | "2" => Ok(Priority::High),
        "normal" | "n" | "3" => Ok(Priority::Normal),
        "low" | "baixa" | "l" | "4" => Ok(Priority::Low),
        other => bail!("prioridade desconhecida: {other} (use urgent, high, normal, low)"),
    }
}

pub fn open_url(url: &str) -> Result<()> {
    Command::new("xdg-open")
        .arg(url)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

fn colors_enabled() -> bool {
    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

fn priority_color(priority: Priority) -> &'static str {
    match priority {
        Priority::Urgent => "\x1b[1;31m",
        Priority::High => "\x1b[33m",
        Priority::Normal => "\x1b[36m",
        Priority::Low => "\x1b[2m",
    }
}

fn print_todo(todo: &Todo, colors: bool) {
    let (color, dim, reset) = if colors {
        (priority_color(todo.priority), "\x1b[2m", "\x1b[0m")
    } else {
        ("", "", "")
    };
    let check = if todo.status == Status::Done { "✔ " } else { "" };
    let badges: Vec<&str> = todo.links.iter().map(|link| link.label.as_str()).collect();
    let badges = if badges.is_empty() {
        String::new()
    } else {
        format!(" {dim}[{}]{reset}", badges.join(", "))
    };
    println!(
        "{dim}#{:<4}{reset}{color}{:<8}{reset} {check}{}{badges}",
        todo.id,
        todo.priority.label(),
        todo.title
    );
}
