mod api;
mod config;
mod context;
mod langsmith;
mod ui;
mod update;

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use inquire::{Confirm, Password, Select, Text};

use crate::api::Client;
use crate::config::Config;
use crate::ui::{print_error, print_logo, system_text};

#[derive(Parser, Debug)]
#[command(name = "ailsd", about = "CLI for chatting with LangSmith deployments")]
struct Cli {
    /// Resume an existing thread
    #[arg(long)]
    resume: bool,

    /// Show version
    #[arg(long)]
    version: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Upgrade to the latest version
    Upgrade,
    /// Manage deployment contexts
    Context {
        #[command(subcommand)]
        action: ContextAction,
    },
}

#[derive(Subcommand, Debug)]
enum ContextAction {
    /// List all contexts
    List,
    /// Show active context
    Current,
    /// Switch active context
    Use { name: String },
    /// Create a new context
    Create { name: String },
    /// Show context details
    Show { name: Option<String> },
    /// Delete a context
    Delete { name: String },
}

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
    option_env!("AILSD_VERSION")
        .unwrap_or(env!("CARGO_PKG_VERSION"))
        .to_string()
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.version {
        println!("ailsd {}", version_string());
        return Ok(());
    }

    match cli.command {
        Some(Command::Upgrade) => {
            if let Err(err) = update::run_upgrade().await {
                eprintln!("{}", print_error(&err.to_string()));
                std::process::exit(1);
            }
            return Ok(());
        }
        Some(Command::Context { action }) => {
            let result = match action {
                ContextAction::List => context::list(),
                ContextAction::Current => context::current(),
                ContextAction::Use { name } => context::use_context(&name),
                ContextAction::Create { name } => context::create_interactive(&name),
                ContextAction::Show { name } => context::show(name.as_deref()),
                ContextAction::Delete { name } => context::delete(&name),
            };
            if let Err(err) = result {
                eprintln!("{}", print_error(&err.to_string()));
                std::process::exit(1);
            }
            return Ok(());
        }
        None => {}
    }

    if let Err(err) = run(cli.resume).await {
        eprintln!("{}", print_error(&err.to_string()));
        std::process::exit(1);
    }

    Ok(())
}

async fn run(resume: bool) -> Result<()> {
    if !config::exists() {
        println!("Welcome to ailsd! Let's configure your first context.\n");
        let cfg = run_configure_inner(None).context("configuration failed")?;
        config::save_context("default", &cfg)?;
        println!();
    }

    let mut cfg = config::load().context("failed to load config")?;

    // Background update check (fire and forget)
    tokio::spawn(async {
        let _ = update::background_check().await;
    });

    let config_path = config::config_path().unwrap_or_else(|_| "~/.ailsd/config.yaml".into());

    // Determine context info for display
    let context_info = match config::load_with_source() {
        Ok((_, config::ConfigSource::Local(path))) => {
            format!("context: local ({})", path.display())
        }
        Ok((_, config::ConfigSource::Global(name))) => {
            format!("context: {}", name)
        }
        Err(_) => "context: default".to_string(),
    };

    print_logo(&version_string(), &cfg.endpoint, &config_path, &context_info);

    if let Some(notice) = update::pending_update_notice() {
        println!("  {}", system_text(&notice));
    }
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
                let context_name = config::current_context_name();
                let new_cfg = run_configure_inner(Some(&cfg)).context("configuration failed")?;
                config::save_context(&context_name, &new_cfg)?;
                cfg = new_cfg;
                client = Client::new(&cfg)?;
                history.clear();
            }
            ui::ChatExit::Quit => return Ok(()),
        }
    }
}

/// Interactive configure flow. Returns a Config. Used by both initial setup and context creation.
pub fn run_configure_inner(existing: Option<&Config>) -> Result<Config> {
    let mut endpoint = String::new();
    let mut api_key = String::new();
    let mut assistant_id = DEFAULT_ASSISTANT.to_string();
    let mut custom_headers: HashMap<String, String> = HashMap::new();

    if let Some(cfg) = existing {
        if !cfg.endpoint.is_empty() {
            endpoint = cfg.endpoint.clone();
        }
        api_key = cfg.api_key.clone();
        assistant_id = cfg.assistant_id.clone();
        custom_headers = cfg.custom_headers.clone();
    }

    // --- Authentication ---
    let auth_start = if !api_key.is_empty() || std::env::var("LANGSMITH_API_KEY").is_ok() {
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
            let env_key = std::env::var("LANGSMITH_API_KEY").unwrap_or_default();
            let has_env = !env_key.is_empty();
            let has_stored = !api_key.is_empty();

            if has_env && !has_stored {
                let use_env = Confirm::new("Use LANGSMITH_API_KEY from environment?")
                    .with_default(true)
                    .prompt()?;
                if !use_env {
                    api_key = Password::new("LangSmith API Key")
                        .with_help_message("Your API key (starts with lsv2_)")
                        .prompt()?;
                } else {
                    api_key = String::new();
                }
            } else {
                let mut pwd = Password::new("LangSmith API Key")
                    .with_help_message("Your API key (starts with lsv2_)");
                if has_env {
                    pwd = pwd.with_help_message(
                        "Your API key (starts with lsv2_). Leave empty to use LANGSMITH_API_KEY env var.",
                    );
                }
                api_key = pwd.prompt()?;
            }
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

    // --- Endpoint: offer deployment search if we have an API key ---
    let effective_key = if api_key.is_empty() {
        std::env::var("LANGSMITH_API_KEY").unwrap_or_default()
    } else {
        api_key.clone()
    };

    if !effective_key.is_empty() && endpoint.is_empty() {
        let search = Confirm::new("Search LangSmith for deployments?")
            .with_default(true)
            .prompt()?;
        if search {
            endpoint = search_and_pick_deployment(&effective_key)?;
        }
    }

    if endpoint.is_empty() {
        let mut endpoint_prompt = Text::new("Endpoint URL")
            .with_placeholder("https://your-deployment.langgraph.app")
            .with_help_message("Your LangGraph deployment URL");
        if let Some(cfg) = existing {
            if !cfg.endpoint.is_empty() {
                endpoint_prompt = endpoint_prompt.with_default(&cfg.endpoint);
            }
        }
        endpoint = endpoint_prompt.prompt()?;
    }

    if endpoint.trim().is_empty() {
        anyhow::bail!("endpoint URL is required");
    }

    // --- Assistant ID ---
    assistant_id = Text::new("Assistant ID")
        .with_help_message("Press Enter to accept default")
        .with_default(&assistant_id)
        .prompt()?;
    if assistant_id.trim().is_empty() {
        assistant_id = DEFAULT_ASSISTANT.to_string();
    }

    Ok(Config {
        endpoint,
        api_key,
        assistant_id,
        custom_headers,
    })
}

fn search_and_pick_deployment(api_key: &str) -> Result<String> {
    let rt = tokio::runtime::Handle::current();
    loop {
        let query = Text::new("Search deployments")
            .with_help_message("Type a name to search (or leave empty to list all)")
            .prompt()?;

        let deployments = rt.block_on(langsmith::search_deployments(api_key, query.trim()))?;

        if deployments.is_empty() {
            println!("No deployments found.");
            let retry = Confirm::new("Try another search?")
                .with_default(true)
                .prompt()?;
            if retry {
                continue;
            }
            return Ok(String::new());
        }

        // Filter to deployments that have a URL
        let with_url: Vec<&langsmith::Deployment> =
            deployments.iter().filter(|d| d.url().is_some()).collect();

        if with_url.is_empty() {
            println!("Found {} deployments but none have a URL configured.", deployments.len());
            let retry = Confirm::new("Try another search?")
                .with_default(true)
                .prompt()?;
            if retry {
                continue;
            }
            return Ok(String::new());
        }

        let labels: Vec<String> = with_url.iter().map(|d| d.to_string()).collect();
        let selection = Select::new("Select deployment", labels).prompt()?;

        // Find the selected deployment by matching the display string
        let selected = with_url
            .iter()
            .find(|d| d.to_string() == selection)
            .unwrap();

        let url = selected.url().unwrap().to_string();
        println!("Selected: {}", url);
        return Ok(url);
    }
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
