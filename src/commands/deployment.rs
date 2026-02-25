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

/// Check if uvx is available.
fn find_uvx() -> Option<String> {
    for candidate in &["uvx", "uv"] {
        if let Ok(path) = which::which(candidate) {
            return Some(path.to_string_lossy().to_string());
        }
    }
    None
}

/// Install uv via the official installer script.
fn install_uv() -> Result<(), String> {
    eprintln!("Installing uv...");
    let status = Command::new("sh")
        .args(["-c", "curl -LsSf https://astral.sh/uv/install.sh | sh"])
        .status()
        .map_err(|e| format!("Failed to run uv installer: {e}"))?;
    if !status.success() {
        return Err("uv installation failed".to_string());
    }
    // The installer puts uv in ~/.local/bin or ~/.cargo/bin — add to current PATH
    if let Ok(home) = std::env::var("HOME") {
        let current = std::env::var("PATH").unwrap_or_default();
        // SAFETY: single-threaded at this point (before tokio spawns), no other threads reading env
        unsafe {
            std::env::set_var(
                "PATH",
                format!("{home}/.local/bin:{home}/.cargo/bin:{current}"),
            );
        }
    }
    eprintln!("uv installed successfully.");
    Ok(())
}

/// Find the Python interpreter, preferring python3 over python.
fn find_python() -> Result<String, String> {
    for candidate in &["python3", "python"] {
        if let Ok(path) = which::which(candidate) {
            return Ok(path.to_string_lossy().to_string());
        }
    }
    Err(
        "Python not found. Install uv (https://docs.astral.sh/uv/) for automatic setup, \
         or install Python >= 3.11 with: pip install -U \"langgraph-cli[inmem]\""
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

    // Determine how to run: prefer uvx for zero-install, fall back to direct python
    enum RunMode {
        Uvx(String),    // uvx or uv binary path
        Python(String), // python3 or python binary
    }

    let run_mode = if let Some(uvx) = find_uvx() {
        eprintln!("Using {uvx} for automatic dependency management...");
        RunMode::Uvx(uvx)
    } else {
        // No uv/uvx found — check if python+langgraph works, otherwise offer to install uv
        let python_ok = find_python().ok().and_then(|python| {
            let check = Command::new(&python)
                .args(["-c", "from langgraph_api.cli import run_server"])
                .output();
            match check {
                Ok(output) if output.status.success() => Some(python),
                _ => None,
            }
        });

        if let Some(python) = python_ok {
            RunMode::Python(python)
        } else {
            // Offer to install uv
            eprintln!("uv is required for `ailsd dev` but was not found.");
            eprint!("Install uv now? [Y/n] ");
            let mut input = String::new();
            std::io::stdin()
                .read_line(&mut input)
                .map_err(|e| format!("Failed to read input: {e}"))?;
            if matches!(input.trim(), "" | "y" | "Y" | "yes" | "Yes") {
                install_uv()?;
                if let Some(uvx) = find_uvx() {
                    RunMode::Uvx(uvx)
                } else {
                    return Err(
                        "uv was installed but uvx was not found on PATH. \
                         Restart your shell and try again."
                            .to_string(),
                    );
                }
            } else {
                return Err(
                    "uv is required. Install manually: https://docs.astral.sh/uv/".to_string(),
                );
            }
        }
    };

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

    let work_dir = config_path.parent().unwrap_or_else(|| Path::new("."));

    // Spawn subprocess with config on stdin
    let mut child = match &run_mode {
        RunMode::Uvx(bin) => {
            let mut cmd = if bin.ends_with("/uv") || bin == "uv" {
                let mut c = Command::new(bin);
                c.arg("tool").arg("run");
                c
            } else {
                Command::new(bin)
            };
            cmd.args([
                "--from",
                "langgraph-cli[inmem]",
                "--python",
                "3.11",
                "python",
                "-c",
                python_code,
            ])
            .current_dir(work_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .map_err(|e| format!("Failed to start uvx: {e}"))?
        }
        RunMode::Python(python) => Command::new(python)
            .arg("-c")
            .arg(python_code)
            .current_dir(work_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .map_err(|e| format!("Failed to start Python: {e}"))?,
    };

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

/// Deploy to LangSmith cloud (internal_docker flow).
///
/// Builds a Docker image locally, pushes it to LangSmith's managed Artifact
/// Registry, and creates or updates a cloud deployment.
#[allow(clippy::too_many_arguments)]
pub async fn deploy(
    config: &str,
    name: &str,
    deployment_type: &str,
    base_image: Option<&str>,
    api_version: Option<&str>,
    image: Option<&str>,
) -> Result<(), String> {
    // 1. Load config
    let config_path = Path::new(config);
    let config_json = crate::deploy::config::validate_config_file(config_path)?;
    let _ = &config_json; // validate config exists and is valid

    // 2. Resolve API key
    let api_key = {
        let cfg = crate::config::load().ok();
        let from_cfg = cfg.as_ref().map(|c| c.api_key.clone()).unwrap_or_default();
        if from_cfg.is_empty() {
            std::env::var("LANGSMITH_API_KEY").map_err(|_| {
                "No API key found. Set LANGSMITH_API_KEY or configure with `ailsd context`"
                    .to_string()
            })?
        } else {
            from_cfg
        }
    };

    // 3. Create host client
    let host = crate::api::host::HostClient::new(&api_key)
        .map_err(|e| format!("Failed to create host client: {e}"))?;

    // 4. Find or create deployment
    let deployment_id = match host
        .find_deployment_by_name(name)
        .await
        .map_err(|e| format!("Failed to list deployments: {e}"))?
    {
        Some((id, _)) => {
            eprintln!("Deployment \"{name}\" already exists, updating...");
            id
        }
        None => {
            eprintln!("Creating deployment \"{name}\"...");
            let d = host
                .create_deployment(name, deployment_type)
                .await
                .map_err(|e| format!("Failed to create deployment: {e}"))?;
            d.get("id")
                .and_then(|i| i.as_str())
                .ok_or_else(|| "No id in create response".to_string())?
                .to_string()
        }
    };

    // 5. Build image (unless --image provided)
    let local_tag = if let Some(img) = image {
        img.to_string()
    } else {
        let tag = format!("ailsd-deploy-{name}:latest");
        eprintln!("Building Docker image ({tag})...");
        build(config, &tag, true, base_image, api_version, None, None, &[])?;
        tag
    };

    // 6. Get push token
    eprintln!("Getting push credentials...");
    let token_resp = host
        .get_push_token(&deployment_id)
        .await
        .map_err(|e| format!("Failed to get push token: {e}"))?;

    // 7. Docker login
    let registry_host = token_resp
        .registry_url
        .split('/')
        .next()
        .unwrap_or(&token_resp.registry_url);

    let login_status = Command::new("docker")
        .args([
            "login",
            "-u",
            "oauth2accesstoken",
            "--password-stdin",
            registry_host,
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                stdin.write_all(token_resp.token.as_bytes())?;
            }
            drop(child.stdin.take());
            child.wait()
        })
        .map_err(|e| format!("docker login failed: {e}"))?;

    if !login_status.success() {
        return Err("docker login failed".to_string());
    }

    // 8. Tag and push
    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
    let remote_tag = format!("{}/{}:{timestamp}", token_resp.registry_url, name);

    eprintln!("Pushing image to {remote_tag}...");
    crate::deploy::exec::run_command("docker", &["tag", &local_tag, &remote_tag], None, false)?;
    crate::deploy::exec::run_command("docker", &["push", &remote_tag], None, true)?;

    // 9. Trigger revision
    eprintln!("Triggering deployment revision...");
    host.patch_deployment(&deployment_id, &remote_tag)
        .await
        .map_err(|e| format!("Failed to update deployment: {e}"))?;

    eprintln!("Deployed successfully!");

    Ok(())
}

/// Create a new LangGraph project from a template.
pub fn new(_path: Option<&str>, _template: Option<&str>) -> Result<(), String> {
    // Placeholder for template creation
    // This will be implemented when the templates module is available
    Err("Template creation not yet implemented. This will be available when the deploy::templates module is complete.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_uvx_returns_absolute_path() {
        // If uvx/uv is available, the result must be an absolute path, not a bare name.
        // This is the regression that caused "No such file or directory" when Command::new()
        // received just "uvx" without the full path.
        if let Some(path) = find_uvx() {
            assert!(
                path.starts_with('/'),
                "find_uvx() should return an absolute path, got: {path}"
            );
        }
    }

    #[test]
    fn find_python_returns_absolute_path() {
        if let Ok(path) = find_python() {
            assert!(
                path.starts_with('/'),
                "find_python() should return an absolute path, got: {path}"
            );
        }
    }

    #[test]
    fn find_uvx_prefers_uvx_over_uv() {
        // If both exist, uvx should be preferred (it's checked first)
        if let Some(path) = find_uvx() {
            let has_uvx = which::which("uvx").is_ok();
            if has_uvx {
                assert!(
                    path.ends_with("/uvx"),
                    "should prefer uvx when available, got: {path}"
                );
            }
        }
    }

    #[test]
    fn find_python_prefers_python3() {
        if let Ok(path) = find_python() {
            let has_python3 = which::which("python3").is_ok();
            if has_python3 {
                assert!(
                    path.ends_with("/python3"),
                    "should prefer python3 when available, got: {path}"
                );
            }
        }
    }
}
