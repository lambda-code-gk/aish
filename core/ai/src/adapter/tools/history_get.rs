//! 履歴取得ツール（manifest + reviewed）
//!
//! manifest が無い場合は reviewed/ を走査して動作する。

use crate::domain::{parse_lines, ManifestRole};
use common::safe_session_path::{
    is_safe_reviewed_path, is_safe_summary_basename, resolve_under_session_dir, REVIEWED_DIR,
};
use common::tool::{Tool, ToolContext, ToolError};
use serde_json::Value;
use std::io;

pub struct HistoryGetTool;

impl HistoryGetTool {
    pub const NAME: &'static str = "history_get";

    pub fn new() -> Self {
        Self
    }
}

impl Default for HistoryGetTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for HistoryGetTool {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn description(&self) -> &'static str {
        "Get reviewed history messages from manifest.jsonl. Supports pagination by id and role filtering."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "limit": { "type": "integer", "description": "Number of messages to return (default 20, max 200)" },
                "before_id": { "type": "string", "description": "Return only messages with id < before_id" },
                "after_id": { "type": "string", "description": "Return only messages with id > after_id" },
                "role": { "type": "string", "enum": ["user", "assistant", "any"], "description": "Role filter (default any)" },
                "include_compaction": { "type": "boolean", "description": "Prepend latest compaction summary if available (default true)" }
            }
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

impl HistoryGetTool {
    fn call_with_manifest(
        &self,
        session_dir: &std::path::Path,
        records: &[crate::domain::ManifestRecordV1],
        args: Value,
    ) -> Result<Value, ToolError> {
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v.min(200) as usize)
            .unwrap_or(20);
        if limit == 0 {
            return Err(ToolError::InvalidArgs("limit must be >= 1".to_string()));
        }
        let before_id = args.get("before_id").and_then(|v| v.as_str());
        let after_id = args.get("after_id").and_then(|v| v.as_str());
        if before_id.is_some() && after_id.is_some() {
            return Err(ToolError::InvalidArgs(
                "specify either before_id or after_id".to_string(),
            ));
        }
        let include_compaction = args
            .get("include_compaction")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let role_filter = parse_role_filter(args.get("role").and_then(|v| v.as_str()))?;

        let mut selected = Vec::new();
        for rec in records.iter() {
            let Some(msg) = rec.message() else {
                continue;
            };
            if !role_filter.matches(msg.role) {
                continue;
            }
            if let Some(b) = before_id {
                if msg.id.as_str() >= b {
                    continue;
                }
            }
            if let Some(a) = after_id {
                if msg.id.as_str() <= a {
                    continue;
                }
            }
            selected.push(msg);
        }

        if selected.len() > limit {
            if after_id.is_some() {
                selected.truncate(limit);
            } else {
                let keep_from = selected.len() - limit;
                selected = selected.split_off(keep_from);
            }
        }

        let mut out = Vec::new();
        if include_compaction {
            if let Some(first) = selected.first() {
                if let Some(comp) = records
                    .iter()
                    .filter_map(|r| r.compaction())
                    .filter(|c| c.to_id.as_str() < first.id.as_str())
                    .last()
                {
                    if is_safe_summary_basename(&comp.summary_path) {
                        let summary_path = session_dir.join(&comp.summary_path);
                        if let Some(safe_path) = resolve_under_session_dir(&session_dir, &summary_path) {
                            if let Ok(summary) = std::fs::read_to_string(&safe_path) {
                                out.push(serde_json::json!({
                                    "id": format!("compaction:{}:{}", comp.from_id, comp.to_id),
                                    "role": "assistant",
                                    "ts": comp.ts,
                                    "content": summary
                                }));
                            }
                        }
                    }
                }
            }
        }

        for msg in selected {
            if !is_safe_reviewed_path(&msg.reviewed_path) {
                return Err(ToolError::ExecutionFailed(format!(
                    "invalid or unsafe reviewed_path: {}",
                    msg.reviewed_path
                )));
            }
            let reviewed_path = session_dir.join(&msg.reviewed_path);
            let safe_path = resolve_under_session_dir(&session_dir, &reviewed_path).ok_or_else(|| {
                ToolError::ExecutionFailed(format!(
                    "reviewed_path not under session dir: {}",
                    msg.reviewed_path
                ))
            })?;
            let content = std::fs::read_to_string(&safe_path).map_err(|e| {
                ToolError::ExecutionFailed(format!("{}: {}", safe_path.display(), e))
            })?;
            out.push(serde_json::json!({
                "id": msg.id,
                "role": msg.role.as_str(),
                "ts": msg.ts,
                "content": content
            }));
        }

        Ok(serde_json::json!({ "messages": out }))
    }

    fn call_without_manifest(
        &self,
        session_dir: &std::path::Path,
        args: Value,
    ) -> Result<Value, ToolError> {
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v.min(200) as usize)
            .unwrap_or(20);
        if limit == 0 {
            return Err(ToolError::InvalidArgs("limit must be >= 1".to_string()));
        }
        let before_id = args.get("before_id").and_then(|v| v.as_str());
        let after_id = args.get("after_id").and_then(|v| v.as_str());
        if before_id.is_some() && after_id.is_some() {
            return Err(ToolError::InvalidArgs(
                "specify either before_id or after_id".to_string(),
            ));
        }
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
            Err(_) => return Ok(serde_json::json!({ "messages": [] })),
        };
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        let mut selected: Vec<_> = entries
            .into_iter()
            .filter(|(id, role, _)| {
                role_filter.matches(*role)
                    && before_id.map_or(true, |b| id.as_str() < b)
                    && after_id.map_or(true, |a| id.as_str() > a)
            })
            .collect();
        if selected.len() > limit {
            if after_id.is_some() {
                selected.truncate(limit);
            } else {
                let keep_from = selected.len() - limit;
                selected = selected.split_off(keep_from);
            }
        }

        let mut out = Vec::new();
        for (id, role, rel_path) in selected {
            let full = session_dir.join(&rel_path);
            let Some(safe_path) = resolve_under_session_dir(session_dir, &full) else {
                continue;
            };
            if let Ok(content) = std::fs::read_to_string(&safe_path) {
                out.push(serde_json::json!({
                    "id": id,
                    "role": role.as_str(),
                    "ts": "",
                    "content": content
                }));
            }
        }
        Ok(serde_json::json!({ "messages": out }))
    }
}

/// `reviewed_<id>_user.txt` / `reviewed_<id>_assistant.txt` から (id, role) を返す。
fn parse_reviewed_filename(name: &str) -> Option<(String, ManifestRole)> {
    let rest = name.strip_prefix("reviewed_")?;
    let (id, role_suffix) = if let Some(s) = rest.strip_suffix("_user.txt") {
        (s, ManifestRole::User)
    } else if let Some(s) = rest.strip_suffix("_assistant.txt") {
        (s, ManifestRole::Assistant)
    } else {
        return None;
    };
    if id.is_empty() || id.contains('/') || id.contains('\\') {
        return None;
    }
    Some((id.to_string(), role_suffix))
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
        let dir = std::env::temp_dir().join(format!("history_get_tool_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let reviewed_dir = dir.join("reviewed");
        std::fs::create_dir_all(&reviewed_dir).unwrap();
        std::fs::write(reviewed_dir.join("reviewed_001_user.txt"), "u1").unwrap();
        std::fs::write(reviewed_dir.join("reviewed_002_assistant.txt"), "a2").unwrap();
        std::fs::write(dir.join("manifest.jsonl"), "\
{\"kind\":\"message\",\"v\":1,\"ts\":\"t1\",\"id\":\"001\",\"role\":\"user\",\"part_path\":\"part_001_user.txt\",\"reviewed_path\":\"reviewed/reviewed_001_user.txt\",\"decision\":\"allow\",\"bytes\":2,\"hash64\":\"aa\"}\n\
{\"kind\":\"message\",\"v\":1,\"ts\":\"t2\",\"id\":\"002\",\"role\":\"assistant\",\"part_path\":\"part_002_assistant.txt\",\"reviewed_path\":\"reviewed/reviewed_002_assistant.txt\",\"decision\":\"allow\",\"bytes\":2,\"hash64\":\"bb\"}\n").unwrap();
        dir
    }

    #[test]
    fn test_history_get_requires_session_dir() {
        let tool = HistoryGetTool::new();
        let ctx = ToolContext::new(None);
        let r = tool.call(serde_json::json!({}), &ctx);
        assert!(matches!(r, Err(ToolError::ExecutionFailed(_))));
    }

    #[test]
    fn test_history_get_basic() {
        let tool = HistoryGetTool::new();
        let dir = prepare_session();
        let ctx = ToolContext::new(Some(dir.clone()));
        let r = tool
            .call(serde_json::json!({"limit": 2, "role": "any"}), &ctx)
            .unwrap();
        let msgs = r["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["id"].as_str(), Some("001"));
        assert_eq!(msgs[1]["id"].as_str(), Some("002"));
        let _ = std::fs::remove_dir_all(dir);
    }

    fn prepare_session_no_manifest() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("history_get_tool_nomanifest_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let reviewed_dir = dir.join("reviewed");
        std::fs::create_dir_all(&reviewed_dir).unwrap();
        std::fs::write(reviewed_dir.join("reviewed_001_user.txt"), "u1").unwrap();
        std::fs::write(reviewed_dir.join("reviewed_002_assistant.txt"), "a2").unwrap();
        dir
    }

    #[test]
    fn test_history_get_without_manifest() {
        let tool = HistoryGetTool::new();
        let dir = prepare_session_no_manifest();
        let ctx = ToolContext::new(Some(dir.clone()));
        let r = tool
            .call(serde_json::json!({"limit": 2, "role": "any"}), &ctx)
            .unwrap();
        let msgs = r["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["id"].as_str(), Some("001"));
        assert_eq!(msgs[0]["ts"].as_str(), Some(""));
        assert_eq!(msgs[1]["id"].as_str(), Some("002"));
        let _ = std::fs::remove_dir_all(dir);
    }

    fn prepare_session_four_messages(suffix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("history_get_tool_4_{}_{}", std::process::id(), suffix));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let reviewed_dir = dir.join("reviewed");
        std::fs::create_dir_all(&reviewed_dir).unwrap();
        for (id, role, content) in [
            ("001", "user", "u1"),
            ("002", "assistant", "a2"),
            ("003", "user", "u3"),
            ("004", "assistant", "a4"),
        ] {
            let f = format!("reviewed_{}_{}.txt", id, role);
            std::fs::write(reviewed_dir.join(&f), content).unwrap();
        }
        let manifest = "\
{\"kind\":\"message\",\"v\":1,\"ts\":\"t1\",\"id\":\"001\",\"role\":\"user\",\"part_path\":\"part_001_user.txt\",\"reviewed_path\":\"reviewed/reviewed_001_user.txt\",\"decision\":\"allow\",\"bytes\":2,\"hash64\":\"a\"}\n\
{\"kind\":\"message\",\"v\":1,\"ts\":\"t2\",\"id\":\"002\",\"role\":\"assistant\",\"part_path\":\"part_002_assistant.txt\",\"reviewed_path\":\"reviewed/reviewed_002_assistant.txt\",\"decision\":\"allow\",\"bytes\":2,\"hash64\":\"b\"}\n\
{\"kind\":\"message\",\"v\":1,\"ts\":\"t3\",\"id\":\"003\",\"role\":\"user\",\"part_path\":\"part_003_user.txt\",\"reviewed_path\":\"reviewed/reviewed_003_user.txt\",\"decision\":\"allow\",\"bytes\":2,\"hash64\":\"c\"}\n\
{\"kind\":\"message\",\"v\":1,\"ts\":\"t4\",\"id\":\"004\",\"role\":\"assistant\",\"part_path\":\"part_004_assistant.txt\",\"reviewed_path\":\"reviewed/reviewed_004_assistant.txt\",\"decision\":\"allow\",\"bytes\":2,\"hash64\":\"d\"}\n";
        std::fs::write(dir.join("manifest.jsonl"), manifest).unwrap();
        dir
    }

    #[test]
    fn test_history_get_after_id_returns_first_limit() {
        let tool = HistoryGetTool::new();
        let dir = prepare_session_four_messages("after");
        let ctx = ToolContext::new(Some(dir.clone()));
        let r = tool
            .call(serde_json::json!({
                "after_id": "001",
                "limit": 2,
                "role": "any"
            }), &ctx)
            .unwrap();
        let msgs = r["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2, "after_id should return first 2 after 001");
        assert_eq!(msgs[0]["id"].as_str(), Some("002"));
        assert_eq!(msgs[1]["id"].as_str(), Some("003"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_history_get_before_id_returns_last_limit() {
        let tool = HistoryGetTool::new();
        let dir = prepare_session_four_messages("before");
        let ctx = ToolContext::new(Some(dir.clone()));
        let r = tool
            .call(serde_json::json!({
                "before_id": "004",
                "limit": 2,
                "role": "any"
            }), &ctx)
            .unwrap();
        let msgs = r["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2, "before_id should return last 2 before 004");
        assert_eq!(msgs[0]["id"].as_str(), Some("002"));
        assert_eq!(msgs[1]["id"].as_str(), Some("003"));
        let _ = std::fs::remove_dir_all(dir);
    }
}

