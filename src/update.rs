use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config;

const GITHUB_REPO: &str = "hinthornw/ailsd";
const CHECK_INTERVAL_SECS: i64 = 4 * 60 * 60; // 4 hours

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateState {
    #[serde(default)]
    pub last_check: Option<DateTime<Utc>>,
    #[serde(default)]
    pub latest_version: Option<String>,
    #[serde(default)]
    pub cached_binary: Option<String>,
    #[serde(default)]
    pub current_version: Option<String>,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
}

fn state_path() -> Result<PathBuf> {
    Ok(config::cache_dir()?.join("update-state.json"))
}

fn read_state() -> UpdateState {
    let Ok(path) = state_path() else {
        return UpdateState::default();
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default()
}

fn write_state(state: &UpdateState) -> Result<()> {
    let path = state_path()?;
    let data = serde_json::to_string_pretty(state)?;
    fs::write(path, data)?;
    Ok(())
}

fn current_version() -> String {
    option_env!("AILSD_VERSION")
        .unwrap_or(env!("CARGO_PKG_VERSION"))
        .to_string()
}

fn platform_target() -> Result<(&'static str, &'static str)> {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        other => anyhow::bail!("unsupported OS: {other}"),
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => anyhow::bail!("unsupported architecture: {other}"),
    };
    Ok((os, arch))
}

async fn fetch_latest_version(http: &reqwest::Client) -> Result<String> {
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let release: GithubRelease = http
        .get(&url)
        .header("User-Agent", "ailsd")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(release.tag_name)
}

async fn download_binary(http: &reqwest::Client, version: &str) -> Result<PathBuf> {
    let (os, arch) = platform_target()?;
    let ver_no_v = version.strip_prefix('v').unwrap_or(version);
    let filename = format!("ailsd_{ver_no_v}_{os}_{arch}.tar.gz");
    let url = format!(
        "https://github.com/{GITHUB_REPO}/releases/download/{version}/{filename}"
    );

    let cache = config::cache_dir()?;
    let archive_path = cache.join(&filename);
    let binary_path = cache.join(format!("ailsd-{version}"));

    let bytes = http
        .get(&url)
        .header("User-Agent", "ailsd")
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;

    fs::write(&archive_path, &bytes)?;

    // Extract the binary from tar.gz
    let tar_gz = fs::File::open(&archive_path)?;
    let tar = flate2::read::GzDecoder::new(tar_gz);
    let mut archive = tar::Archive::new(tar);
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;
        if path.file_name().and_then(|n| n.to_str()) == Some("ailsd") {
            entry.unpack(&binary_path)?;
            break;
        }
    }

    // Clean up archive
    let _ = fs::remove_file(&archive_path);

    if !binary_path.exists() {
        anyhow::bail!("binary not found in archive");
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&binary_path, fs::Permissions::from_mode(0o755))?;
    }

    Ok(binary_path)
}

fn needs_check(state: &UpdateState) -> bool {
    match state.last_check {
        None => true,
        Some(last) => {
            let elapsed = Utc::now().signed_duration_since(last).num_seconds();
            elapsed >= CHECK_INTERVAL_SECS
        }
    }
}

fn is_newer(latest: &str, current: &str) -> bool {
    let strip = |v: &str| v.strip_prefix('v').unwrap_or(v).to_string();
    strip(latest) != strip(current)
}

/// Run in background on startup. Silently checks for updates and pre-downloads.
pub async fn background_check() -> Result<()> {
    let state = read_state();
    if !needs_check(&state) {
        return Ok(());
    }

    let http = reqwest::Client::new();
    let latest = fetch_latest_version(&http).await?;
    let current = current_version();

    let mut new_state = UpdateState {
        last_check: Some(Utc::now()),
        latest_version: Some(latest.clone()),
        current_version: Some(current.clone()),
        cached_binary: state.cached_binary.clone(),
    };

    if is_newer(&latest, &current) {
        // Check if we already cached this version
        let already_cached = state
            .cached_binary
            .as_ref()
            .map(|p| PathBuf::from(p).exists() && p.contains(&latest))
            .unwrap_or(false);

        if !already_cached {
            let path = download_binary(&http, &latest).await?;
            new_state.cached_binary = Some(path.to_string_lossy().to_string());
        }
    }

    write_state(&new_state)?;
    Ok(())
}

/// Check if there's a pending update and return a notice string.
pub fn pending_update_notice() -> Option<String> {
    let state = read_state();
    let latest = state.latest_version.as_ref()?;
    let current = current_version();
    if is_newer(latest, &current) {
        Some(format!(
            "Update available: {latest} (current: v{current}). Run `ailsd upgrade` to update.",
            current = current.strip_prefix('v').unwrap_or(&current),
        ))
    } else {
        None
    }
}

/// Explicit upgrade command.
pub async fn run_upgrade() -> Result<()> {
    let current = current_version();
    println!("Current version: v{}", current.strip_prefix('v').unwrap_or(&current));
    println!("Checking for updates...");

    let http = reqwest::Client::new();
    let latest = fetch_latest_version(&http)
        .await
        .context("failed to check for updates")?;

    if !is_newer(&latest, &current) {
        println!("Already up to date.");
        return Ok(());
    }

    println!("New version available: {latest}");

    // Check for cached binary first
    let state = read_state();
    let cached = state
        .cached_binary
        .as_ref()
        .filter(|p| PathBuf::from(p).exists() && p.contains(&latest));

    let binary_path = if let Some(path) = cached {
        println!("Using cached download...");
        PathBuf::from(path)
    } else {
        println!("Downloading {latest}...");
        download_binary(&http, &latest).await.context("failed to download update")?
    };

    // Self-replace
    let current_exe = std::env::current_exe().context("failed to determine current executable")?;
    let current_exe = current_exe
        .canonicalize()
        .unwrap_or(current_exe);

    println!("Installing...");

    // Try direct copy first (works if we have write permission, e.g. ~/.local/bin)
    // We use copy+rename instead of direct rename to handle cross-filesystem moves
    let backup = current_exe.with_extension("bak");
    let tmp_new = current_exe.with_extension("new");

    if fs::copy(&binary_path, &tmp_new).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&tmp_new, fs::Permissions::from_mode(0o755));
        }
        // Backup old, swap in new
        let _ = fs::rename(&current_exe, &backup);
        if let Err(e) = fs::rename(&tmp_new, &current_exe) {
            let _ = fs::rename(&backup, &current_exe);
            let _ = fs::remove_file(&tmp_new);
            anyhow::bail!("failed to install new binary: {e}");
        }
        let _ = fs::remove_file(&backup);
    } else {
        // Need elevated permissions (e.g. /usr/local/bin)
        println!("Requires sudo to update {}...", current_exe.display());
        let _ = fs::remove_file(&tmp_new);
        let status = std::process::Command::new("sudo")
            .args(["cp", "-f"])
            .arg(&binary_path)
            .arg(&current_exe)
            .status()
            .context("failed to run sudo")?;
        if !status.success() {
            anyhow::bail!("sudo install failed");
        }
    }

    // Clear cached state
    let new_state = UpdateState {
        last_check: Some(Utc::now()),
        latest_version: Some(latest.clone()),
        cached_binary: None,
        current_version: Some(latest.clone()),
    };
    let _ = write_state(&new_state);

    // Clean up cached binary if it still exists
    let _ = fs::remove_file(&binary_path);

    println!("Successfully upgraded to {latest}!");
    Ok(())
}
