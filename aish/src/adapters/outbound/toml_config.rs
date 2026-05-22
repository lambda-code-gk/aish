//! `~/.config/aish/config.toml` アダプタ。

use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct AishConfig {
    pub log_dir: PathBuf,
    pub shell: String,
}

const DEFAULT_CONFIG: &str = ".config/aish/config.toml";

impl AishConfig {
    pub fn load() -> Self {
        let path = Self::resolve_path();
        if path.is_file() {
            if let Ok(raw) = fs::read_to_string(&path) {
                if let Ok(file) = toml::from_str::<FileConfig>(&raw) {
                    return Self::from_file(file);
                }
            }
        }
        Self::default()
    }

    fn resolve_path() -> PathBuf {
        if let Ok(p) = std::env::var("AISH_CONFIG") {
            return PathBuf::from(p);
        }
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home).join(DEFAULT_CONFIG)
    }

    fn from_file(file: FileConfig) -> Self {
        let mut cfg = Self::default();
        if let Some(dir) = file.log_dir {
            cfg.log_dir = expand_home(dir);
        }
        if let Some(shell) = file.shell {
            cfg.shell = shell;
        }
        cfg
    }

    pub fn default_session_log(&self) -> PathBuf {
        let name = format!("session-{}.jsonl", std::process::id());
        self.log_dir.join(name)
    }
}

impl Default for AishConfig {
    fn default() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        Self {
            log_dir: PathBuf::from(home).join(".local/share/aish/sessions"),
            shell: std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string()),
        }
    }
}

fn expand_home(path: String) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(path)
}

#[derive(Debug, Deserialize)]
struct FileConfig {
    log_dir: Option<String>,
    shell: Option<String>,
}
