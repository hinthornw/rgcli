use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use directories::UserDirs;
use serde::{Deserialize, Serialize};

const LOCAL_CONFIG_FILE: &str = ".ailsd.yaml";
const ENV_API_KEY: &str = "LANGSMITH_API_KEY";
const GITHUB_REPO: &str = "hinthornw/ailsd";
const SCHEMA_BRANCH: &str = "main";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub endpoint: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub assistant_id: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub custom_headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextConfig {
    #[serde(default = "default_context_name")]
    pub current_context: String,
    #[serde(default)]
    pub contexts: HashMap<String, Config>,
}

const CHAT_LANGCHAIN_ENDPOINT: &str =
    "https://chat-langchain-993a2fee078256ab879993a971197820.us.langgraph.app";

fn default_context_name() -> String {
    "default".to_string()
}

/// Built-in default config: Chat Langchain (no auth required).
pub fn builtin_default_config() -> Config {
    Config {
        endpoint: CHAT_LANGCHAIN_ENDPOINT.to_string(),
        api_key: String::new(),
        assistant_id: "docs_agent".to_string(),
        custom_headers: HashMap::new(),
    }
}

/// Built-in local dev config pointing to localhost:2024.
pub fn local_dev_config() -> Config {
    Config {
        endpoint: "http://localhost:2024".to_string(),
        api_key: String::new(),
        assistant_id: "agent".to_string(),
        custom_headers: HashMap::new(),
    }
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            current_context: default_context_name(),
            contexts: HashMap::from([
                ("default".to_string(), builtin_default_config()),
                ("local-dev".to_string(), local_dev_config()),
            ]),
        }
    }
}

/// Global settings (not per-context). Stored at ~/.ailsd/settings.yaml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Auto-upgrade on exit when a new version is cached (default: true)
    #[serde(default = "default_true")]
    pub auto_update: bool,
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self { auto_update: true }
    }
}

pub fn load_settings() -> Settings {
    let Ok(dir) = config_dir() else {
        return Settings::default();
    };
    let path = dir.join("settings.yaml");
    fs::read_to_string(path)
        .ok()
        .and_then(|data| serde_yaml::from_str(&data).ok())
        .unwrap_or_default()
}

/// Ensure settings.yaml exists with schema reference. Called on startup.
pub fn ensure_settings_file() {
    let Ok(dir) = config_dir() else {
        return;
    };
    let path = dir.join("settings.yaml");
    if !path.exists() {
        let _ = save_settings(&Settings::default());
    }
}

pub fn save_settings(settings: &Settings) -> Result<()> {
    let dir = config_dir()?;
    fs::create_dir_all(&dir)?;
    let path = dir.join("settings.yaml");
    let schema_url = format!(
        "https://raw.githubusercontent.com/{GITHUB_REPO}/{SCHEMA_BRANCH}/schemas/settings.json"
    );
    let data = format!(
        "# yaml-language-server: $schema={schema_url}\n{}",
        serde_yaml::to_string(settings)?
    );
    fs::write(&path, data)?;
    Ok(())
}

pub fn config_dir() -> Result<PathBuf> {
    let home = UserDirs::new()
        .ok_or_else(|| anyhow::anyhow!("unable to determine home directory"))?
        .home_dir()
        .to_path_buf();
    Ok(home.join(".ailsd"))
}

pub fn cache_dir() -> Result<PathBuf> {
    let dir = config_dir()?.join("cache");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn config_path() -> Result<String> {
    Ok(config_dir()?
        .join("config.yaml")
        .to_string_lossy()
        .to_string())
}

fn local_config_path() -> Option<PathBuf> {
    let path = PathBuf::from(LOCAL_CONFIG_FILE);
    if path.exists() { Some(path) } else { None }
}

pub fn exists() -> bool {
    local_config_path().is_some() || {
        let Ok(path) = config_dir().map(|dir| dir.join("config.yaml")) else {
            return false;
        };
        path.exists()
    }
}

/// Describes where the active config came from.
pub enum ConfigSource {
    Local(PathBuf),
    Global(String), // context name
}

/// Load the active config, resolving local override > global context > legacy migration.
pub fn load() -> Result<Config> {
    let (cfg, _source) = load_with_source()?;
    Ok(cfg)
}

pub fn load_with_source() -> Result<(Config, ConfigSource)> {
    // 1. Local .ailsd.yaml in cwd
    if let Some(path) = local_config_path() {
        let data = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let cfg: Config = serde_yaml::from_str(&data)?;
        return Ok((cfg, ConfigSource::Local(path)));
    }

    // 2. Global config
    let global_path = config_dir()?.join("config.yaml");
    let data = fs::read_to_string(&global_path).context("failed to read config")?;

    // Try new context format first
    if let Ok(ctx_cfg) = serde_yaml::from_str::<ContextConfig>(&data) {
        if !ctx_cfg.contexts.is_empty() {
            let name = &ctx_cfg.current_context;
            let cfg = ctx_cfg
                .contexts
                .get(name)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("context '{}' not found", name))?;
            return Ok((cfg, ConfigSource::Global(name.clone())));
        }
    }

    // 3. Legacy flat format — auto-migrate
    let cfg: Config = serde_yaml::from_str(&data)?;
    let ctx_cfg = ContextConfig {
        current_context: "default".to_string(),
        contexts: HashMap::from([("default".to_string(), cfg.clone())]),
    };
    save_context_config(&ctx_cfg)?;
    Ok((cfg, ConfigSource::Global("default".to_string())))
}

/// Load the full context config (for context management commands).
pub fn load_context_config() -> Result<ContextConfig> {
    let global_path = config_dir()?.join("config.yaml");
    if !global_path.exists() {
        return Ok(ContextConfig::default());
    }
    let data = fs::read_to_string(&global_path)?;

    // Try new format
    if let Ok(ctx_cfg) = serde_yaml::from_str::<ContextConfig>(&data) {
        if !ctx_cfg.contexts.is_empty() {
            return Ok(ctx_cfg);
        }
    }

    // Legacy migration
    let cfg: Config = serde_yaml::from_str(&data)?;
    let ctx_cfg = ContextConfig {
        current_context: "default".to_string(),
        contexts: HashMap::from([("default".to_string(), cfg)]),
    };
    save_context_config(&ctx_cfg)?;
    Ok(ctx_cfg)
}

pub fn save_context_config(ctx_cfg: &ContextConfig) -> Result<()> {
    let dir = config_dir()?;
    fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    }
    let path = dir.join("config.yaml");
    let schema_url = format!(
        "https://raw.githubusercontent.com/{GITHUB_REPO}/{SCHEMA_BRANCH}/schemas/config.json"
    );
    let data = format!(
        "# yaml-language-server: $schema={schema_url}\n{}",
        serde_yaml::to_string(ctx_cfg)?
    );
    fs::write(&path, data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Save a single context by name into the global config.
pub fn save_context(name: &str, cfg: &Config) -> Result<()> {
    let mut ctx_cfg = load_context_config().unwrap_or_default();
    ctx_cfg.contexts.insert(name.to_string(), cfg.clone());
    save_context_config(&ctx_cfg)
}

/// Get the current context name.
pub fn current_context_name() -> String {
    load_context_config()
        .map(|c| c.current_context)
        .unwrap_or_else(|_| "default".to_string())
}

impl Config {
    pub fn headers(&self) -> HashMap<String, String> {
        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        // API key: use stored value, fall back to LANGSMITH_API_KEY env var
        let api_key = if self.api_key.is_empty() {
            std::env::var(ENV_API_KEY).unwrap_or_default()
        } else {
            self.api_key.clone()
        };
        if !api_key.is_empty() {
            headers.insert("X-Api-Key".to_string(), api_key);
        }

        for (k, v) in &self.custom_headers {
            headers.insert(k.clone(), v.clone());
        }
        headers
    }
}
