//! 動的 Tab 補完候補（設定ファイル読み取り専用）。

use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;

use aibe_protocol::{is_known_tool, GIT_DIFF, GIT_STATUS, GREP, LIST_DIR, READ_FILE, SHELL_EXEC};
use clap_complete::engine::CompletionCandidate;

const DEFAULT_AIBE_CONFIG: &str = ".config/aibe/config.toml";
const DEFAULT_AISH_CONFIG: &str = ".config/aish/config.toml";
const DEFAULT_AI_CONFIG: &str = ".config/ai/config.toml";

const TOOL_CATEGORIES: &[&str] = &["@read-only", "@exec", "@full", "none", "@none"];
const TOOL_NAMES: &[&str] = &[READ_FILE, LIST_DIR, GREP, GIT_DIFF, GIT_STATUS, SHELL_EXEC];

/// `aibe` 設定 `[profiles.*]` のキー名。
pub fn list_profile_names() -> Vec<String> {
    let path = resolve_aibe_config_path();
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(value) = toml::from_str::<toml::Value>(&raw) else {
        return Vec::new();
    };
    let Some(table) = value.get("profiles").and_then(|v| v.as_table()) else {
        return Vec::new();
    };
    let mut names: Vec<String> = table.keys().cloned().collect();
    names.sort();
    names
}

/// `aish` 設定 `log_dir` 配下の 12 桁小文字 hex session id。
pub fn list_session_ids() -> Vec<String> {
    let Some(log_dir) = resolve_aish_log_dir() else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(log_dir) else {
        return Vec::new();
    };
    let mut ids = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let s = name.to_string_lossy();
        if is_session_id(&s) && entry.path().is_dir() {
            ids.push(s.into_owned());
        }
    }
    ids.sort();
    ids
}

pub fn complete_profile(prefix: &OsStr) -> Vec<CompletionCandidate> {
    to_candidates(filter_prefix(
        list_profile_names(),
        &prefix.to_string_lossy(),
    ))
}

pub fn complete_preset(prefix: &OsStr) -> Vec<CompletionCandidate> {
    to_candidates(filter_prefix(
        list_preset_names(),
        &prefix.to_string_lossy(),
    ))
}

pub fn complete_session(prefix: &OsStr) -> Vec<CompletionCandidate> {
    to_candidates(filter_prefix(list_session_ids(), &prefix.to_string_lossy()))
}

pub fn complete_tools_token(input: &OsStr) -> Vec<CompletionCandidate> {
    let input = input.to_string_lossy();
    let segments: Vec<&str> = input.split(',').map(str::trim).collect();
    let trailing_empty = input.ends_with(',');
    let completed: Vec<&str> = segments.iter().copied().filter(|s| !s.is_empty()).collect();
    let token = if trailing_empty {
        ""
    } else {
        completed.last().copied().unwrap_or("")
    };
    let prior: Vec<&str> = if trailing_empty {
        completed
    } else {
        completed[..completed.len().saturating_sub(1)].to_vec()
    };

    if prior.iter().any(|t| *t == "none" || *t == "@none") {
        return Vec::new();
    }
    if !prior.is_empty() && (token == "none" || token == "@none") {
        return Vec::new();
    }

    let mut candidates: Vec<String> = TOOL_CATEGORIES
        .iter()
        .chain(TOOL_NAMES.iter())
        .filter(|name| is_known_tool(name) || name.starts_with('@') || **name == "none")
        .map(|s| (*s).to_string())
        .collect();
    if !prior.is_empty() {
        candidates.retain(|c| c != "none" && c != "@none");
    }
    candidates.sort();
    candidates.dedup();
    to_candidates(filter_prefix(candidates, token))
}

fn to_candidates(items: Vec<String>) -> Vec<CompletionCandidate> {
    items.into_iter().map(CompletionCandidate::new).collect()
}

fn filter_prefix(items: Vec<String>, prefix: &str) -> Vec<String> {
    items
        .into_iter()
        .filter(|item| item.starts_with(prefix))
        .collect()
}

fn is_session_id(s: &str) -> bool {
    s.len() == 12
        && s.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn resolve_aibe_config_path() -> PathBuf {
    if let Ok(p) = std::env::var("AIBE_CONFIG") {
        return PathBuf::from(p);
    }
    home_config(DEFAULT_AIBE_CONFIG)
}

fn list_preset_names() -> Vec<String> {
    let path = resolve_ai_config_path();
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(value) = toml::from_str::<toml::Value>(&raw) else {
        return Vec::new();
    };
    let Some(table) = value.get("presets").and_then(|v| v.as_table()) else {
        return Vec::new();
    };
    let mut names: Vec<String> = table.keys().cloned().collect();
    names.sort();
    names
}

fn resolve_ai_config_path() -> PathBuf {
    if let Ok(p) = std::env::var("AI_CONFIG") {
        return PathBuf::from(p);
    }
    home_config(DEFAULT_AI_CONFIG)
}

fn resolve_aish_log_dir() -> Option<PathBuf> {
    let explicit = std::env::var("AISH_CONFIG").ok();
    let path = explicit
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| home_config(DEFAULT_AISH_CONFIG));

    if !path.is_file() {
        if explicit.is_some() {
            return None;
        }
        return Some(default_aish_log_dir());
    }

    let Ok(raw) = fs::read_to_string(&path) else {
        return None;
    };
    let Ok(value) = toml::from_str::<toml::Value>(&raw) else {
        return None;
    };
    if let Some(dir) = value.get("log_dir").and_then(|v| v.as_str()) {
        return Some(expand_home(dir));
    }
    None
}

fn default_aish_log_dir() -> PathBuf {
    home_dir().join(".local/share/aish/sessions")
}

fn expand_home(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        return home_dir().join(rest);
    }
    PathBuf::from(path)
}

fn home_config(rel: &str) -> PathBuf {
    home_dir().join(rel)
}

fn home_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_id_validation() {
        assert!(is_session_id("002f15d02b54"));
        assert!(!is_session_id("002F15D02B54"));
        assert!(!is_session_id("123"));
    }

    #[test]
    fn tools_token_uses_last_comma_segment() {
        let out = complete_tools_token(OsStr::new("@read,she"));
        assert!(out
            .iter()
            .any(|c| c.get_value().to_string_lossy().starts_with("she")));
    }

    #[test]
    fn tools_token_omits_none_when_prior_tokens_exist() {
        let out = complete_tools_token(OsStr::new("@read-only,"));
        let values: Vec<String> = out
            .iter()
            .map(|c| c.get_value().to_string_lossy().into_owned())
            .collect();
        assert!(!values.iter().any(|v| v == "none" || v == "@none"));
    }

    #[test]
    fn session_ids_empty_when_aish_config_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("missing.toml");
        std::env::set_var("AISH_CONFIG", &missing);
        assert!(list_session_ids().is_empty());
        std::env::remove_var("AISH_CONFIG");
    }
}
