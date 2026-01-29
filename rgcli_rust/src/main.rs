mod api;
mod config;
mod ui;

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Parser;
use inquire::{Confirm, Password, Select, Text};

use crate::api::Client;
use crate::config::Config;
use crate::ui::{print_error, print_logo};

#[derive(Parser, Debug)]
#[command(name = "lsc", about = "CLI for chatting with LangSmith deployments")]
struct Cli {
    /// Resume an existing thread
    #[arg(long)]
    resume: bool,

    /// Show version
    #[arg(long)]
    version: bool,
}

const DEFAULT_ENDPOINT: &str =
    "https://chat-langchain-993a2fee078256ab879993a971197820.us.langgraph.app";
const DEFAULT_ASSISTANT: &str = "docs_agent";

#[derive(Clone, Copy)]
enum AuthOption {
    None,
    ApiKey,
    Headers,
}

impl AuthOption {
    fn key(&self) -> &'static str {
        match self {
            AuthOption::None => "none",
            AuthOption::ApiKey => "apikey",
            AuthOption::Headers => "headers",
        }
    }
}

impl std::fmt::Display for AuthOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthOption::None => write!(f, "None (public endpoint)"),
            AuthOption::ApiKey => write!(f, "LangSmith API Key"),
            AuthOption::Headers => write!(f, "Custom Headers"),
        }
    }
}

fn version_string() -> String {
    option_env!("LSC_VERSION")
        .unwrap_or(env!("CARGO_PKG_VERSION"))
        .to_string()
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.version {
        println!("lsc {}", version_string());
        return Ok(());
    }

    if let Err(err) = run(cli.resume).await {
        eprintln!("{}", print_error(&err.to_string()));
        std::process::exit(1);
    }

    Ok(())
}

async fn run(resume: bool) -> Result<()> {
    if !config::exists() {
        println!("Welcome to lsc! Let's configure your connection.\n");
        run_configure().await.context("configuration failed")?;
        println!();
    }

    let mut cfg = config::load().context("failed to load config")?;

    let config_path = config::config_path().unwrap_or_else(|_| "~/.lsc/config.yaml".into());
    print_logo(&version_string(), &cfg.endpoint, &config_path);
    println!();

    let mut client = Client::new(&cfg)?;

    let (thread_id, mut history) = if resume {
        match handle_resume(&client).await? {
            Some((thread_id, history)) => (thread_id, history),
            None => return Ok(()),
        }
    } else {
        let thread = client.create_thread().await?;
        (thread.thread_id, Vec::new())
    };

    loop {
        match ui::run_chat_loop(&client, &cfg.assistant_id, &thread_id, &history).await? {
            ui::ChatExit::Configure => {
                run_configure().await.context("configuration failed")?;
                cfg = config::load().context("failed to reload config")?;
                client = Client::new(&cfg)?;
                history.clear();
            }
            ui::ChatExit::Quit => return Ok(()),
        }
    }
}

async fn run_configure() -> Result<()> {
    let mut endpoint = DEFAULT_ENDPOINT.to_string();
    let mut api_key = String::new();
    let mut assistant_id = DEFAULT_ASSISTANT.to_string();
    let mut custom_headers: HashMap<String, String> = HashMap::new();
    if config::exists() {
        if let Ok(cfg) = config::load() {
            if !cfg.endpoint.is_empty() {
                endpoint = cfg.endpoint;
            }
            api_key = cfg.api_key;
            assistant_id = cfg.assistant_id;
            custom_headers = cfg.custom_headers;
        }
    }

    endpoint = Text::new("Endpoint URL")
        .with_help_message("Press Enter to accept default")
        .with_default(&endpoint)
        .prompt()?;

    if endpoint.trim().is_empty() {
        anyhow::bail!("endpoint URL is required");
    }

    let auth_start = if !api_key.is_empty() {
        1
    } else if !custom_headers.is_empty() {
        2
    } else {
        0
    };
    let auth_choice = Select::new(
        "Authentication",
        vec![AuthOption::None, AuthOption::ApiKey, AuthOption::Headers],
    )
    .with_help_message("How should we authenticate with this deployment?")
    .with_starting_cursor(auth_start)
    .prompt()?;
    let auth_type = auth_choice.key();

    match auth_type {
        "apikey" => {
            api_key = Password::new("LangSmith API Key")
                .with_help_message("Your API key (starts with lsv2_)")
                .prompt()?;
            custom_headers.clear();
        }
        "headers" => {
            api_key.clear();
            custom_headers.clear();
            loop {
                let header_name = Text::new("Header Name")
                    .with_placeholder("Authorization")
                    .prompt()?;
                if header_name.trim().is_empty() {
                    break;
                }
                let header_value = Password::new("Header Value").prompt()?;
                custom_headers.insert(header_name, header_value);
                let add_more = Confirm::new("Add another header?")
                    .with_default(true)
                    .prompt()?;
                if !add_more {
                    break;
                }
            }
        }
        _ => {
            api_key.clear();
            custom_headers.clear();
        }
    }

    assistant_id = Text::new("Assistant ID")
        .with_help_message("Press Enter to accept default")
        .with_default(&assistant_id)
        .prompt()?;
    if assistant_id.trim().is_empty() {
        assistant_id = DEFAULT_ASSISTANT.to_string();
    }

    let cfg = Config {
        endpoint,
        api_key,
        assistant_id,
        custom_headers,
    };

    config::save(&cfg)?;
    let path = config::config_path().unwrap_or_else(|_| "~/.lsc/config.yaml".into());
    println!("\nConfiguration saved to {}", path);

    Ok(())
}

async fn handle_resume(client: &Client) -> Result<Option<(String, Vec<api::Message>)>> {
    println!("Searching for threads...");
    let threads = client.search_threads(20).await?;

    if threads.is_empty() {
        println!("No existing threads found. Starting a new conversation.");
        let thread = client.create_thread().await?;
        return Ok(Some((thread.thread_id, Vec::new())));
    }

    print!("\x1b[F\x1b[K");
    let selected = ui::pick_thread(&threads)?;
    let Some(thread) = selected else {
        return Ok(None);
    };

    let history = match client.get_thread_state(&thread.thread_id).await {
        Ok(state) => api::get_messages(&state.values),
        Err(_) => Vec::new(),
    };

    Ok(Some((thread.thread_id, history)))
}
