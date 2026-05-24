//! `ai ask` のツール allowlist 解決（`docs/0002_ai-tools-client-spec.md`）。
//! cwd・送信 payload・レイヤー分割は `docs/0003_architecture-review-refactor-spec.md`。
//!
//! ツール名の正本は `aibe::READ_FILE` / `aibe::SHELL_EXEC`。

use aibe::{is_known_tool, READ_FILE, SHELL_EXEC};
use thiserror::Error;

/// 展開・検証済みツール名の集合。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToolAllowlist {
    names: Vec<String>,
}

impl ToolAllowlist {
    pub fn names(&self) -> &[String] {
        &self.names
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    pub fn into_names(self) -> Vec<String> {
        self.names
    }
}

/// 解決済み allowlist と起動時表示用メタデータ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTools {
    pub allowlist: ToolAllowlist,
    pub startup: ToolsStartupLine,
}

/// 起動時 `stderr` 1 行分のメタデータ（表示は adapter 層）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolsStartupLine {
    /// `read_file` / `read_file, shell_exec` / `none`
    pub enabled_list: String,
    /// 括弧内の元指定（`@read-only` 等）。`none` のときは `None`。
    pub source_hint: Option<String>,
    pub warn_shell: bool,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ToolsResolveError {
    #[error("unknown tool category: {0}")]
    UnknownCategory(String),
    #[error("unknown tool: {0}")]
    UnknownTool(String),
    #[error("none cannot be combined with other tools")]
    NoneMixed,
}

/// config 由来のトークン列（未展開）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConfigToolsTokens(pub Vec<String>);

/// CLI `--tools LIST` が指定されたときは config を捨て、LIST のみを使う。
pub fn resolve_tools(
    cli_list: Option<&str>,
    config: &ConfigToolsTokens,
) -> Result<ResolvedTools, ToolsResolveError> {
    let tokens = match cli_list {
        Some(list) => split_comma_list(list),
        None => config.0.clone(),
    };
    resolve_tokens(&tokens)
}

/// config の `tools` フィールド（文字列 or 配列）をトークン列にする。
pub fn tokens_from_config_value(raw: AskToolsConfigRaw) -> ConfigToolsTokens {
    match raw {
        AskToolsConfigRaw::String(s) => ConfigToolsTokens(split_comma_list(&s)),
        AskToolsConfigRaw::Array(items) => {
            ConfigToolsTokens(items.into_iter().map(|s| s.trim().to_string()).collect())
        }
    }
}

#[derive(Debug, Clone)]
pub enum AskToolsConfigRaw {
    String(String),
    Array(Vec<String>),
}

fn resolve_tokens(tokens: &[String]) -> Result<ResolvedTools, ToolsResolveError> {
    if tokens.is_empty() {
        return Ok(empty_resolved());
    }

    if tokens.len() == 1 {
        let t = tokens[0].as_str();
        if t == "none" || t == "@none" {
            return Ok(empty_resolved());
        }
    }

    if tokens.iter().any(|t| t == "none" || t == "@none") {
        return Err(ToolsResolveError::NoneMixed);
    }

    let source_hint = tokens.join(",");
    let mut expanded = Vec::new();
    for token in tokens {
        let t = token.trim();
        if t.is_empty() {
            continue;
        }
        if let Some(names) = expand_category(t)? {
            expanded.extend(names.iter().map(|s| (*s).to_string()));
        } else if is_known_tool(t) {
            expanded.push(t.to_string());
        } else if t.starts_with('@') {
            return Err(ToolsResolveError::UnknownCategory(t.to_string()));
        } else {
            return Err(ToolsResolveError::UnknownTool(t.to_string()));
        }
    }

    let names = dedup_preserve_order(expanded);
    for name in &names {
        if !is_known_tool(name) {
            return Err(ToolsResolveError::UnknownTool(name.clone()));
        }
    }

    let warn_shell = shell_warning(&names, tokens);
    let enabled_list = if names.is_empty() {
        "none".to_string()
    } else {
        names.join(", ")
    };

    Ok(ResolvedTools {
        allowlist: ToolAllowlist { names },
        startup: ToolsStartupLine {
            enabled_list,
            source_hint: Some(source_hint),
            warn_shell,
        },
    })
}

fn empty_resolved() -> ResolvedTools {
    ResolvedTools {
        allowlist: ToolAllowlist::default(),
        startup: ToolsStartupLine {
            enabled_list: "none".to_string(),
            source_hint: None,
            warn_shell: false,
        },
    }
}

fn expand_category(token: &str) -> Result<Option<&'static [&'static str]>, ToolsResolveError> {
    match token {
        "@read-only" => Ok(Some(&[READ_FILE])),
        "@exec" => Ok(Some(&[SHELL_EXEC])),
        "@full" => Ok(Some(&[READ_FILE, SHELL_EXEC])),
        _ if token.starts_with('@') => Err(ToolsResolveError::UnknownCategory(token.to_string())),
        _ => Ok(None),
    }
}

fn shell_warning(resolved: &[String], original_tokens: &[String]) -> bool {
    resolved.iter().any(|n| n == SHELL_EXEC)
        || original_tokens
            .iter()
            .any(|t| matches!(t.as_str(), SHELL_EXEC | "@exec" | "@full"))
}

fn dedup_preserve_order(names: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for n in names {
        if !out.iter().any(|x| x == &n) {
            out.push(n);
        }
    }
    out
}

fn split_comma_list(list: &str) -> Vec<String> {
    list.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_empty() {
        let r = resolve_tools(None, &ConfigToolsTokens::default()).unwrap();
        assert!(r.allowlist.is_empty());
        assert_eq!(r.startup.enabled_list, "none");
        assert!(!r.startup.warn_shell);
    }

    #[test]
    fn cli_overrides_config() {
        let cfg = ConfigToolsTokens(vec!["@read-only".into()]);
        let r = resolve_tools(Some("none"), &cfg).unwrap();
        assert!(r.allowlist.is_empty());
    }

    #[test]
    fn read_only_expands() {
        let r = resolve_tools(Some("@read-only"), &ConfigToolsTokens::default()).unwrap();
        assert_eq!(r.allowlist.names(), &[READ_FILE.to_string()]);
        assert!(!r.startup.warn_shell);
    }

    #[test]
    fn full_expands_order() {
        let r = resolve_tools(Some("@full"), &ConfigToolsTokens::default()).unwrap();
        assert_eq!(
            r.allowlist.names(),
            &[READ_FILE.to_string(), SHELL_EXEC.to_string()]
        );
        assert!(r.startup.warn_shell);
    }

    #[test]
    fn dedup_read_only_and_literal() {
        let r = resolve_tools(Some("@read-only,read_file"), &ConfigToolsTokens::default()).unwrap();
        assert_eq!(r.allowlist.names(), &[READ_FILE.to_string()]);
    }

    #[test]
    fn category_plus_shell_exec() {
        let r =
            resolve_tools(Some("@read-only,shell_exec"), &ConfigToolsTokens::default()).unwrap();
        assert_eq!(
            r.allowlist.names(),
            &[READ_FILE.to_string(), SHELL_EXEC.to_string()]
        );
        assert!(r.startup.warn_shell);
    }

    #[test]
    fn none_mixed_errors() {
        let err = resolve_tools(Some("none,read_file"), &ConfigToolsTokens::default()).unwrap_err();
        assert_eq!(err, ToolsResolveError::NoneMixed);
    }

    #[test]
    fn unknown_category_before_socket() {
        let err = resolve_tools(Some("@inspect"), &ConfigToolsTokens::default()).unwrap_err();
        assert!(matches!(err, ToolsResolveError::UnknownCategory(_)));
    }

    #[test]
    fn unknown_tool_before_socket() {
        let err = resolve_tools(Some("bogus"), &ConfigToolsTokens::default()).unwrap_err();
        assert!(matches!(err, ToolsResolveError::UnknownTool(_)));
    }

    #[test]
    fn config_array_no_comma_split() {
        let raw = AskToolsConfigRaw::Array(vec!["@read-only".into(), "read_file".into()]);
        let tokens = tokens_from_config_value(raw);
        let r = resolve_tools(None, &tokens).unwrap();
        assert_eq!(r.allowlist.names(), &[READ_FILE.to_string()]);
    }
}
