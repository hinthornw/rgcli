use anyhow::{Context, Result, bail};
use comfy_table::{Cell, Color, Table};
use serde_json::{Value, json};

use crate::api::Client;
use crate::api::types::{SandboxSessionAcquireResponse, SandboxSessionMode};
use lsandbox::{RunOpts, SandboxClient};

fn get_api_key() -> Result<String> {
    let cfg = crate::config::load().context("failed to load config")?;
    let key = if cfg.api_key.is_empty() {
        std::env::var("LANGSMITH_API_KEY").unwrap_or_default()
    } else {
        cfg.api_key.clone()
    };
    if key.is_empty() {
        bail!("No API key configured. Run `ailsd` interactively or set LANGSMITH_API_KEY.");
    }
    Ok(key)
}

fn client() -> Result<SandboxClient> {
    let key = get_api_key()?;
    SandboxClient::new(&key).map_err(|e| anyhow::anyhow!("{e}"))
}

fn sdk_client() -> Result<Client> {
    let cfg =
        crate::config::load().context("failed to load config (run `ailsd` interactively first)")?;
    Client::new(&cfg).context("failed to create API client")
}

#[derive(Default, Clone)]
struct SessionView {
    thread_id: Option<String>,
    session_id: Option<String>,
    sandbox_id: Option<String>,
    sandbox_provider: Option<String>,
    http_base_url: Option<String>,
    ws_base_url: Option<String>,
    token_expires_at: Option<String>,
    token_present: bool,
}

fn get_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut cur = value;
    for key in path {
        cur = cur.get(*key)?;
    }
    Some(cur)
}

fn pick_string(value: &Value, paths: &[&[&str]]) -> Option<String> {
    paths.iter().find_map(|path| {
        get_path(value, path)
            .and_then(Value::as_str)
            .map(str::to_string)
    })
}

fn has_value(value: &Value, paths: &[&[&str]]) -> bool {
    paths
        .iter()
        .any(|path| get_path(value, path).is_some_and(|v| !v.is_null()))
}

fn session_view_from_value(value: &Value) -> SessionView {
    SessionView {
        thread_id: pick_string(value, &[&["thread_id"], &["scope", "id"]]),
        session_id: pick_string(value, &[&["session_id"], &["binding", "id"], &["id"]]),
        sandbox_id: pick_string(
            value,
            &[
                &["sandbox", "id"],
                &["binding", "sandbox_id"],
                &["sandbox_id"],
            ],
        ),
        sandbox_provider: pick_string(
            value,
            &[
                &["sandbox", "provider"],
                &["binding", "provider"],
                &["provider"],
            ],
        ),
        http_base_url: pick_string(
            value,
            &[
                &["sandbox", "http_base_url"],
                &["dataplane", "http_base_url"],
                &["http_base_url"],
                &["http_url"],
            ],
        ),
        ws_base_url: pick_string(
            value,
            &[
                &["sandbox", "ws_base_url"],
                &["dataplane", "ws_base_url"],
                &["ws_base_url"],
                &["ws_url"],
            ],
        ),
        token_expires_at: pick_string(value, &[&["expires_at"], &["credentials", "expires_at"]]),
        token_present: has_value(
            value,
            &[
                &["token"],
                &["access_token"],
                &["credentials", "access_token"],
            ],
        ),
    }
}

fn session_view_from_acquire(resp: &SandboxSessionAcquireResponse) -> SessionView {
    SessionView {
        thread_id: Some(resp.thread_id.clone()),
        session_id: Some(resp.session_id.clone()),
        sandbox_id: Some(resp.sandbox.id.clone()),
        sandbox_provider: Some(resp.sandbox.provider.clone()),
        http_base_url: Some(resp.sandbox.http_base_url.clone()),
        ws_base_url: Some(resp.sandbox.ws_base_url.clone()),
        token_expires_at: Some(resp.expires_at.clone()),
        token_present: !resp.token.is_empty(),
    }
}

fn merge_missing(dst: &mut SessionView, src: SessionView) {
    if dst.thread_id.is_none() {
        dst.thread_id = src.thread_id;
    }
    if dst.session_id.is_none() {
        dst.session_id = src.session_id;
    }
    if dst.sandbox_id.is_none() {
        dst.sandbox_id = src.sandbox_id;
    }
    if dst.sandbox_provider.is_none() {
        dst.sandbox_provider = src.sandbox_provider;
    }
    if dst.http_base_url.is_none() {
        dst.http_base_url = src.http_base_url;
    }
    if dst.ws_base_url.is_none() {
        dst.ws_base_url = src.ws_base_url;
    }
    if dst.token_expires_at.is_none() {
        dst.token_expires_at = src.token_expires_at;
    }
    if !dst.token_present {
        dst.token_present = src.token_present;
    }
}

fn print_session_output(action: &str, view: &SessionView, released: Option<bool>) -> Result<()> {
    let mut out = json!({
        "action": action,
        "thread_id": view.thread_id.clone(),
        "session_id": view.session_id.clone(),
        "sandbox": {
            "id": view.sandbox_id.clone(),
            "provider": view.sandbox_provider.clone(),
            "http_base_url": view.http_base_url.clone(),
            "ws_base_url": view.ws_base_url.clone(),
        },
        "token": {
            "present": view.token_present,
            "expires_at": view.token_expires_at.clone(),
        }
    });
    if let Some(released) = released {
        out["released"] = Value::Bool(released);
    }
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

async fn get_session_by_id(client: &Client, session_id: &str) -> Result<Value> {
    let response = client.get_sandbox_session(session_id).await?;
    Ok(serde_json::to_value(response)?)
}

async fn resolve_session_for_relay(
    client: &Client,
    session_id: &str,
) -> Result<(SandboxSessionAcquireResponse, String)> {
    let refreshed = client.refresh_sandbox_session(session_id).await?;
    let session = client.get_sandbox_session(session_id).await?;
    let token = if refreshed.token.is_empty() {
        session.token.clone()
    } else {
        refreshed.token
    };
    if token.is_empty() {
        bail!("session refresh did not return a relay token");
    }
    Ok((session, token))
}

pub async fn session_get(thread_id: &str) -> Result<()> {
    let client = sdk_client()?;
    let resp = client
        .acquire_sandbox_session(thread_id, SandboxSessionMode::Get)
        .await?;
    let view = session_view_from_acquire(&resp);
    print_session_output("session-get", &view, None)
}

pub async fn session_ensure(thread_id: &str) -> Result<()> {
    let client = sdk_client()?;
    let resp = client
        .acquire_sandbox_session(thread_id, SandboxSessionMode::Ensure)
        .await?;
    let view = session_view_from_acquire(&resp);
    print_session_output("session-ensure", &view, None)
}

pub async fn session_refresh(session_id: &str) -> Result<()> {
    let client = sdk_client()?;
    let resp = client.refresh_sandbox_session(session_id).await?;

    let mut view = SessionView {
        session_id: Some(session_id.to_string()),
        token_expires_at: Some(resp.expires_at),
        token_present: !resp.token.is_empty(),
        ..Default::default()
    };
    if view.session_id.is_none() {
        view.session_id = Some(session_id.to_string());
    }

    if view.sandbox_id.is_none()
        || view.sandbox_provider.is_none()
        || view.http_base_url.is_none()
        || view.ws_base_url.is_none()
        || view.thread_id.is_none()
    {
        if let Ok(snapshot) = get_session_by_id(&client, session_id).await {
            merge_missing(&mut view, session_view_from_value(&snapshot));
        }
    }

    print_session_output("session-refresh", &view, None)
}

pub async fn session_release(session_id: &str) -> Result<()> {
    let client = sdk_client()?;
    let mut view = get_session_by_id(&client, session_id)
        .await
        .map(|v| session_view_from_value(&v))
        .unwrap_or_default();

    client.release_sandbox_session(session_id).await?;

    if view.session_id.is_none() {
        view.session_id = Some(session_id.to_string());
    }
    print_session_output("session-release", &view, Some(true))
}

pub async fn session_exec(session_id: &str, command: &str, timeout: u64) -> Result<()> {
    let client = sdk_client()?;
    let (session, token) = resolve_session_for_relay(&client, session_id).await?;
    let result = client
        .relay_execute_sandbox_session(&session.sandbox.http_base_url, &token, command, timeout)
        .await?;

    let stdout = result
        .get("stdout")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let stderr = result
        .get("stderr")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let exit_code = result.get("exit_code").and_then(Value::as_i64).unwrap_or(0);

    if !stdout.is_empty() {
        print!("{stdout}");
    }
    if !stderr.is_empty() {
        eprint!("{stderr}");
    }
    if exit_code != 0 {
        std::process::exit(exit_code as i32);
    }
    Ok(())
}

pub async fn list() -> Result<()> {
    let c = client()?;
    let sandboxes = c
        .list_sandboxes()
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    if sandboxes.is_empty() {
        println!("No sandboxes found.");
        return Ok(());
    }

    let mut table = Table::new();
    table.set_header(vec!["Name", "Template", "Dataplane URL", "Created"]);

    for sb in &sandboxes {
        table.add_row(vec![
            Cell::new(&sb.name).fg(Color::Cyan),
            Cell::new(&sb.template_name),
            Cell::new(sb.dataplane_url.as_deref().unwrap_or("-")),
            Cell::new(sb.created_at.as_deref().unwrap_or("-")),
        ]);
    }

    println!("{table}");
    Ok(())
}

pub async fn templates() -> Result<()> {
    let c = client()?;
    let templates = c
        .list_templates()
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    if templates.is_empty() {
        println!("No templates found.");
        return Ok(());
    }

    let mut table = Table::new();
    table.set_header(vec!["Name", "Image", "CPU", "Memory"]);

    for t in &templates {
        table.add_row(vec![
            Cell::new(&t.name).fg(Color::Cyan),
            Cell::new(&t.image),
            Cell::new(&t.resources.cpu),
            Cell::new(&t.resources.memory),
        ]);
    }

    println!("{table}");
    Ok(())
}

pub async fn create(template: &str, name: Option<&str>) -> Result<()> {
    let c = client()?;
    let sandbox = c
        .create_sandbox(template, name)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("Created sandbox: {}", sandbox.name());
    if let Some(url) = sandbox.dataplane_url() {
        println!("Dataplane URL: {url}");
    }
    Ok(())
}

pub async fn exec(name: &str, command: &str, timeout: u64) -> Result<()> {
    let c = client()?;
    let sandbox = c
        .get_sandbox(name)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let result = sandbox
        .run_with(&RunOpts::new(command).timeout(timeout))
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    if !result.stdout.is_empty() {
        print!("{}", result.stdout);
    }
    if !result.stderr.is_empty() {
        eprint!("{}", result.stderr);
    }
    if !result.success() {
        std::process::exit(result.exit_code);
    }
    Ok(())
}

pub async fn connect(name: &str) -> Result<()> {
    let c = client()?;
    let sandbox = c
        .get_sandbox(name)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    connect_streaming_shell(sandbox, &format!("sandbox '{}'", name)).await
}

pub async fn session_connect(session_id: &str) -> Result<()> {
    let sdk = sdk_client()?;
    let (session, token) = resolve_session_for_relay(&sdk, session_id).await?;

    let c = client()?;
    let sandbox = c.sandbox_from_dataplane(
        &format!("session-{session_id}"),
        &session.sandbox.http_base_url,
        &token,
    );

    connect_streaming_shell(
        sandbox,
        &format!(
            "session '{}' (sandbox '{}')",
            session_id, session.sandbox.id
        ),
    )
    .await
}

pub async fn sync(name: &str, local_path: &str, remote_path: &str) -> Result<()> {
    let c = client()?;
    let sandbox = c
        .get_sandbox(name)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    println!("Syncing {} -> sandbox:{}...", local_path, remote_path);

    // Create tar archive of local directory
    let tar_data = create_tar_gz(local_path)?;
    let tar_size = tar_data.len();

    // Upload tar to sandbox
    sandbox
        .write("/tmp/_ailsd_sync.tar.gz", &tar_data)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    // Extract on sandbox
    let cmd = format!(
        "mkdir -p {remote_path} && tar xzf /tmp/_ailsd_sync.tar.gz -C {remote_path} && rm /tmp/_ailsd_sync.tar.gz"
    );
    let result = sandbox
        .run(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    if result.success() {
        println!(
            "Synced {} ({} bytes compressed) to {}",
            local_path, tar_size, remote_path
        );
    } else {
        bail!("Sync failed: {}", result.stderr);
    }
    Ok(())
}

pub async fn delete(name: &str) -> Result<()> {
    let c = client()?;
    c.delete_sandbox(name)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("Deleted sandbox: {name}");
    Ok(())
}

/// Create a tar.gz archive of a local directory, respecting .gitignore.
fn create_tar_gz(path: &str) -> Result<Vec<u8>> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::path::Path;

    let src = Path::new(path)
        .canonicalize()
        .with_context(|| format!("path not found: {path}"))?;

    let buf = Vec::new();
    let enc = GzEncoder::new(buf, Compression::fast());
    let mut tar = tar::Builder::new(enc);

    if src.is_file() {
        let name = src.file_name().unwrap().to_str().unwrap_or("file");
        tar.append_path_with_name(&src, name)?;
    } else {
        // Walk directory, skip .git and common ignores
        walk_dir(&src, &src, &mut tar)?;
    }

    let enc = tar.into_inner()?;
    Ok(enc.finish()?)
}

fn walk_dir(
    root: &std::path::Path,
    dir: &std::path::Path,
    tar: &mut tar::Builder<flate2::write::GzEncoder<Vec<u8>>>,
) -> Result<()> {
    use std::fs;

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();

        // Skip common unneeded directories
        if name == ".git"
            || name.starts_with(".git/")
            || name == "target"
            || name.starts_with("target/")
            || name == "node_modules"
            || name.starts_with("node_modules/")
            || name == "__pycache__"
            || name.starts_with("__pycache__/")
        {
            continue;
        }

        if path.is_dir() {
            walk_dir(root, &path, tar)?;
        } else {
            tar.append_path_with_name(&path, &name)?;
        }
    }
    Ok(())
}

async fn connect_streaming_shell(sandbox: lsandbox::Sandbox, label: &str) -> Result<()> {
    println!("Connecting to {label}...");
    let mut handle = sandbox
        .run_streaming("/bin/bash")
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    // Interactive terminal: read stdin in a separate task, forward to sandbox
    let input = handle.input_sender();
    let stdin_task = tokio::spawn(async move {
        use tokio::io::AsyncBufReadExt;
        let stdin = tokio::io::stdin();
        let reader = tokio::io::BufReader::new(stdin);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if input.send(&format!("{line}\n")).await.is_err() {
                break;
            }
        }
    });

    // Print output chunks as they arrive
    while let Some(chunk) = handle.recv().await {
        print!("{}", chunk.data);
    }

    stdin_task.abort();

    match handle.wait().await {
        Ok(result) => {
            if !result.success() {
                std::process::exit(result.exit_code);
            }
        }
        Err(e) => {
            eprintln!("Connection closed: {e}");
        }
    }
    Ok(())
}
