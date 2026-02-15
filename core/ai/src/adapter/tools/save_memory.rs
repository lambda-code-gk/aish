//! save_memory ツール: 永続メモリに 1 件保存

use crate::adapter::memory_storage;
use crate::domain::MemoryEntry;
use common::tool::{Tool, ToolContext, ToolError};
use serde_json::Value;
use std::path::PathBuf;

pub struct SaveMemoryTool;

impl SaveMemoryTool {
    pub const NAME: &'static str = "save_memory";

    pub fn new() -> Self {
        Self
    }
}

impl Default for SaveMemoryTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for SaveMemoryTool {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn description(&self) -> &'static str {
        "Save useful information to the memory system. The memory will be stored in the project-specific directory if .aish/memory exists, otherwise in the global directory."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "content": { "type": "string", "description": "The content to remember" },
                "category": { "type": "string", "description": "Category: code_pattern, error_solution, workflow, best_practice, configuration, etc.", "default": "general" },
                "keywords": { "type": "array", "items": { "type": "string" }, "description": "Keywords for searching this memory later" },
                "subject": { "type": "string", "description": "A brief subject or title describing what this memory is about" }
            },
            "required": ["content"]
        }))
    }

    fn call(&self, args: Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let dir = resolve_memory_dir_for_save(ctx)?;
        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing 'content'".to_string()))?
            .to_string();
        if content.trim().is_empty() {
            return Err(ToolError::InvalidArgs("content is required".to_string()));
        }
        let category = args
            .get("category")
            .and_then(|v| v.as_str())
            .unwrap_or("general")
            .to_string();
        let keywords: Vec<String> = args
            .get("keywords")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let subject = args
            .get("subject")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let timestamp = common::ports::outbound::now_iso8601();
        let entry = MemoryEntry::new("", content, category, keywords, subject, timestamp);

        let id = memory_storage::save_entry(
            dir.as_path(),
            &entry,
            ctx.log.as_deref(),
        )
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(serde_json::json!({
            "memory_id": id,
            "memory_dir": dir.to_string_lossy()
        }))
    }
}

fn resolve_memory_dir_for_save(ctx: &ToolContext) -> Result<PathBuf, ToolError> {
    if let Some(ref p) = ctx.memory_dir_project {
        return Ok(p.clone());
    }
    if let Some(ref p) = ctx.memory_dir_global {
        return Ok(p.clone());
    }
    Err(ToolError::ExecutionFailed(
        "memory is not configured (no memory_dir_project or memory_dir_global)".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::tool::Tool;

    #[test]
    fn test_save_memory_requires_memory_dir() {
        let tool = SaveMemoryTool::new();
        let ctx = ToolContext::new(None);
        let r = tool.call(
            serde_json::json!({"content": "hello", "subject": "test"}),
            &ctx,
        );
        assert!(matches!(r, Err(ToolError::ExecutionFailed(_))));
    }

    #[test]
    fn test_save_memory_requires_content() {
        let dir = std::env::temp_dir().join("save_memory_test");
        let _ = std::fs::create_dir_all(&dir);
        let tool = SaveMemoryTool::new();
        let ctx = ToolContext::new(None).with_memory_dirs(None, Some(dir.clone()));
        let r = tool.call(serde_json::json!({}), &ctx);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(matches!(r, Err(ToolError::InvalidArgs(_))));
    }

    #[test]
    fn test_save_memory_success() {
        let dir = std::env::temp_dir().join("save_memory_success_test");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let tool = SaveMemoryTool::new();
        let ctx = ToolContext::new(None).with_memory_dirs(None, Some(dir.clone()));
        let r = tool.call(
            serde_json::json!({
                "content": "To fix permission denied, use chmod +x script.",
                "category": "error_solution",
                "keywords": ["chmod", "permission"],
                "subject": "script permission"
            }),
            &ctx,
        );
        let _ = std::fs::remove_dir_all(&dir);
        assert!(r.is_ok());
        let out = r.unwrap();
        assert!(out.get("memory_id").and_then(|v| v.as_str()).unwrap().len() >= 8);
        assert_eq!(out.get("memory_dir").and_then(|v| v.as_str()).unwrap(), dir.to_string_lossy().as_ref());
    }
}
