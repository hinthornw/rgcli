use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use directories::UserDirs;
use serde::{Deserialize, Serialize};

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
    Ok(config_dir()?.join("config.yaml").to_string_lossy().to_string())
}

pub fn exists() -> bool {
    let Ok(path) = config_dir().map(|dir| dir.join("config.yaml")) else {
        return false;
    };
    path.exists()
}

pub fn load() -> Result<Config> {
    let path = config_dir()?.join("config.yaml");
    let data = fs::read_to_string(path)?;
    let cfg = serde_yaml::from_str(&data)?;
    Ok(cfg)
}

pub fn save(cfg: &Config) -> Result<()> {
    let dir = config_dir()?;
    fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    }
    let path = dir.join("config.yaml");
    let data = serde_yaml::to_string(cfg)?;
    fs::write(path, data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir.join("config.yaml"), fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

impl Config {
    pub fn headers(&self) -> HashMap<String, String> {
        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());
        if !self.api_key.is_empty() {
            headers.insert("X-Api-Key".to_string(), self.api_key.clone());
        }
        for (k, v) in &self.custom_headers {
            headers.insert(k.clone(), v.clone());
        }
        headers
    }
}
