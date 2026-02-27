use anyhow::{Context, Result, bail};
use comfy_table::{Cell, Color, Table};

use lsandbox::{SandboxClient, RunOpts};

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

pub async fn list() -> Result<()> {
    let c = client()?;
    let sandboxes = c.list_sandboxes().await.map_err(|e| anyhow::anyhow!("{e}"))?;

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
    let templates = c.list_templates().await.map_err(|e| anyhow::anyhow!("{e}"))?;

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
    let sandbox = c.get_sandbox(name).await.map_err(|e| anyhow::anyhow!("{e}"))?;
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
    let sandbox = c.get_sandbox(name).await.map_err(|e| anyhow::anyhow!("{e}"))?;

    println!("Connecting to sandbox '{}'...", sandbox.name());
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

pub async fn sync(
    name: &str,
    local_path: &str,
    remote_path: &str,
) -> Result<()> {
    let c = client()?;
    let sandbox = c.get_sandbox(name).await.map_err(|e| anyhow::anyhow!("{e}"))?;

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
    let result = sandbox.run(&cmd).await.map_err(|e| anyhow::anyhow!("{e}"))?;

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
    c.delete_sandbox(name).await.map_err(|e| anyhow::anyhow!("{e}"))?;
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
