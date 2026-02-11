//! 履歴検索ツール（manifest + reviewed）
//!
//! manifest が無い場合は reviewed/ を走査して動作する。

use crate::domain::{parse_lines, ManifestRole};
use common::safe_session_path::{is_safe_reviewed_path, resolve_under_session_dir, REVIEWED_DIR};
use common::tool::{Tool, ToolContext, ToolError};
use serde_json::Value;
use std::io;

pub struct HistorySearchTool;

impl HistorySearchTool {
    pub const NAME: &'static str = "history_search";

    pub fn new() -> Self {
        Self
    }
}

impl Default for HistorySearchTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for HistorySearchTool {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn description(&self) -> &'static str {
        "Search reviewed history content using substring match."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Substring to search for" },
                "limit": { "type": "integer", "description": "Max hits to return (default 10, max 50)" },
                "role": { "type": "string", "enum": ["user", "assistant", "any"], "description": "Role filter (default any)" },
                "case_sensitive": { "type": "boolean", "description": "Case sensitive search (default false)" }
            },
            "required": ["query"]
        }))
    }

    fn call(&self, args: Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let session_dir = ctx
            .session_dir
            .clone()
            .ok_or_else(|| ToolError::ExecutionFailed("session_dir is not set".to_string()))?;
        let manifest_path = session_dir.join("manifest.jsonl");
        match std::fs::read_to_string(&manifest_path) {
            Ok(body) => self.call_with_manifest(&session_dir, &parse_lines(&body), args),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                self.call_without_manifest(&session_dir, args)
            }
            Err(e) => Err(ToolError::ExecutionFailed(format!(
                "{}: {}",
                manifest_path.display(),
                e
            ))),
        }
    }
}

impl HistorySearchTool {
    fn call_with_manifest(
        &self,
        session_dir: &std::path::Path,
        records: &[crate::domain::ManifestRecordV1],
        args: Value,
    ) -> Result<Value, ToolError> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing 'query'".to_string()))?;
        if query.is_empty() {
            return Err(ToolError::InvalidArgs("query must not be empty".to_string()));
        }
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v.min(50) as usize)
            .unwrap_or(10);
        if limit == 0 {
            return Err(ToolError::InvalidArgs("limit must be >= 1".to_string()));
        }
        let case_sensitive = args
            .get("case_sensitive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let role_filter = parse_role_filter(args.get("role").and_then(|v| v.as_str()))?;

        let mut hits = Vec::new();
        for rec in records.iter().rev() {
            let Some(msg) = rec.message() else {
                continue;
            };
            if !role_filter.matches(msg.role) {
                continue;
            }
            if !is_safe_reviewed_path(&msg.reviewed_path) {
                continue;
            }
            let reviewed_path = session_dir.join(&msg.reviewed_path);
            let Some(safe_path) = resolve_under_session_dir(session_dir, &reviewed_path) else {
                continue;
            };
            let content = match std::fs::read_to_string(&safe_path) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if let Some((start, end)) = find_match_range(&content, query, case_sensitive) {
                hits.push(serde_json::json!({
                    "id": msg.id,
                    "role": msg.role.as_str(),
                    "ts": msg.ts,
                    "snippet": snippet(&content, start, end),
                    "path": msg.reviewed_path
                }));
            }
            if hits.len() >= limit {
                break;
            }
        }
        Ok(serde_json::json!({ "hits": hits }))
    }

    fn call_without_manifest(
        &self,
        session_dir: &std::path::Path,
        args: Value,
    ) -> Result<Value, ToolError> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing 'query'".to_string()))?;
        if query.is_empty() {
            return Err(ToolError::InvalidArgs("query must not be empty".to_string()));
        }
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v.min(50) as usize)
            .unwrap_or(10);
        if limit == 0 {
            return Err(ToolError::InvalidArgs("limit must be >= 1".to_string()));
        }
        let case_sensitive = args
            .get("case_sensitive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let role_filter = parse_role_filter(args.get("role").and_then(|v| v.as_str()))?;

        let reviewed_dir = session_dir.join(REVIEWED_DIR);
        let mut entries: Vec<(String, ManifestRole, String)> = match std::fs::read_dir(&reviewed_dir) {
            Ok(rd) => rd
                .filter_map(|e| e.ok())
                .filter_map(|e| {
                    let path = e.path();
                    if !path.is_file() {
                        return None;
                    }
                    let name = path.file_name().and_then(|n| n.to_str())?;
                    let (id, role) = parse_reviewed_filename(name)?;
                    let rel_path = format!("{}/{}", REVIEWED_DIR, name);
                    Some((id, role, rel_path))
                })
                .collect(),
            Err(_) => return Ok(serde_json::json!({ "hits": [] })),
        };
        entries.sort_by(|a, b| b.0.cmp(&a.0)); // rev order (newest first)

        let mut hits = Vec::new();
        for (id, role, rel_path) in entries {
            if hits.len() >= limit {
                break;
            }
            if !role_filter.matches(role) {
                continue;
            }
            let full = session_dir.join(&rel_path);
            let Some(safe_path) = resolve_under_session_dir(session_dir, &full) else {
                continue;
            };
            let content = match std::fs::read_to_string(&safe_path) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if let Some((start, end)) = find_match_range(&content, query, case_sensitive) {
                hits.push(serde_json::json!({
                    "id": id,
                    "role": role.as_str(),
                    "ts": "",
                    "snippet": snippet(&content, start, end),
                    "path": rel_path
                }));
            }
        }
        Ok(serde_json::json!({ "hits": hits }))
    }
}

/// `reviewed_<id>_user.txt` / `reviewed_<id>_assistant.txt` から (id, role) を返す。
fn parse_reviewed_filename(name: &str) -> Option<(String, ManifestRole)> {
    let rest = name.strip_prefix("reviewed_")?;
    let (id, role) = if let Some(s) = rest.strip_suffix("_user.txt") {
        (s, ManifestRole::User)
    } else if let Some(s) = rest.strip_suffix("_assistant.txt") {
        (s, ManifestRole::Assistant)
    } else {
        return None;
    };
    if id.is_empty() || id.contains('/') || id.contains('\\') {
        return None;
    }
    Some((id.to_string(), role))
}

fn find_match_range(content: &str, query: &str, case_sensitive: bool) -> Option<(usize, usize)> {
    if case_sensitive {
        content
            .find(query)
            .map(|start| (start, start + query.len()))
    } else {
        let content_lc = content.to_lowercase();
        let query_lc = query.to_lowercase();
        content_lc
            .find(&query_lc)
            .map(|start| (start, start + query_lc.len()))
    }
}

fn snippet(content: &str, start: usize, end: usize) -> String {
    let mut s = start.saturating_sub(80);
    while s > 0 && !content.is_char_boundary(s) {
        s -= 1;
    }
    let mut e = (end + 80).min(content.len());
    while e < content.len() && !content.is_char_boundary(e) {
        e += 1;
    }
    content[s..e].to_string()
}

#[derive(Debug, Clone, Copy)]
enum RoleFilter {
    Any,
    User,
    Assistant,
}

impl RoleFilter {
    fn matches(self, role: ManifestRole) -> bool {
        match self {
            Self::Any => true,
            Self::User => role == ManifestRole::User,
            Self::Assistant => role == ManifestRole::Assistant,
        }
    }
}

fn parse_role_filter(role: Option<&str>) -> Result<RoleFilter, ToolError> {
    match role.unwrap_or("any") {
        "any" => Ok(RoleFilter::Any),
        "user" => Ok(RoleFilter::User),
        "assistant" => Ok(RoleFilter::Assistant),
        other => Err(ToolError::InvalidArgs(format!(
            "invalid role '{}': expected user|assistant|any",
            other
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepare_session() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("history_search_tool_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let reviewed_dir = dir.join("reviewed");
        std::fs::create_dir_all(&reviewed_dir).unwrap();
        std::fs::write(reviewed_dir.join("reviewed_001_user.txt"), "The quick brown fox").unwrap();
        std::fs::write(reviewed_dir.join("reviewed_002_assistant.txt"), "Jumps over lazy dog").unwrap();
        std::fs::write(dir.join("manifest.jsonl"), "\
{\"kind\":\"message\",\"v\":1,\"ts\":\"t1\",\"id\":\"001\",\"role\":\"user\",\"part_path\":\"part_001_user.txt\",\"reviewed_path\":\"reviewed/reviewed_001_user.txt\",\"decision\":\"allow\",\"bytes\":19,\"hash64\":\"aa\"}\n\
{\"kind\":\"message\",\"v\":1,\"ts\":\"t2\",\"id\":\"002\",\"role\":\"assistant\",\"part_path\":\"part_002_assistant.txt\",\"reviewed_path\":\"reviewed/reviewed_002_assistant.txt\",\"decision\":\"allow\",\"bytes\":19,\"hash64\":\"bb\"}\n").unwrap();
        dir
    }

    #[test]
    fn test_history_search_requires_session_dir() {
        let tool = HistorySearchTool::new();
        let ctx = ToolContext::new(None);
        let r = tool.call(serde_json::json!({"query":"fox"}), &ctx);
        assert!(matches!(r, Err(ToolError::ExecutionFailed(_))));
    }

    #[test]
    fn test_history_search_hits() {
        let tool = HistorySearchTool::new();
        let dir = prepare_session();
        let ctx = ToolContext::new(Some(dir.clone()));
        let r = tool
            .call(serde_json::json!({"query":"lazy","limit":10}), &ctx)
            .unwrap();
        let hits = r["hits"].as_array().unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0]["id"].as_str(), Some("002"));
        let _ = std::fs::remove_dir_all(dir);
    }

    fn prepare_session_no_manifest() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("history_search_tool_nomanifest_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let reviewed_dir = dir.join("reviewed");
        std::fs::create_dir_all(&reviewed_dir).unwrap();
        std::fs::write(reviewed_dir.join("reviewed_001_user.txt"), "The quick brown fox").unwrap();
        std::fs::write(reviewed_dir.join("reviewed_002_assistant.txt"), "Jumps over lazy dog").unwrap();
        dir
    }

    #[test]
    fn test_history_search_without_manifest() {
        let tool = HistorySearchTool::new();
        let dir = prepare_session_no_manifest();
        let ctx = ToolContext::new(Some(dir.clone()));
        let r = tool
            .call(serde_json::json!({"query":"fox","limit":10}), &ctx)
            .unwrap();
        let hits = r["hits"].as_array().unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0]["id"].as_str(), Some("001"));
        assert_eq!(hits[0]["path"].as_str(), Some("reviewed/reviewed_001_user.txt"));
        let _ = std::fs::remove_dir_all(dir);
    }
}

