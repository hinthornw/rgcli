use std::fs;
use std::path::Path;

use anyhow::Result;

pub fn run() -> Result<()> {
    let exe = std::env::current_exe()?;
    let exe_str = exe.to_string_lossy();

    // Detect Homebrew install
    if exe_str.contains("/Cellar/") || exe_str.contains("/homebrew/") {
        println!("ailsd was installed via Homebrew. Run:\n");
        println!("  brew uninstall ailsd");
        return Ok(());
    }

    println!("This will remove ailsd from {}", exe.display());
    if !confirm("Proceed?") {
        return Ok(());
    }

    // Ask about config/cache
    let config_dir = dirs_home().map(|h| format!("{h}/.ailsd"));
    let remove_config = config_dir
        .as_ref()
        .map(|d| Path::new(d).exists())
        .unwrap_or(false)
        && confirm("Also remove configuration and cache (~/.ailsd/)?");

    if remove_config {
        if let Some(ref dir) = config_dir {
            fs::remove_dir_all(dir)?;
        }
    }

    // Remove binary — on Unix the running process keeps its memory-mapped copy
    if let Err(e) = fs::remove_file(&exe) {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            eprintln!("Permission denied. Try:\n\n  sudo rm {}", exe.display());
            std::process::exit(1);
        }
        return Err(e.into());
    }

    println!("\nRemoved {}", exe.display());
    if remove_config {
        println!("Removed ~/.ailsd/");
    } else if config_dir
        .as_ref()
        .map(|d| Path::new(d).exists())
        .unwrap_or(false)
    {
        println!("Config preserved at ~/.ailsd/");
    }

    Ok(())
}

fn confirm(prompt: &str) -> bool {
    eprint!("{prompt} [y/N] ");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();
    matches!(input.trim(), "y" | "Y" | "yes" | "Yes")
}

fn dirs_home() -> Option<String> {
    std::env::var("HOME").ok()
}
