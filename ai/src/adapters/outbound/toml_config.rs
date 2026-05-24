//! `~/.config/ai/config.toml` アダプタ。

use std::fs;
use std::path::PathBuf;

use aibe::default_socket_path;
use serde::Deserialize;

use crate::domain::{tokens_from_config_value, AskToolsConfigRaw, ConfigToolsTokens};

#[derive(Debug, Clone)]
pub struct AiConfig {
    pub socket_path: PathBuf,
    pub ask_tools: ConfigToolsTokens,
}

const DEFAULT_CONFIG: &str = ".config/ai/config.toml";

impl AiConfig {
    pub fn load() -> Self {
        let path = Self::resolve_path();
        let mut cfg = Self {
            socket_path: default_socket_path(),
            ask_tools: ConfigToolsTokens::default(),
        };
        if path.is_file() {
            if let Ok(raw) = fs::read_to_string(&path) {
                if let Ok(file) = toml::from_str::<FileConfig>(&raw) {
                    if let Some(p) = file.socket_path {
                        cfg.socket_path = expand_home(p);
                    }
                    if let Some(ask) = file.ask {
                        if let Some(tools) = ask.tools {
                            cfg.ask_tools = tokens_from_config_value(match tools {
                                AskToolsToml::String(s) => AskToolsConfigRaw::String(s),
                                AskToolsToml::Array(a) => AskToolsConfigRaw::Array(a),
                            });
                        }
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
    ask: Option<AskSection>,
}

#[derive(Debug, Deserialize)]
struct AskSection {
    tools: Option<AskToolsToml>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum AskToolsToml {
    String(String),
    Array(Vec<String>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    use crate::domain::{resolve_tools, AskToolsConfigRaw};

    #[test]
    fn parses_ask_tools_string_and_array() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        let mut f = std::fs::File::create(&path).expect("create");
        writeln!(
            f,
            r#"
socket_path = "/tmp/s.sock"
[ask]
tools = "@read-only,shell_exec"
"#
        )
        .expect("write");

        let raw = fs::read_to_string(&path).expect("read");
        let file: FileConfig = toml::from_str(&raw).expect("parse");
        let ask = file.ask.expect("ask");
        let tokens =
            tokens_from_config_value(AskToolsConfigRaw::String(match ask.tools.expect("tools") {
                AskToolsToml::String(s) => s,
                AskToolsToml::Array(_) => panic!("expected string"),
            }));
        let resolved = resolve_tools(None, &tokens).expect("resolve");
        assert_eq!(
            resolved.allowlist.names(),
            &["read_file".to_string(), "shell_exec".to_string()]
        );
    }

    #[test]
    fn load_config_file_cli_none_overrides_ask_tools() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        let mut f = std::fs::File::create(&path).expect("create");
        writeln!(
            f,
            r#"
[ask]
tools = "@read-only"
"#
        )
        .expect("write");

        unsafe {
            std::env::set_var("AI_CONFIG", &path);
        }
        let cfg = AiConfig::load();
        unsafe {
            std::env::remove_var("AI_CONFIG");
        }

        let resolved = resolve_tools(Some("none"), &cfg.ask_tools).expect("resolve");
        assert!(resolved.allowlist.is_empty());

        let from_config_only = resolve_tools(None, &cfg.ask_tools).expect("resolve");
        assert_eq!(
            from_config_only.allowlist.names(),
            &["read_file".to_string()]
        );
    }
}
