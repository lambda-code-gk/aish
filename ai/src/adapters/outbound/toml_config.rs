//! `~/.config/ai/config.toml` アダプタ。

use std::fs;
use std::path::PathBuf;

use aibe::default_socket_path;
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct AiConfig {
    pub socket_path: PathBuf,
}

const DEFAULT_CONFIG: &str = ".config/ai/config.toml";

impl AiConfig {
    pub fn load() -> Self {
        let path = Self::resolve_path();
        let mut cfg = Self {
            socket_path: default_socket_path(),
        };
        if path.is_file() {
            if let Ok(raw) = fs::read_to_string(&path) {
                if let Ok(file) = toml::from_str::<FileConfig>(&raw) {
                    if let Some(p) = file.socket_path {
                        cfg.socket_path = expand_home(p);
                    }
                }
            }
        }
        if let Ok(p) = std::env::var("AIBE_SOCKET_PATH") {
            cfg.socket_path = PathBuf::from(p);
        }
        cfg
    }

    fn resolve_path() -> PathBuf {
        if let Ok(p) = std::env::var("AI_CONFIG") {
            return PathBuf::from(p);
        }
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home).join(DEFAULT_CONFIG)
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
    socket_path: Option<String>,
}
