//! The config file: one key, the library folder path. It lives in the XDG
//! config directory, or at the path in `EPUBSYNC_CONFIG` when that is set,
//! which the tests use.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub library: PathBuf,
}

/// The message every command prints when the config file is missing.
pub fn missing_message(path: &Path) -> String {
    format!(
        "no config file at {}. Run `epubsync init <folder>` to create a library.",
        path.display()
    )
}

pub fn path() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("EPUBSYNC_CONFIG") {
        return Ok(PathBuf::from(p));
    }
    let dirs = directories::ProjectDirs::from("", "", "epubsync")
        .ok_or_else(|| anyhow!("no home directory to place the config file in"))?;
    Ok(dirs.config_dir().join("config.toml"))
}

pub fn load() -> Result<Config> {
    let path = path()?;
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(anyhow!(missing_message(&path)));
        }
        Err(e) => return Err(e).with_context(|| format!("read {}", path.display())),
    };
    toml::from_str(&text).with_context(|| format!("parse {}", path.display()))
}

pub fn save(config: &Config) -> Result<PathBuf> {
    let path = path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let text = toml::to_string(config)?;
    std::fs::write(&path, text).with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}
