//! `~/.config/aibe/config.toml` アダプタ。

use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

use crate::ports::outbound::{AppConfig, ConfigError, ConfigLoader, LlmConfig, ToolsConfig};

const DEFAULT_CONFIG_PATH: &str = ".config/aibe/config.toml";

/// TOML + 環境変数オーバーライド。
pub struct TomlConfig {
    path: PathBuf,
}

impl TomlConfig {
    pub fn load() -> Result<AppConfig, ConfigError> {
        Self::from_path(Self::resolve_path()).load()
    }

    pub fn from_path(path: PathBuf) -> Self {
        Self { path }
    }

    fn resolve_path() -> PathBuf {
        if let Ok(p) = std::env::var("AIBE_CONFIG") {
            return PathBuf::from(p);
        }
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home).join(DEFAULT_CONFIG_PATH)
    }
}

impl ConfigLoader for TomlConfig {
    fn load(&self) -> Result<AppConfig, ConfigError> {
        let file_cfg = if self.path.is_file() {
            let raw = fs::read_to_string(&self.path)
                .map_err(|e| ConfigError::Io(format!("{}: {e}", self.path.display())))?;
            Some(
                toml::from_str::<FileConfig>(&raw)
                    .map_err(|e| ConfigError::Invalid(e.to_string()))?,
            )
        } else {
            None
        };

        let socket_path = std::env::var("AIBE_SOCKET_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                file_cfg
                    .as_ref()
                    .and_then(|c| c.socket_path.clone())
                    .map(expand_home)
                    .unwrap_or_else(crate::default_socket_path)
            });

        let llm = parse_llm(file_cfg.as_ref())?;
        let tools = parse_tools(file_cfg.as_ref());
        Ok(AppConfig {
            socket_path,
            llm,
            tools,
        })
    }
}

fn parse_tools(file: Option<&FileConfig>) -> ToolsConfig {
    let section = file.and_then(|c| c.tools.as_ref());
    let mut tools = ToolsConfig::default();
    if let Some(t) = section {
        if let Some(n) = t.max_rounds {
            tools.max_rounds = n;
        }
        if let Some(ms) = t.exec_timeout_ms {
            tools.exec_timeout_ms = ms;
        }
        if let Some(n) = t.max_tool_output_bytes {
            tools.max_tool_output_bytes = n;
        }
        if let Some(shell) = t.shell_exec.as_ref() {
            if let Some(enabled) = shell.enabled {
                tools.shell_exec.enabled = enabled;
            }
            if let Some(cmds) = shell.allowed_commands.clone() {
                tools.shell_exec.allowed_commands = cmds;
            }
        }
        if let Some(rf) = t.read_file.as_ref() {
            if let Some(roots) = rf.allowed_roots.clone() {
                tools.read_file.allowed_roots = roots.into_iter().map(expand_home).collect();
            }
        }
    }
    tools
}

fn parse_llm(file: Option<&FileConfig>) -> Result<LlmConfig, ConfigError> {
    let section = file.and_then(|c| c.llm.as_ref());
    let provider = section
        .and_then(|l| l.provider.clone())
        .or_else(|| std::env::var("AIBE_LLM_PROVIDER").ok())
        .unwrap_or_else(|| "mock".to_string());

    match provider.as_str() {
        "mock" => Ok(LlmConfig::Mock),
        "openai_compatible" | "openai-compatible" => {
            let api_key = section
                .and_then(|l| l.api_key.clone())
                .or_else(|| std::env::var("AIBE_API_KEY").ok())
                .filter(|k| !k.is_empty())
                .ok_or_else(|| {
                    ConfigError::Invalid(
                        "openai_compatible requires api_key in config or AIBE_API_KEY".into(),
                    )
                })?;
            let base_url = section
                .and_then(|l| l.base_url.clone())
                .or_else(|| std::env::var("AIBE_BASE_URL").ok())
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
            let model = section
                .and_then(|l| l.model.clone())
                .or_else(|| std::env::var("AIBE_MODEL").ok())
                .unwrap_or_else(|| "gpt-4o-mini".to_string());
            Ok(LlmConfig::OpenAiCompatible {
                base_url: base_url.trim_end_matches('/').to_string(),
                api_key,
                model,
            })
        }
        other => Err(ConfigError::Invalid(format!(
            "unknown llm provider: {other}"
        ))),
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
    llm: Option<LlmSection>,
    tools: Option<ToolsSection>,
}

#[derive(Debug, Deserialize)]
struct ToolsSection {
    max_rounds: Option<u32>,
    exec_timeout_ms: Option<u64>,
    max_tool_output_bytes: Option<usize>,
    shell_exec: Option<ShellExecSection>,
    read_file: Option<ReadFileSection>,
}

#[derive(Debug, Deserialize)]
struct ShellExecSection {
    enabled: Option<bool>,
    allowed_commands: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct ReadFileSection {
    allowed_roots: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct LlmSection {
    provider: Option<String>,
    api_key: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tools_section() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
[tools]
max_rounds = 3
exec_timeout_ms = 5000
max_tool_output_bytes = 4096

[tools.shell_exec]
enabled = true
allowed_commands = ["echo", "git"]

[tools.read_file]
allowed_roots = ["~/projects"]
"#,
        )
        .expect("write");

        let cfg = TomlConfig::from_path(path).load().expect("load");
        assert_eq!(cfg.tools.max_rounds, 3);
        assert_eq!(cfg.tools.exec_timeout_ms, 5000);
        assert_eq!(cfg.tools.max_tool_output_bytes, 4096);
        assert!(cfg.tools.shell_exec.enabled);
        assert_eq!(cfg.tools.shell_exec.allowed_commands, vec!["echo", "git"]);
        assert_eq!(cfg.tools.read_file.allowed_roots.len(), 1);
    }

    #[test]
    fn parses_openai_compatible() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
socket_path = "/tmp/aibe.sock"
[llm]
provider = "openai_compatible"
api_key = "test-key"
base_url = "http://127.0.0.1:8080/v1"
model = "local"
"#,
        )
        .expect("write");

        let cfg = TomlConfig::from_path(path).load().expect("load");
        assert_eq!(cfg.socket_path, PathBuf::from("/tmp/aibe.sock"));
        match cfg.llm {
            LlmConfig::OpenAiCompatible {
                base_url,
                api_key,
                model,
            } => {
                assert_eq!(base_url, "http://127.0.0.1:8080/v1");
                assert_eq!(api_key, "test-key");
                assert_eq!(model, "local");
            }
            LlmConfig::Mock => panic!("expected openai_compatible"),
        }
    }
}
