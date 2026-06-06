//! aibe 設定から `[[external_commands]]` 名のみ読む（非秘密メタデータ）。

use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

const DEFAULT_AIBE_CONFIG: &str = ".config/aibe/config.toml";

/// 登録済み外部コマンド名（設定ファイルが無い・節が無いときは空）。
pub fn external_command_names() -> Vec<String> {
    external_command_names_from_path(&resolve_aibe_config_path())
}

fn external_command_names_from_path(path: &std::path::Path) -> Vec<String> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(_) => return Vec::new(),
    };
    let file: AibeConfigSnippet = match toml::from_str(&raw) {
        Ok(file) => file,
        Err(_) => return Vec::new(),
    };
    file.external_commands
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.name)
        .collect()
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
    #[serde(default)]
    external_commands: Option<Vec<ExternalCommandEntry>>,
}

#[derive(Debug, Deserialize)]
struct ExternalCommandEntry {
    name: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn reads_external_command_names() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        let mut f = std::fs::File::create(&path).expect("create");
        write!(
            f,
            r#"
[[external_commands]]
name = "codex"
command = "codex"

[[external_commands]]
name = "claude"
command = "claude"
"#
        )
        .expect("write");
        assert_eq!(
            external_command_names_from_path(&path),
            vec!["codex".to_string(), "claude".to_string()]
        );
    }

    #[test]
    fn missing_file_returns_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("missing.toml");
        assert!(external_command_names_from_path(&path).is_empty());
    }
}
