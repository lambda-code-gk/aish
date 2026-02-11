//! 履歴取得ツール（manifest + reviewed）

use crate::domain::{parse_lines, ManifestRole};
use common::tool::{Tool, ToolContext, ToolError};
use serde_json::Value;

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
        let body = std::fs::read_to_string(&manifest_path)
            .map_err(|e| ToolError::ExecutionFailed(format!("{}: {}", manifest_path.display(), e)))?;
        let records = parse_lines(&body);

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
        for rec in &records {
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
            let keep_from = selected.len() - limit;
            selected = selected.split_off(keep_from);
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
                    let summary_path = session_dir.join(&comp.summary_path);
                    if let Ok(summary) = std::fs::read_to_string(&summary_path) {
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

        for msg in selected {
            let reviewed_path = session_dir.join(&msg.reviewed_path);
            let content = std::fs::read_to_string(&reviewed_path).map_err(|e| {
                ToolError::ExecutionFailed(format!("{}: {}", reviewed_path.display(), e))
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
        std::fs::write(dir.join("reviewed_001_user.txt"), "u1").unwrap();
        std::fs::write(dir.join("reviewed_002_assistant.txt"), "a2").unwrap();
        std::fs::write(dir.join("manifest.jsonl"), "\
{\"kind\":\"message\",\"v\":1,\"ts\":\"t1\",\"id\":\"001\",\"role\":\"user\",\"part_path\":\"part_001_user.txt\",\"reviewed_path\":\"reviewed_001_user.txt\",\"decision\":\"allow\",\"bytes\":2,\"hash64\":\"aa\"}\n\
{\"kind\":\"message\",\"v\":1,\"ts\":\"t2\",\"id\":\"002\",\"role\":\"assistant\",\"part_path\":\"part_002_assistant.txt\",\"reviewed_path\":\"reviewed_002_assistant.txt\",\"decision\":\"allow\",\"bytes\":2,\"hash64\":\"bb\"}\n").unwrap();
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
}

