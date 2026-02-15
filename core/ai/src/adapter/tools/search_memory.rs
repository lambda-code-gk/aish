//! search_memory ツール: 永続メモリを検索

use crate::adapter::memory_storage;
use common::tool::{Tool, ToolContext, ToolError};
use serde_json::Value;

pub struct SearchMemoryTool;

impl SearchMemoryTool {
    pub const NAME: &'static str = "search_memory";

    pub fn new() -> Self {
        Self
    }
}

impl Default for SearchMemoryTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for SearchMemoryTool {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn description(&self) -> &'static str {
        "Search memories related to the query. Returns memory metadata (id, category, keywords, score) and content. Use get_memory_content to retrieve full content of a specific memory by ID if needed."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query" },
                "category": { "type": "string", "description": "Filter by category (optional)" },
                "limit": { "type": "integer", "description": "Maximum number of results", "default": 5 }
            },
            "required": ["query"]
        }))
    }

    fn call(&self, args: Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let (project_dir, global_dir) = resolve_memory_dirs(ctx)?;
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let category = args.get("category").and_then(|v| v.as_str()).map(String::from);
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v.min(50) as usize)
            .unwrap_or(5);

        let entries = memory_storage::search_entries(
            project_dir.as_deref(),
            global_dir.as_path(),
            &query,
            category.as_deref(),
            limit,
            true,
        )
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let arr: Vec<Value> = entries
            .into_iter()
            .map(|e| {
                serde_json::json!({
                    "id": e.id,
                    "category": e.category,
                    "keywords": e.keywords,
                    "subject": e.subject,
                    "score": e.score,
                    "content": e.content,
                    "source": e.source
                })
            })
            .collect();
        Ok(serde_json::json!(arr))
    }
}

fn resolve_memory_dirs(ctx: &ToolContext) -> Result<(Option<std::path::PathBuf>, std::path::PathBuf), ToolError> {
    let global = ctx
        .memory_dir_global
        .clone()
        .ok_or_else(|| ToolError::ExecutionFailed("memory is not configured (memory_dir_global)".to_string()))?;
    Ok((ctx.memory_dir_project.clone(), global))
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::tool::Tool;

    #[test]
    fn test_search_memory_requires_memory_dir() {
        let tool = SearchMemoryTool::new();
        let ctx = ToolContext::new(None);
        let r = tool.call(serde_json::json!({"query": "foo"}), &ctx);
        assert!(matches!(r, Err(ToolError::ExecutionFailed(_))));
    }

    #[test]
    fn test_search_memory_returns_empty_when_no_entries() {
        let dir = std::env::temp_dir().join("search_memory_empty_test");
        let _ = std::fs::create_dir_all(&dir);
        let tool = SearchMemoryTool::new();
        let ctx = ToolContext::new(None).with_memory_dirs(None, Some(dir.clone()));
        let r = tool.call(serde_json::json!({"query": "anything"}), &ctx);
        let _ = std::fs::remove_dir_all(&dir);
        let out = r.unwrap();
        let arr = out.as_array().unwrap();
        assert!(arr.is_empty());
    }
}
