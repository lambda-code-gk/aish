//! get_memory_content ツール: ID で永続メモリ 1 件を取得

use crate::adapter::memory_storage;
use common::tool::{Tool, ToolContext, ToolError};
use serde_json::Value;

pub struct GetMemoryContentTool;

impl GetMemoryContentTool {
    pub const NAME: &'static str = "get_memory_content";

    pub fn new() -> Self {
        Self
    }
}

impl Default for GetMemoryContentTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for GetMemoryContentTool {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn description(&self) -> &'static str {
        "Get the full content of a specific memory by its ID. Use this after search_memory returns memory IDs to retrieve detailed information when needed."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "memory_id": { "type": "string", "description": "The memory ID returned by search_memory" }
            },
            "required": ["memory_id"]
        }))
    }

    fn call(&self, args: Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let memory_id = args
            .get("memory_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing 'memory_id'".to_string()))?
            .to_string();
        if memory_id.is_empty() {
            return Err(ToolError::InvalidArgs("memory_id is required".to_string()));
        }

        let project_dir = ctx.memory_dir_project.clone();
        let global_dir = ctx
            .memory_dir_global
            .clone()
            .ok_or_else(|| ToolError::ExecutionFailed("memory is not configured (memory_dir_global)".to_string()))?;

        let entry = memory_storage::get_entry_by_id(
            project_dir.as_deref(),
            &global_dir,
            &memory_id,
            ctx.log.as_deref(),
        )
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        Ok(serde_json::json!({
            "id": entry.id,
            "content": entry.content,
            "category": entry.category,
            "keywords": entry.keywords,
            "subject": entry.subject,
            "timestamp": entry.timestamp,
            "source": entry.source
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::tool::Tool;

    #[test]
    fn test_get_memory_content_requires_memory_id() {
        let dir = std::env::temp_dir().join("get_memory_id_test");
        let _ = std::fs::create_dir_all(&dir);
        let tool = GetMemoryContentTool::new();
        let ctx = ToolContext::new(None).with_memory_dirs(None, Some(dir));
        let r = tool.call(serde_json::json!({}), &ctx);
        assert!(matches!(r, Err(ToolError::InvalidArgs(_))));
    }

    #[test]
    fn test_get_memory_content_requires_memory_dir() {
        let tool = GetMemoryContentTool::new();
        let ctx = ToolContext::new(None);
        let r = tool.call(serde_json::json!({"memory_id": "abc123"}), &ctx);
        assert!(matches!(r, Err(ToolError::ExecutionFailed(_))));
    }
}
