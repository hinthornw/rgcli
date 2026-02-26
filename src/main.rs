mod api;
mod bench;
mod commands;
mod config;
mod context;
mod debug_log;
mod deploy;
mod langsmith;
mod ui;
mod update;

use std::collections::HashMap;
use std::io::{self, IsTerminal, Read as _};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use inquire::{Confirm, Password, Select, Text};

use crate::api::Client;
use crate::config::Config;
use crate::ui::print_error;

#[derive(Parser, Debug)]
#[command(
    name = "ailsd",
    about = "Your friendly neighborhood LangGraph CLI",
    long_about = "Chat, debug, load test, and manage LangGraph deployments — all from your terminal.\n\n\
        Works out of the box with Chat Langchain (Q&A over LangChain docs).\n\
        Run 'ailsd context create <name>' to connect your own deployment.",
    after_help = "\x1b[1;35m~ Examples ~\x1b[0m

  \x1b[2m# Interactive chat (the fun part)\x1b[0m
  ailsd

  \x1b[2m# Resume where you left off\x1b[0m
  ailsd --resume

  \x1b[2m# Pipe mode — great for scripts\x1b[0m
  echo \"what is langgraph?\" | ailsd
  echo \"explain agents\" | ailsd --json | jq .

  \x1b[2m# Explore your deployment\x1b[0m
  ailsd assistants list
  ailsd threads list
  ailsd assistants graph agent --ascii

  \x1b[2m# Load test like a pro\x1b[0m
  ailsd bench --concurrent 10 --requests 50

  \x1b[2m# Debug runs and traces\x1b[0m
  ailsd logs --last 10
  ailsd runs list <thread-id>

  \x1b[2m# Manage contexts (like kubectl)\x1b[0m
  ailsd context create production
  ailsd context use production

\x1b[1;35m~ Interactive keys ~\x1b[0m
  Enter          Send message
  Alt+Enter      Insert newline
  Esc Esc        Cancel streaming
  F12            Toggle devtools
  Ctrl+C Ctrl+C  Quit"
)]
struct Cli {
    /// Resume a previous chat thread
    #[arg(long)]
    resume: bool,

    /// Show version
    #[arg(long)]
    version: bool,

    /// Thread ID to use (creates new thread if omitted)
    #[arg(long, value_name = "ID")]
    thread_id: Option<String>,

    /// Output raw JSON instead of extracted text (pipe mode only)
    #[arg(long)]
    json: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Upgrade to the latest version
    Upgrade,
    /// Remove ailsd from your system
    Uninstall,
    /// View or modify global settings
    Settings {
        /// Setting to change (e.g. auto_update=false)
        #[arg(value_name = "KEY=VALUE")]
        set: Option<String>,
    },
    /// Diagnose deployment connectivity and configuration
    Doctor,
    /// Manage deployment contexts (like kubectl config)
    #[command(after_help = "\x1b[1mExamples:\x1b[0m
  ailsd context create staging
  ailsd context use staging
  ailsd context list
  ailsd context show production
  ailsd context delete old-context")]
    Context {
        #[command(subcommand)]
        action: ContextAction,
    },
    /// Manage assistants
    Assistants {
        #[command(subcommand)]
        action: AssistantAction,
    },
    /// Manage threads
    Threads {
        #[command(subcommand)]
        action: ThreadAction,
    },
    /// Manage runs
    Runs {
        #[command(subcommand)]
        action: RunAction,
    },
    /// Persistent key-value store operations
    Store {
        #[command(subcommand)]
        action: StoreAction,
    },
    /// Manage cron jobs
    Crons {
        #[command(subcommand)]
        action: CronAction,
    },
    /// Load test a deployment
    Bench {
        /// Number of concurrent requests
        #[arg(long, default_value = "5")]
        concurrent: usize,
        /// Total number of requests
        #[arg(long, default_value = "20")]
        requests: usize,
        /// Input message to send
        #[arg(long, default_value = "hello")]
        input: String,
        /// File with inputs (one per line)
        #[arg(long)]
        input_file: Option<String>,
    },
    /// View run logs and traces
    Logs {
        /// Thread ID to show logs for
        #[arg(long)]
        thread: Option<String>,
        /// Run ID to show details for
        #[arg(long)]
        run: Option<String>,
        /// Show last N runs
        #[arg(long, default_value = "5")]
        last: usize,
    },
    /// Launch API server with Docker
    Up {
        /// Path to configuration file declaring dependencies, graphs and environment variables
        #[arg(short, long, default_value = "langgraph.json")]
        config: String,

        /// Port to expose
        #[arg(short, long, default_value_t = 8123)]
        port: u16,

        /// Path to docker-compose.yml file with additional services
        #[arg(short, long)]
        docker_compose: Option<String>,

        /// Show detailed output
        #[arg(short, long)]
        verbose: bool,

        /// Restart on file changes using docker compose watch
        #[arg(short, long)]
        watch: bool,

        /// Recreate containers even if configuration hasn't changed
        #[arg(long)]
        recreate: bool,

        /// Skip pulling latest images before running
        #[arg(long)]
        no_pull: bool,

        /// Wait for services to be healthy before returning
        #[arg(long)]
        wait: bool,

        /// Port to expose the debugger on
        #[arg(long)]
        debugger_port: Option<u16>,

        /// Base URL for the debugger
        #[arg(long)]
        debugger_base_url: Option<String>,

        /// Postgres connection URI
        #[arg(long)]
        postgres_uri: Option<String>,

        /// API version of the LangGraph server
        #[arg(long)]
        api_version: Option<String>,

        /// Pre-built image to use instead of building
        #[arg(long)]
        image: Option<String>,

        /// Base image for the LangGraph API server
        #[arg(long)]
        base_image: Option<String>,
    },

    /// Build API server Docker image
    Build {
        /// Path to configuration file
        #[arg(short, long, default_value = "langgraph.json")]
        config: String,

        /// Tag for the docker image
        #[arg(short, long)]
        tag: String,

        /// Skip pulling latest images before building
        #[arg(long)]
        no_pull: bool,

        /// Base image for the LangGraph API server
        #[arg(long)]
        base_image: Option<String>,

        /// API version of the LangGraph server
        #[arg(long)]
        api_version: Option<String>,

        /// Custom install command
        #[arg(long)]
        install_command: Option<String>,

        /// Custom build command
        #[arg(long)]
        build_command: Option<String>,

        /// Additional arguments to pass to docker build
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        docker_build_args: Vec<String>,
    },

    /// Generate a Dockerfile for the API server
    Dockerfile {
        /// Path to save the generated Dockerfile
        save_path: String,

        /// Path to configuration file
        #[arg(short, long, default_value = "langgraph.json")]
        config: String,

        /// Add docker-compose.yml, .env, and .dockerignore files
        #[arg(long)]
        add_docker_compose: bool,

        /// Base image for the LangGraph API server
        #[arg(long)]
        base_image: Option<String>,

        /// API version of the LangGraph server
        #[arg(long)]
        api_version: Option<String>,
    },

    /// Run API server in development mode
    Dev {
        /// Network interface to bind to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,

        /// Port number
        #[arg(long, default_value_t = 2024)]
        port: u16,

        /// Disable automatic reloading
        #[arg(long)]
        no_reload: bool,

        /// Path to configuration file
        #[arg(short, long, default_value = "langgraph.json")]
        config: String,

        /// Max concurrent jobs per worker
        #[arg(long)]
        n_jobs_per_worker: Option<u32>,

        /// Skip opening browser
        #[arg(long)]
        no_browser: bool,

        /// Enable remote debugging on specified port
        #[arg(long)]
        debug_port: Option<u16>,

        /// Wait for debugger client to connect
        #[arg(long)]
        wait_for_client: bool,

        /// URL of LangGraph Studio
        #[arg(long)]
        studio_url: Option<String>,

        /// Allow synchronous I/O blocking operations
        #[arg(long)]
        allow_blocking: bool,

        /// Expose via public tunnel
        #[arg(long)]
        tunnel: bool,

        /// Log level for the API server
        #[arg(long, default_value = "WARNING")]
        server_log_level: String,
    },

    /// Create a new project from a template
    New {
        /// Path to create the project
        path: Option<String>,

        /// Template to use
        #[arg(long)]
        template: Option<String>,
    },

    /// Deploy to LangSmith cloud
    Deploy {
        /// Path to configuration file
        #[arg(short, long, default_value = "langgraph.json")]
        config: String,

        /// Deployment name
        #[arg(short, long)]
        name: String,

        /// Deployment type (dev or prod)
        #[arg(long, default_value = "dev")]
        deployment_type: String,

        /// Base image for the API server
        #[arg(long)]
        base_image: Option<String>,

        /// API version of the server
        #[arg(long)]
        api_version: Option<String>,

        /// Pre-built image to push (skip building)
        #[arg(long)]
        image: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum AssistantAction {
    /// List all assistants
    List,
    /// Get assistant details
    Get { id: String },
    /// Show assistant graph structure
    Graph {
        id: String,
        /// Render as ASCII art instead of JSON
        #[arg(long)]
        ascii: bool,
    },
    /// Show state and config schemas
    Schemas { id: String },
    /// List assistant versions
    Versions { id: String },
}

#[derive(Subcommand, Debug)]
enum ThreadAction {
    /// List threads
    List {
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Get thread details
    Get { id: String },
    /// Create a new thread
    Create,
    /// Delete a thread
    Delete { id: String },
    /// Show thread state
    State { id: String },
    /// Show thread checkpoint history
    History {
        id: String,
        #[arg(long, default_value = "10")]
        limit: usize,
    },
    /// Copy/fork a thread
    Copy { id: String },
    /// Prune old checkpoints from a thread
    Prune { id: String },
}

#[derive(Subcommand, Debug)]
enum RunAction {
    /// List runs for a thread
    List {
        thread_id: String,
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Get run details
    Get { thread_id: String, run_id: String },
    /// Cancel a running run
    Cancel { thread_id: String, run_id: String },
    /// Watch runs for a thread (live status updates)
    Watch {
        thread_id: String,
        #[arg(long, default_value = "2")]
        interval: u64,
    },
}

#[derive(Subcommand, Debug)]
enum StoreAction {
    /// Get an item from the store
    Get { namespace: String, key: String },
    /// Put an item in the store
    Put {
        namespace: String,
        key: String,
        #[arg(long)]
        value: String,
    },
    /// Delete an item from the store
    Delete { namespace: String, key: String },
    /// Search items in a namespace
    Search {
        namespace: String,
        #[arg(long)]
        query: Option<String>,
        #[arg(long, default_value = "10")]
        limit: usize,
    },
    /// List namespaces
    Namespaces,
}

#[derive(Subcommand, Debug)]
enum CronAction {
    /// List cron jobs
    List {
        #[arg(long)]
        assistant: Option<String>,
    },
    /// Create a cron job
    Create {
        #[arg(long)]
        assistant: String,
        #[arg(long)]
        schedule: String,
    },
    /// Delete a cron job
    Delete { id: String },
}

#[derive(Subcommand, Debug)]
enum ContextAction {
    /// List all contexts (* marks active)
    List,
    /// Show which context is active and where it comes from
    Current,
    /// Switch the active context
    Use {
        /// Context name to switch to
        name: String,
    },
    /// Create a new context interactively
    Create {
        /// Name for the new context
        name: String,
    },
    /// Show context details (API key is masked)
    Show {
        /// Context name (defaults to active context)
        name: Option<String>,
    },
    /// Delete a context
    Delete {
        /// Context name to delete
        name: String,
    },
}

const DEFAULT_ASSISTANT: &str = "agent";

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
    config::ensure_settings_file();
    debug_log::reset();
    debug_log::log("main", &format!("ailsd {} starting", version_string()));

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
        Some(Command::Settings { set }) => {
            if let Some(kv) = set {
                if let Some((key, value)) = kv.split_once('=') {
                    let mut settings = config::load_settings();
                    match key.trim() {
                        "auto_update" => {
                            settings.auto_update = value.trim().parse().unwrap_or_else(|_| {
                                eprintln!("Invalid value for auto_update, expected true/false");
                                std::process::exit(1);
                            });
                        }
                        other => {
                            eprintln!("Unknown setting: {other}");
                            eprintln!("Available: auto_update");
                            std::process::exit(1);
                        }
                    }
                    if let Err(e) = config::save_settings(&settings) {
                        eprintln!("{}", print_error(&e.to_string()));
                        std::process::exit(1);
                    }
                    println!("Updated: {key} = {value}");
                } else {
                    eprintln!("Expected KEY=VALUE format (e.g. auto_update=false)");
                    std::process::exit(1);
                }
            } else {
                let settings = config::load_settings();
                println!("auto_update = {}", settings.auto_update);
                println!("\nSettings file: {}", config::config_dir().map(|d| d.join("settings.yaml").to_string_lossy().to_string()).unwrap_or_else(|_| "?".to_string()));
            }
            return Ok(());
        }
        Some(Command::Uninstall) => {
            if let Err(err) = commands::uninstall::run() {
                eprintln!("{}", print_error(&err.to_string()));
                std::process::exit(1);
            }
            return Ok(());
        }
        Some(Command::Doctor) => {
            if let Err(err) = run_doctor().await {
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
        Some(Command::Assistants { action }) => {
            return run_sdk_command(|client| async move {
                match action {
                    AssistantAction::List => commands::assistants::list(&client).await,
                    AssistantAction::Get { id } => commands::assistants::get(&client, &id).await,
                    AssistantAction::Graph { id, ascii } => {
                        commands::assistants::graph(&client, &id, ascii).await
                    }
                    AssistantAction::Schemas { id } => {
                        commands::assistants::schemas(&client, &id).await
                    }
                    AssistantAction::Versions { id } => {
                        commands::assistants::versions(&client, &id).await
                    }
                }
            })
            .await;
        }
        Some(Command::Threads { action }) => {
            return run_sdk_command(|client| async move {
                match action {
                    ThreadAction::List { limit } => commands::threads::list(&client, limit).await,
                    ThreadAction::Get { id } => commands::threads::get(&client, &id).await,
                    ThreadAction::Create => commands::threads::create(&client).await,
                    ThreadAction::Delete { id } => commands::threads::delete(&client, &id).await,
                    ThreadAction::State { id } => commands::threads::state(&client, &id).await,
                    ThreadAction::History { id, limit } => {
                        commands::threads::history(&client, &id, limit).await
                    }
                    ThreadAction::Copy { id } => commands::threads::copy(&client, &id).await,
                    ThreadAction::Prune { id } => commands::threads::prune(&client, &id).await,
                }
            })
            .await;
        }
        Some(Command::Runs { action }) => {
            return run_sdk_command(|client| async move {
                match action {
                    RunAction::List { thread_id, limit } => {
                        commands::runs::list(&client, &thread_id, limit).await
                    }
                    RunAction::Get { thread_id, run_id } => {
                        commands::runs::get(&client, &thread_id, &run_id).await
                    }
                    RunAction::Cancel { thread_id, run_id } => {
                        commands::runs::cancel(&client, &thread_id, &run_id).await
                    }
                    RunAction::Watch {
                        thread_id,
                        interval,
                    } => commands::runs::watch(&client, &thread_id, interval).await,
                }
            })
            .await;
        }
        Some(Command::Store { action }) => {
            return run_sdk_command(|client| async move {
                match action {
                    StoreAction::Get { namespace, key } => {
                        commands::store::get_item(&client, &namespace, &key).await
                    }
                    StoreAction::Put {
                        namespace,
                        key,
                        value,
                    } => commands::store::put_item(&client, &namespace, &key, &value).await,
                    StoreAction::Delete { namespace, key } => {
                        commands::store::delete_item(&client, &namespace, &key).await
                    }
                    StoreAction::Search {
                        namespace,
                        query,
                        limit,
                    } => {
                        commands::store::search(&client, &namespace, query.as_deref(), limit).await
                    }
                    StoreAction::Namespaces => commands::store::namespaces(&client).await,
                }
            })
            .await;
        }
        Some(Command::Crons { action }) => {
            return run_sdk_command(|client| async move {
                match action {
                    CronAction::List { assistant } => {
                        commands::crons::list(&client, assistant.as_deref()).await
                    }
                    CronAction::Create {
                        assistant,
                        schedule,
                    } => commands::crons::create(&client, &assistant, &schedule).await,
                    CronAction::Delete { id } => commands::crons::delete(&client, &id).await,
                }
            })
            .await;
        }
        Some(Command::Bench {
            concurrent,
            requests,
            input,
            input_file,
        }) => {
            return run_sdk_command(|client| async move {
                let inputs = if let Some(path) = input_file {
                    let content = std::fs::read_to_string(&path)
                        .with_context(|| format!("failed to read {}", path))?;
                    content
                        .lines()
                        .filter(|l| !l.trim().is_empty())
                        .map(String::from)
                        .collect()
                } else {
                    vec![input]
                };
                let cfg = config::load()?;
                commands::bench::run(&client, &cfg.assistant_id, concurrent, requests, inputs).await
            })
            .await;
        }
        Some(Command::Logs { thread, run, last }) => {
            return run_sdk_command(|client| async move {
                commands::logs::show(&client, thread.as_deref(), run.as_deref(), last).await
            })
            .await;
        }
        Some(Command::Up {
            config,
            port,
            docker_compose,
            verbose,
            watch,
            recreate,
            no_pull,
            wait,
            debugger_port,
            debugger_base_url,
            postgres_uri,
            api_version,
            image,
            base_image,
        }) => {
            let result = commands::deployment::up(
                &config,
                port,
                docker_compose.as_deref(),
                verbose,
                watch,
                recreate,
                !no_pull,
                wait,
                debugger_port,
                debugger_base_url.as_deref(),
                postgres_uri.as_deref(),
                api_version.as_deref(),
                image.as_deref(),
                base_image.as_deref(),
            );
            if let Err(err) = result {
                eprintln!("{}", print_error(&err));
                std::process::exit(1);
            }
            return Ok(());
        }
        Some(Command::Build {
            config,
            tag,
            no_pull,
            base_image,
            api_version,
            install_command,
            build_command,
            docker_build_args,
        }) => {
            let result = commands::deployment::build(
                &config,
                &tag,
                !no_pull,
                base_image.as_deref(),
                api_version.as_deref(),
                install_command.as_deref(),
                build_command.as_deref(),
                &docker_build_args,
            );
            if let Err(err) = result {
                eprintln!("{}", print_error(&err));
                std::process::exit(1);
            }
            return Ok(());
        }
        Some(Command::Dockerfile {
            save_path,
            config,
            add_docker_compose,
            base_image,
            api_version,
        }) => {
            let result = commands::deployment::dockerfile(
                &save_path,
                &config,
                add_docker_compose,
                base_image.as_deref(),
                api_version.as_deref(),
            );
            if let Err(err) = result {
                eprintln!("{}", print_error(&err));
                std::process::exit(1);
            }
            return Ok(());
        }
        Some(Command::Dev {
            host,
            port,
            no_reload,
            config,
            n_jobs_per_worker,
            no_browser,
            debug_port,
            wait_for_client,
            studio_url,
            allow_blocking,
            tunnel,
            server_log_level,
        }) => {
            let result = commands::deployment::dev(
                &host,
                port,
                no_reload,
                &config,
                n_jobs_per_worker,
                no_browser,
                debug_port,
                wait_for_client,
                studio_url.as_deref(),
                allow_blocking,
                tunnel,
                &server_log_level,
            );
            if let Err(err) = result {
                eprintln!("{}", print_error(&err));
                std::process::exit(1);
            }
            return Ok(());
        }
        Some(Command::New { path, template }) => {
            let result = commands::deployment::new(path.as_deref(), template.as_deref());
            if let Err(err) = result {
                eprintln!("{}", print_error(&err));
                std::process::exit(1);
            }
            return Ok(());
        }
        Some(Command::Deploy {
            config,
            name,
            deployment_type,
            base_image,
            api_version,
            image,
        }) => {
            let result = commands::deployment::deploy(
                &config,
                &name,
                &deployment_type,
                base_image.as_deref(),
                api_version.as_deref(),
                image.as_deref(),
            )
            .await;
            if let Err(err) = result {
                eprintln!("{}", print_error(&err));
                std::process::exit(1);
            }
            return Ok(());
        }
        None => {}
    }

    // Pipe mode: stdin is not a TTY
    let is_pipe = !io::stdin().is_terminal();
    if is_pipe {
        if let Err(err) = run_pipe(cli.thread_id.as_deref(), cli.json).await {
            eprintln!("{}", err);
            std::process::exit(1);
        }
        return Ok(());
    }

    if let Err(err) = run(cli.resume, cli.thread_id.as_deref()).await {
        eprintln!("{}", print_error(&err.to_string()));
        std::process::exit(1);
    }

    Ok(())
}

/// Helper to load config, create client, and run a subcommand.
async fn run_sdk_command<F, Fut>(f: F) -> Result<()>
where
    F: FnOnce(Client) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    let cfg = config::load().context("failed to load config (run `ailsd` interactively first)")?;
    let client = Client::new(&cfg)?;
    if let Err(err) = f(client).await {
        eprintln!("{}", print_error(&err.to_string()));
        std::process::exit(1);
    }
    Ok(())
}

async fn run_pipe(thread_id: Option<&str>, json_output: bool) -> Result<()> {
    let cfg = config::load()
        .context("failed to load config (run `ailsd` interactively first to configure)")?;
    let client = Client::new(&cfg)?;

    // Read all of stdin
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let input = input.trim();
    if input.is_empty() {
        anyhow::bail!("no input provided on stdin");
    }

    // Create or reuse thread
    let tid = match thread_id {
        Some(id) => id.to_string(),
        None => client.create_thread().await?.thread_id,
    };

    // Run and wait for result
    let result = client.wait_run(&tid, &cfg.assistant_id, input).await?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        // Extract the last assistant message from the result
        let messages = crate::api::get_messages(&result);
        if let Some(last) = messages.iter().rev().find(|m| m.role == "assistant") {
            print!("{}", last.content);
        } else {
            // Fallback: print raw JSON
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
    }

    // Print thread ID to stderr so it can be reused
    eprintln!("thread_id={}", tid);

    Ok(())
}

async fn run(resume: bool, thread_id_arg: Option<&str>) -> Result<()> {
    if !config::exists() {
        // Save the built-in default so config file exists for future runs
        let default_cfg = config::ContextConfig::default();
        config::save_context_config(&default_cfg)?;
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

    let mut client = Client::new(&cfg)?;

    let (mut thread_id, mut history) = if let Some(tid) = thread_id_arg {
        // Direct thread ID provided — load its history
        let history = match client.get_thread_state(tid).await {
            Ok(state) => crate::api::get_messages(&state.values),
            Err(_) => Vec::new(),
        };
        (tid.to_string(), history)
    } else if resume {
        match handle_resume(&client).await? {
            Some((thread_id, history)) => (thread_id, history),
            None => return Ok(()),
        }
    } else {
        let thread = client.create_thread().await?;
        (thread.thread_id, Vec::new())
    };

    let ctx_cfg = config::load_context_config().unwrap_or_default();
    let context_names: Vec<String> = ctx_cfg.contexts.keys().cloned().collect();

    // Fetch available assistants
    let available_assistants: Vec<(String, String)> = match client.list_assistants().await {
        Ok(assistants) => assistants
            .iter()
            .filter_map(|a| {
                let id = a.get("assistant_id")?.as_str()?.to_string();
                let name = a
                    .get("name")
                    .and_then(|v| v.as_str())
                    .or_else(|| a.get("graph_id").and_then(|v| v.as_str()))
                    .unwrap_or("unnamed")
                    .to_string();
                Some((id, name))
            })
            .collect(),
        Err(_) => Vec::new(),
    };

    // Auto-resolve assistant ID: if configured ID isn't in the available list, use the first one
    let mut assistant_id = cfg.assistant_id.clone();
    if !available_assistants.is_empty()
        && !available_assistants.iter().any(|(id, _)| *id == assistant_id)
    {
        let (first_id, first_name) = &available_assistants[0];
        eprintln!(
            "Note: assistant '{}' not found, using '{}' ({})",
            assistant_id, first_name, first_id
        );
        assistant_id = first_id.clone();
    }

    let mut chat_config = ui::ChatConfig {
        version: version_string(),
        endpoint: cfg.endpoint.clone(),
        config_path,
        context_info,
        context_names,
        available_assistants,
        tenant_id: None,
        project_id: None,
    };

    loop {
        match ui::run_chat_loop(
            &client,
            &assistant_id,
            &thread_id,
            &history,
            &chat_config,
        )
        .await?
        {
            ui::ChatExit::Configure => {
                let context_name = config::current_context_name();
                let new_cfg = run_configure_inner(Some(&cfg)).context("configuration failed")?;
                config::save_context(&context_name, &new_cfg)?;
                cfg = new_cfg;
                client = Client::new(&cfg)?;
                history.clear();
                chat_config.endpoint = cfg.endpoint.clone();
                chat_config.context_info = format!("context: {}", config::current_context_name());
            }
            ui::ChatExit::SwitchContext(name) => {
                context::use_context(&name)?;
                cfg = config::load().context("failed to load config")?;
                client = Client::new(&cfg)?;
                history.clear();
                let thread = client.create_thread().await?;
                thread_id = thread.thread_id;
                chat_config.endpoint = cfg.endpoint.clone();
                chat_config.context_info = format!("context: {}", name);
            }
            ui::ChatExit::NewThread => {
                let thread = client.create_thread().await?;
                thread_id = thread.thread_id;
                history.clear();
            }
            ui::ChatExit::RunDoctor => {
                run_doctor().await?;
                println!("\nPress Enter to return to chat...");
                let _ = std::io::stdin().read_line(&mut String::new());
            }
            ui::ChatExit::RunBench => {
                commands::bench::run(&client, &cfg.assistant_id, 5, 20, vec!["hello".to_string()])
                    .await?;
                println!("\nPress Enter to return to chat...");
                let _ = std::io::stdin().read_line(&mut String::new());
            }
            ui::ChatExit::Quit => {
                println!("To resume this thread:\n  ailsd --thread-id {}", thread_id);
                if let Some(version) = update::auto_upgrade_if_available() {
                    println!("Auto-updated to {version}.");
                }
                return Ok(());
            }
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
            println!(
                "Found {} deployments but none have a URL configured.",
                deployments.len()
            );
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

async fn run_doctor() -> Result<()> {
    let cfg = config::load().context("failed to load config")?;
    let client = Client::new(&cfg)?;

    println!("Diagnosing deployment: {}\n", cfg.endpoint);

    // 1. Connectivity
    print!("  Connectivity ... ");
    let start = std::time::Instant::now();
    match client.get_info().await {
        Ok(info) => {
            let latency = start.elapsed().as_millis();
            println!("OK ({}ms)", latency);
            if let Some(version) = info.get("version").and_then(|v| v.as_str()) {
                println!("  API version  ... {}", version);
            }
            if let Some(lg_version) = info.get("langgraph_api_version").and_then(|v| v.as_str()) {
                println!("  LangGraph    ... {}", lg_version);
            }
        }
        Err(err) => {
            println!("FAILED");
            println!("    Error: {}", err);
            println!("\n  Check your endpoint URL and network connectivity.");
            return Ok(());
        }
    }

    // 2. Auth
    print!("  Auth         ... ");
    match client.list_assistants().await {
        Ok(assistants) => {
            println!("OK");
            println!("  Assistants   ... {} found", assistants.len());
            for a in &assistants {
                if let Some(id) = a.get("assistant_id").and_then(|v| v.as_str()) {
                    let name = a
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("(unnamed)");
                    println!("    - {} ({})", id, name);
                }
            }
        }
        Err(err) => {
            println!("FAILED");
            println!("    Error: {}", err);
            println!("\n  Check your API key or authentication headers.");
            return Ok(());
        }
    }

    // 3. Thread creation
    print!("  Threads      ... ");
    match client.create_thread().await {
        Ok(_) => println!("OK (can create threads)"),
        Err(err) => {
            println!("FAILED");
            println!("    Error: {}", err);
        }
    }

    println!("\nAll checks passed!");
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
