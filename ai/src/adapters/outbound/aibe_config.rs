//! `AIBE_CONFIG` から `shell_exec_approval` だけを読む。

use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

const DEFAULT_AIBE_CONFIG: &str = ".config/aibe/config.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AibeShellExecApproval {
    pub mode: Option<String>,
    pub source: String,
}

pub fn load_shell_exec_approval() -> AibeShellExecApproval {
    let path = resolve_aibe_config_path();
    load_shell_exec_approval_from_path(&path)
}

fn load_shell_exec_approval_from_path(path: &PathBuf) -> AibeShellExecApproval {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(_) => {
            return AibeShellExecApproval {
                mode: None,
                source: "aibe_config:missing".to_string(),
            };
        }
    };
    let Ok(file) = toml::from_str::<AibeConfigSnippet>(&raw) else {
        return AibeShellExecApproval {
            mode: None,
            source: "aibe_config:invalid".to_string(),
        };
    };
    let mode = file
        .tools
        .and_then(|tools| tools.shell_exec.and_then(|shell| shell.shell_exec_approval));
    AibeShellExecApproval {
        mode,
        source: "aibe_config".to_string(),
    }
}

fn resolve_aibe_config_path() -> PathBuf {
    if let Ok(p) = std::env::var("AIBE_CONFIG") {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(DEFAULT_AIBE_CONFIG)
}

#[derive(Debug, Deserialize)]
struct AibeConfigSnippet {
    tools: Option<ToolsSection>,
}

#[derive(Debug, Deserialize)]
struct ToolsSection {
    shell_exec: Option<ShellExecSection>,
}

#[derive(Debug, Deserialize)]
struct ShellExecSection {
    shell_exec_approval: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn reads_shell_exec_approval() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        let mut f = std::fs::File::create(&path).expect("create");
        write!(
            f,
            r#"
[tools.shell_exec]
shell_exec_approval = "ask"
"#
        )
        .expect("write");
        let loaded = load_shell_exec_approval_from_path(&path);
        assert_eq!(loaded.mode.as_deref(), Some("ask"));
        assert_eq!(loaded.source, "aibe_config");
    }
}
