// ASCII logo management.
//
// Logos are stored as plain text files in the logo_dir. The filename is the
// key used in config (e.g., "arch", "nixos", "ubuntu"). The logos/ directory
// lives next to the binary or under ~/.config/atlasfetch/logos/.
//
// On first run, logos are copied from the binary's adjacent logos/ directory
// into the user's config directory so that updates don't break existing configs.

use color_eyre::Result;
use std::fs;
use std::path::PathBuf;

use crate::config;

/// All available built-in logo keys.
pub fn available_logos() -> Result<Vec<String>> {
    let dir = config::logo_dir()?;
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut keys: Vec<String> = fs::read_dir(&dir)?
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    keys.sort();
    // Filter out _small variants (legacy) and hidden files
    keys.retain(|k| !k.starts_with('.') && !k.ends_with("_small"));
    Ok(keys)
}

/// Load the ASCII art for the current config.
pub fn load(cfg: &config::Config) -> Result<String> {
    // Try the configured key first
    if !cfg.logo.key.is_empty() {
        let dir = config::logo_dir()?;
        let path = dir.join(&cfg.logo.key);
        if let Ok(art) = fs::read_to_string(&path) {
            return Ok(art.trim_end_matches('\n').to_string());
        }
    }

    // Fall back to logo path
    let logo_path = shellexpand(&cfg.logo.path)?;
    if let Ok(art) = fs::read_to_string(&logo_path) {
        let trimmed = art.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(trimmed);
        }
    }

    // Ultimate fallback: a minimal Arch-like logo
    Ok(default_ascii())
}

/// Copy logos from the binary's adjacent directory to the user config dir.
pub fn ensure_logos() -> Result<()> {
    let exe = std::env::current_exe()?;
    let exe_dir = exe.parent().unwrap_or(std::path::Path::new("/"));
    let src = exe_dir.join("logos");
    let dst = config::config_dir()?.join("logos");

    if dst.exists() {
        return Ok(());
    }
    if !src.exists() {
        return Ok(());
    }

    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(&src)? {
        let entry = entry?;
        let ftype = entry.file_type()?;
        if ftype.is_file() {
            let name = entry.file_name();
            fs::copy(entry.path(), dst.join(&name))?;
        }
    }
    Ok(())
}

fn shellexpand(s: &str) -> Result<PathBuf> {
    if let Some(rest) = s.strip_prefix('~') {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        Ok(PathBuf::from(home).join(rest.trim_start_matches('/')))
    } else {
        Ok(PathBuf::from(s))
    }
}

fn default_ascii() -> String {
    r#"                    -`
                   .o+`
                  `ooo/
                 `+oooo:
                `+oooooo:
                -+oooooo+:
              `/:-:++oooo+:
             `/+++++/++++++:
            `/++++++++++++++:
           `/+++ooooooooooooo/`
          ./ooosssso++osssssso+`
        .oossssso-````/ossssss+`
       -osssssso.      :ssssssso.
      :osssssss/        osssso+++.
     /ossssssss/        +ssssooo/-
   `/ossssso+/:-        -:/+osssso+-
  `+sso+:-`                 `.-/+oso:
 `++:.                           `-/+/
 .`                                 `/`"#.into()
}
