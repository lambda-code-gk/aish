//! ファイル上書きツール（adapter 層）
//!
//! 指定パスにテキストを上書きする。OS 副作用を伴うため adapter に配置。

use common::tool::{Tool, ToolContext, ToolError};
use serde_json::Value;
use std::path::Path;

/// ファイル上書きツール（API 名 "write_file"）
pub struct WriteFileTool;

impl WriteFileTool {
    pub const NAME: &'static str = "write_file";

    pub fn new() -> Self {
        Self
    }
}

impl Default for WriteFileTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for WriteFileTool {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn description(&self) -> &'static str {
        "Overwrite a file with the given content. Use when you need to create or fully replace a file. Pass 'path' and 'content'."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to the file to write" },
                "content": { "type": "string", "description": "Content to write (overwrites entire file)" }
            },
            "required": ["path", "content"]
        }))
    }

    fn call(&self, args: Value, _ctx: &ToolContext) -> Result<Value, ToolError> {
        let path_str = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing 'path'".to_string()))?
            .to_string();

        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing 'content'".to_string()))?
            .to_string();

        if path_str.trim().is_empty() {
            return Err(ToolError::InvalidArgs("path must not be empty".to_string()));
        }

        let path = Path::new(&path_str);
        std::fs::write(path, &content)
            .map_err(|e| ToolError::ExecutionFailed(format!("{}: {}", path_str, e)))?;

        Ok(serde_json::json!({
            "path": path_str,
            "written": content.len()
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_file_basic() {
        let tool = WriteFileTool::new();
        let ctx = ToolContext::new(None);

        let dir = std::env::temp_dir().join("write_file_tool_test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("out.txt");
        let path_str = file.to_string_lossy().to_string();

        let r = tool
            .call(
                serde_json::json!({ "path": path_str, "content": "line1\nline2\n" }),
                &ctx,
            )
            .unwrap();
        assert_eq!(r["written"].as_u64(), Some(12));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "line1\nline2\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_write_file_overwrite() {
        let tool = WriteFileTool::new();
        let ctx = ToolContext::new(None);

        let dir = std::env::temp_dir().join("write_file_overwrite_test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("overwrite.txt");
        std::fs::write(&file, "old").unwrap();
        let path_str = file.to_string_lossy().to_string();

        tool.call(
            serde_json::json!({ "path": path_str, "content": "new content" }),
            &ctx,
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "new content");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_write_file_missing_args() {
        let tool = WriteFileTool::new();
        let ctx = ToolContext::new(None);
        assert!(matches!(
            tool.call(serde_json::json!({}), &ctx),
            Err(ToolError::InvalidArgs(_))
        ));
        assert!(matches!(
            tool.call(serde_json::json!({ "path": "x" }), &ctx),
            Err(ToolError::InvalidArgs(_))
        ));
    }
}
