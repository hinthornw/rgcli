use anyhow::{Context, Result};

use crate::config;

pub fn list() -> Result<()> {
    let ctx_cfg = config::load_context_config()?;

    if ctx_cfg.contexts.is_empty() {
        println!("No contexts configured. Run `ailsd context create <name>` to create one.");
        return Ok(());
    }

    // Check if local override is active
    let local_active = config::load_with_source()
        .ok()
        .map(|(_, source)| matches!(source, config::ConfigSource::Local(_)))
        .unwrap_or(false);

    let mut names: Vec<&String> = ctx_cfg.contexts.keys().collect();
    names.sort();

    for name in names {
        let cfg = &ctx_cfg.contexts[name];
        let is_current = name == &ctx_cfg.current_context && !local_active;
        let marker = if is_current { "* " } else { "  " };
        println!("{}{:<20} {}", marker, name, cfg.endpoint);
    }

    if local_active {
        println!("\n  (local .ailsd.yaml override is active)");
    }

    Ok(())
}

pub fn current() -> Result<()> {
    let (cfg, source) = config::load_with_source()?;
    match source {
        config::ConfigSource::Local(path) => {
            println!("Active: local override ({})", path.display());
        }
        config::ConfigSource::Global(name) => {
            println!("Active: {}", name);
        }
    }
    println!("Endpoint: {}", cfg.endpoint);
    if !cfg.assistant_id.is_empty() {
        println!("Assistant: {}", cfg.assistant_id);
    }
    let has_key = !cfg.api_key.is_empty()
        || std::env::var("LANGSMITH_API_KEY").is_ok();
    println!("Auth: {}", if has_key { "API key" } else { "none" });
    Ok(())
}

pub fn use_context(name: &str) -> Result<()> {
    let mut ctx_cfg = config::load_context_config()?;
    if !ctx_cfg.contexts.contains_key(name) {
        anyhow::bail!("context '{}' not found. Run `ailsd context list` to see available contexts.", name);
    }
    ctx_cfg.current_context = name.to_string();
    config::save_context_config(&ctx_cfg)?;
    println!("Switched to context '{}'.", name);
    Ok(())
}

pub fn show(name: Option<&str>) -> Result<()> {
    let ctx_cfg = config::load_context_config()?;
    let name = name.unwrap_or(&ctx_cfg.current_context);
    let cfg = ctx_cfg
        .contexts
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("context '{}' not found", name))?;

    println!("Context: {}", name);
    println!("Endpoint: {}", cfg.endpoint);
    println!("Assistant: {}", cfg.assistant_id);

    let masked_key = if cfg.api_key.is_empty() {
        if std::env::var("LANGSMITH_API_KEY").is_ok() {
            "(from LANGSMITH_API_KEY env)".to_string()
        } else {
            "(none)".to_string()
        }
    } else {
        let k = &cfg.api_key;
        if k.len() > 8 {
            format!("{}...{}", &k[..4], &k[k.len() - 4..])
        } else {
            "****".to_string()
        }
    };
    println!("API Key: {}", masked_key);

    if !cfg.custom_headers.is_empty() {
        println!("Custom Headers:");
        for k in cfg.custom_headers.keys() {
            println!("  {}: ****", k);
        }
    }

    Ok(())
}

pub fn delete(name: &str) -> Result<()> {
    let mut ctx_cfg = config::load_context_config()?;
    if !ctx_cfg.contexts.contains_key(name) {
        anyhow::bail!("context '{}' not found", name);
    }
    if ctx_cfg.contexts.len() == 1 {
        anyhow::bail!("cannot delete the last context");
    }
    ctx_cfg.contexts.remove(name);
    if ctx_cfg.current_context == name {
        ctx_cfg.current_context = ctx_cfg.contexts.keys().next().unwrap().clone();
        println!("Switched to context '{}'.", ctx_cfg.current_context);
    }
    config::save_context_config(&ctx_cfg)?;
    println!("Deleted context '{}'.", name);
    Ok(())
}

pub fn create_interactive(name: &str) -> Result<()> {
    let ctx_cfg = config::load_context_config().unwrap_or_default();
    if ctx_cfg.contexts.contains_key(name) {
        anyhow::bail!("context '{}' already exists. Use `ailsd context show {}` or delete it first.", name, name);
    }
    println!("Creating context '{}'...\n", name);
    let cfg = crate::run_configure_inner(None).context("configuration failed")?;
    config::save_context(name, &cfg)?;
    println!("\nContext '{}' created.", name);
    Ok(())
}
