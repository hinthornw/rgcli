use std::path::Path;
use std::process::Command;

/// Launch LangGraph API server with Docker Compose.
#[allow(clippy::too_many_arguments)]
pub fn up(
    config: &str,
    port: u16,
    docker_compose: Option<&str>,
    verbose: bool,
    watch: bool,
    recreate: bool,
    pull: bool,
    wait: bool,
    debugger_port: Option<u16>,
    debugger_base_url: Option<&str>,
    postgres_uri: Option<&str>,
    api_version: Option<&str>,
    image: Option<&str>,
    base_image: Option<&str>,
) -> Result<(), String> {
    eprintln!("Starting LangGraph API server...");
    eprintln!(
        "For local dev, requires env var LANGSMITH_API_KEY with access to LangSmith Deployment.\n\
         For production use, requires a license key in env var LANGGRAPH_CLOUD_LICENSE_KEY."
    );

    // Validate config
    let config_path = Path::new(config);
    let mut config_json = crate::deploy::config::validate_config_file(config_path)?;

    // Check docker capabilities
    let capabilities = crate::deploy::capabilities::check_capabilities()?;

    // Pull latest images if requested
    if pull {
        let tag = crate::deploy::docker_tag::docker_tag(&config_json, base_image, api_version);
        eprintln!("Pulling images...");
        crate::deploy::exec::run_command("docker", &["pull", &tag], None, verbose)?;
    }

    // Generate compose YAML
    let debugger_base_url_resolved = debugger_base_url
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("http://127.0.0.1:{port}"));

    let mut compose_stdin = crate::deploy::compose::compose(
        &capabilities,
        port,
        debugger_port,
        Some(&debugger_base_url_resolved),
        postgres_uri,
        image,
        base_image,
        api_version,
    );

    // Append config-to-compose output (build instructions, env, watch sections)
    let base_img = base_image
        .map(|s| s.to_string())
        .unwrap_or_else(|| crate::deploy::docker_tag::default_base_image(&config_json));
    let compose_config = crate::deploy::docker::config_to_compose(
        config_path,
        &mut config_json,
        Some(&base_img),
        api_version,
        image,
        watch,
    )?;
    compose_stdin.push_str(&compose_config);

    // Build docker compose args
    let mut args: Vec<String> = Vec::new();
    args.push("--project-directory".to_string());
    args.push(
        config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_string_lossy()
            .to_string(),
    );

    if let Some(dc) = docker_compose {
        args.push("-f".to_string());
        args.push(dc.to_string());
    }

    // Read compose from stdin
    args.push("-f".to_string());
    args.push("-".to_string());

    // Add up + options
    args.push("up".to_string());
    args.push("--remove-orphans".to_string());

    if recreate {
        args.push("--force-recreate".to_string());
        args.push("--renew-anon-volumes".to_string());
        // Try to remove the volume, ignore errors
        let _ = crate::deploy::exec::run_command(
            "docker",
            &["volume", "rm", "langgraph-data"],
            None,
            false,
        );
    }

    if watch {
        args.push("--watch".to_string());
    }

    if wait {
        args.push("--wait".to_string());
    } else {
        args.push("--abort-on-container-exit".to_string());
    }

    eprintln!("Building...");

    // Determine compose command
    let compose_cmd = match capabilities.compose_type {
        crate::deploy::capabilities::ComposeType::Plugin => vec!["docker", "compose"],
        crate::deploy::capabilities::ComposeType::Standalone => vec!["docker-compose"],
    };

    // Build final command args
    let mut cmd_args: Vec<&str> = Vec::new();
    if compose_cmd.len() > 1 {
        // "docker compose ..."
        cmd_args.extend_from_slice(&compose_cmd[1..]);
    }
    for a in &args {
        cmd_args.push(a.as_str());
    }

    // Run docker compose with streaming output, intercepting stdout
    // to detect startup and show Ready! URLs
    let mut ready_printed = false;
    let debugger_base_url_query = debugger_base_url
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("http://127.0.0.1:{port}"));

    crate::deploy::exec::run_command_streaming_with_callback(
        compose_cmd[0],
        &cmd_args,
        Some(&compose_stdin),
        verbose,
        |line| {
            if !ready_printed && line.contains("Application startup complete") {
                ready_printed = true;

                let debugger_origin = if let Some(dp) = debugger_port {
                    format!("http://localhost:{dp}")
                } else {
                    "https://smith.langchain.com".to_string()
                };

                println!("\nReady!");
                println!("  - API: http://localhost:{port}");
                println!("  - Docs: http://localhost:{port}/docs");
                println!(
                    "  - LangGraph Studio: {debugger_origin}/studio/?baseUrl={debugger_base_url_query}"
                );
            }
        },
    )?;

    Ok(())
}

/// Build a LangGraph API server Docker image.
#[allow(clippy::too_many_arguments)]
pub fn build(
    config: &str,
    tag: &str,
    pull: bool,
    base_image: Option<&str>,
    api_version: Option<&str>,
    install_command: Option<&str>,
    build_command: Option<&str>,
    docker_build_args: &[String],
) -> Result<(), String> {
    // Check docker is available
    if which::which("docker").is_err() {
        return Err("Docker not installed".to_string());
    }

    // Validate config
    let config_path = Path::new(config);
    let mut config_json = crate::deploy::config::validate_config_file(config_path)?;

    // Pull latest images if requested
    if pull {
        let image_tag =
            crate::deploy::docker_tag::docker_tag(&config_json, base_image, api_version);
        eprintln!("Pulling images...");
        crate::deploy::exec::run_command("docker", &["pull", &image_tag], None, true)?;
    }

    eprintln!("Building...");

    // Determine build context
    let is_js_project = config_json.node_version.is_some() && config_json.python_version.is_none();

    // For JS projects with install/build commands, use CWD; otherwise use config parent
    let build_context = if is_js_project && (build_command.is_some() || install_command.is_some()) {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".to_string())
    } else {
        config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_string_lossy()
            .to_string()
    };

    // Generate Dockerfile
    let (dockerfile_content, additional_contexts) = crate::deploy::docker::config_to_docker(
        config_path,
        &mut config_json,
        base_image,
        api_version,
        install_command,
        build_command,
        Some(&build_context),
        false, // no variable escaping for docker build
    )?;

    // Build docker build args
    let mut args: Vec<String> = vec![
        "build".to_string(),
        "-f".to_string(),
        "-".to_string(), // Dockerfile from stdin
        "-t".to_string(),
        tag.to_string(),
    ];

    // Add additional build contexts
    for (name, path) in &additional_contexts {
        args.push("--build-context".to_string());
        args.push(format!("{name}={path}"));
    }

    // Add passthrough docker build args
    for extra in docker_build_args {
        args.push(extra.clone());
    }

    // Add build context as last arg
    args.push(build_context);

    // Run docker build with streaming output
    let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    crate::deploy::exec::run_command_streaming(
        "docker",
        &args_refs,
        Some(&dockerfile_content),
        true,
    )?;

    eprintln!("Successfully built image: {tag}");

    Ok(())
}

/// Find the Python interpreter, preferring python3 over python.
fn find_python() -> Result<String, String> {
    for candidate in &["python3", "python"] {
        if which::which(candidate).is_ok() {
            return Ok(candidate.to_string());
        }
    }
    Err(
        "Python not found. The `ailsd deploy dev` command requires Python >= 3.11 with \
         langgraph-cli[inmem] installed.\n\
         Install with: pip install -U \"langgraph-cli[inmem]\""
            .to_string(),
    )
}

/// Run the LangGraph API server in development mode (in-memory, via Python subprocess).
#[allow(clippy::too_many_arguments)]
pub fn dev(
    host: &str,
    port: u16,
    no_reload: bool,
    config: &str,
    n_jobs_per_worker: Option<u32>,
    no_browser: bool,
    debug_port: Option<u16>,
    wait_for_client: bool,
    studio_url: Option<&str>,
    allow_blocking: bool,
    tunnel: bool,
    server_log_level: &str,
) -> Result<(), String> {
    // Validate config
    let config_path = Path::new(config);
    let config_json = crate::deploy::config::validate_config_file(config_path)?;

    // Check for node_version -- in-mem server doesn't support JS graphs
    if config_json.node_version.is_some() {
        return Err(
            "In-mem server for JS graphs is not supported in this version. \
             Please use `npx @langchain/langgraph-cli` instead."
                .to_string(),
        );
    }

    let python = find_python()?;

    // Pre-check that langgraph_api is importable
    let check = Command::new(&python)
        .args(["-c", "from langgraph_api.cli import run_server"])
        .output();

    match check {
        Ok(output) if !output.status.success() => {
            return Err("Required package 'langgraph-api' is not installed.\n\
                 Please install it with:\n\n\
                     pip install -U \"langgraph-cli[inmem]\"\n\n\
                 Note: The in-mem server requires Python 3.11 or higher."
                .to_string());
        }
        Err(_) => {
            return Err(format!(
                "Failed to run {python}. The `ailsd deploy dev` command requires Python >= 3.11 with \
                 langgraph-cli[inmem] installed.\n\
                 Install with: pip install -U \"langgraph-cli[inmem]\""
            ));
        }
        _ => {}
    }

    // Build a JSON config object to pass via stdin.
    let dev_config = serde_json::json!({
        "host": host,
        "port": port,
        "reload": !no_reload,
        "open_browser": !no_browser,
        "wait_for_client": wait_for_client,
        "allow_blocking": allow_blocking,
        "tunnel": tunnel,
        "server_log_level": server_log_level,
        "dependencies": config_json.dependencies,
        "graphs": config_json.graphs,
        "n_jobs_per_worker": n_jobs_per_worker,
        "debug_port": debug_port,
        "studio_url": studio_url,
        "env": config_json.env,
        "store": config_json.store,
        "auth": config_json.auth,
        "http": config_json.http,
        "ui": config_json.ui,
        "ui_config": config_json.ui_config,
        "webhooks": config_json.webhooks,
    });

    // Python bootstrap: reads JSON from stdin, calls run_server
    let python_code = r#"
import sys, os, json, pathlib
config = json.loads(sys.stdin.read())
cwd = os.getcwd()
sys.path.append(cwd)
for dep in config.get('dependencies', []):
    dep_path = pathlib.Path(cwd) / dep
    if dep_path.is_dir() and dep_path.exists():
        sys.path.append(str(dep_path))
from langgraph_api.cli import run_server
run_server(
    config['host'],
    config['port'],
    config['reload'],
    config['graphs'],
    n_jobs_per_worker=config.get('n_jobs_per_worker'),
    open_browser=config['open_browser'],
    debug_port=config.get('debug_port'),
    env=config.get('env'),
    store=config.get('store'),
    wait_for_client=config['wait_for_client'],
    auth=config.get('auth'),
    http=config.get('http'),
    ui=config.get('ui'),
    ui_config=config.get('ui_config'),
    webhooks=config.get('webhooks'),
    studio_url=config.get('studio_url'),
    allow_blocking=config['allow_blocking'],
    tunnel=config['tunnel'],
    server_level=config['server_log_level'],
)
"#;

    eprintln!("Starting LangGraph API server in development mode...");

    // Spawn Python subprocess with config on stdin
    let mut child = Command::new(&python)
        .arg("-c")
        .arg(python_code)
        .current_dir(config_path.parent().unwrap_or_else(|| Path::new(".")))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .map_err(|e| format!("Failed to start Python: {e}"))?;

    // Write JSON config to stdin
    if let Some(ref mut stdin) = child.stdin {
        use std::io::Write;
        let json_bytes = dev_config.to_string();
        stdin
            .write_all(json_bytes.as_bytes())
            .map_err(|e| format!("Failed to write config to Python stdin: {e}"))?;
    }
    // Drop stdin to signal EOF
    drop(child.stdin.take());

    let status = child
        .wait()
        .map_err(|e| format!("Failed to wait for Python: {e}"))?;

    if !status.success() {
        let code = status.code().unwrap_or(1);
        if code == 130 {
            // User interrupted with Ctrl-C
            return Ok(());
        }
        return Err(format!("Development server exited with code {code}"));
    }

    Ok(())
}

/// Docker ignore file content.
fn get_docker_ignore_content() -> &'static str {
    "\
# Ignore node_modules and other dependency directories
node_modules
bower_components
vendor

# Ignore logs and temporary files
*.log
*.tmp
*.swp

# Ignore .env files and other environment files
.env
.env.*
*.local

# Ignore git-related files
.git
.gitignore

# Ignore Docker-related files and configs
.dockerignore
docker-compose.yml

# Ignore build and cache directories
dist
build
.cache
__pycache__

# Ignore IDE and editor configurations
.vscode
.idea
*.sublime-project
*.sublime-workspace
.DS_Store  # macOS-specific

# Ignore test and coverage files
coverage
*.coverage
*.test.js
*.spec.js
tests
"
}

/// Generate a Dockerfile for the LangGraph API server.
pub fn dockerfile(
    save_path: &str,
    config: &str,
    add_docker_compose: bool,
    base_image: Option<&str>,
    api_version: Option<&str>,
) -> Result<(), String> {
    let save_path = Path::new(save_path);
    let abs_save_path = if save_path.is_absolute() {
        save_path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(save_path)
    };

    eprintln!("Validating configuration at path: {config}");
    let config_path = Path::new(config);
    let mut config_json = crate::deploy::config::validate_config_file(config_path)?;
    eprintln!("Configuration validated!");

    eprintln!("Generating Dockerfile at {}", abs_save_path.display());

    let (dockerfile_content, additional_contexts) = crate::deploy::docker::config_to_docker(
        config_path,
        &mut config_json,
        base_image,
        api_version,
        None,
        None,
        None,
        false,
    )?;

    std::fs::write(&abs_save_path, &dockerfile_content)
        .map_err(|e| format!("Failed to write Dockerfile: {e}"))?;
    eprintln!("Created: Dockerfile");

    if !additional_contexts.is_empty() {
        let ctx_str: Vec<String> = additional_contexts
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        eprintln!(
            "Run docker build with these additional build contexts `--build-context {}`",
            ctx_str.join(",")
        );
    }

    if add_docker_compose {
        let parent = abs_save_path.parent().unwrap_or(Path::new("."));

        // Write .dockerignore
        let dockerignore_path = parent.join(".dockerignore");
        std::fs::write(&dockerignore_path, get_docker_ignore_content())
            .map_err(|e| format!("Failed to write .dockerignore: {e}"))?;
        eprintln!("Created: .dockerignore");

        // Generate docker-compose.yml
        let capabilities = crate::deploy::capabilities::check_capabilities()?;
        let compose_dict = crate::deploy::compose::compose_as_dict(
            &capabilities,
            8123,
            None,
            None,
            None,
            None,
            base_image,
            api_version,
        );

        let compose_yaml = crate::deploy::compose::dict_to_yaml(&compose_dict, 0);
        let compose_path = parent.join("docker-compose.yml");
        std::fs::write(&compose_path, &compose_yaml)
            .map_err(|e| format!("Failed to write docker-compose.yml: {e}"))?;
        eprintln!("Created: docker-compose.yml");

        // Create .env file if it doesn't exist
        let env_path = parent.join(".env");
        if !env_path.exists() {
            let env_content = "\
# Uncomment the following line to add your LangSmith API key
# LANGSMITH_API_KEY=your-api-key
# Or if you have a LangSmith Deployment license key, then uncomment the following line:
# LANGGRAPH_CLOUD_LICENSE_KEY=your-license-key
# Add any other environment variables go below...
";
            std::fs::write(&env_path, env_content)
                .map_err(|e| format!("Failed to write .env: {e}"))?;
            eprintln!("Created: .env");
        } else {
            eprintln!("Skipped: .env. It already exists!");
        }
    }

    eprintln!(
        "Files generated successfully at path {}!",
        abs_save_path.parent().unwrap_or(Path::new(".")).display()
    );

    Ok(())
}

/// Create a new LangGraph project from a template.
pub fn new(_path: Option<&str>, _template: Option<&str>) -> Result<(), String> {
    // Placeholder for template creation
    // This will be implemented when the templates module is available
    Err("Template creation not yet implemented. This will be available when the deploy::templates module is complete.".to_string())
}
