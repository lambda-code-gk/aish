//! ファイル読み取りツール（adapter 層）
//!
//! 指定パスのファイルをテキストとして読み取る。OS 副作用を伴うため adapter に配置。

use common::tool::{Tool, ToolContext, ToolError};
use serde_json::Value;
use std::path::Path;

/// ファイル読み取りツール（API 名 "read_file"）
pub struct ReadFileTool;

impl ReadFileTool {
    pub const NAME: &'static str = "read_file";

    pub fn new() -> Self {
        Self
    }
}

impl Default for ReadFileTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for ReadFileTool {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn description(&self) -> &'static str {
        "Read the contents of a file as text. Use when you need to inspect a file. Pass 'path' (file path, relative to current working directory)."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to the file to read (relative or absolute)" }
            },
            "required": ["path"]
        }))
    }

    fn call(&self, args: Value, _ctx: &ToolContext) -> Result<Value, ToolError> {
        let path_str = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing 'path'".to_string()))?
            .to_string();

        if path_str.trim().is_empty() {
            return Err(ToolError::InvalidArgs("path must not be empty".to_string()));
        }

        let path = Path::new(&path_str);
        let content = std::fs::read_to_string(path)
            .map_err(|e| ToolError::ExecutionFailed(format!("{}: {}", path_str, e)))?;

        Ok(serde_json::json!({
            "content": content,
            "path": path_str
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_file_basic() {
        let tool = ReadFileTool::new();
        let ctx = ToolContext::new(None);

        let dir = std::env::temp_dir().join("read_file_tool_test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("hello.txt");
        std::fs::write(&file, "hello world\n").unwrap();
        let path_str = file.to_string_lossy().to_string();

        let r = tool
            .call(serde_json::json!({ "path": path_str }), &ctx)
            .unwrap();
        assert_eq!(r["content"].as_str(), Some("hello world\n"));
        assert_eq!(r["path"].as_str(), Some(path_str.as_str()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_read_file_missing_path() {
        let tool = ReadFileTool::new();
        let ctx = ToolContext::new(None);
        let r = tool.call(serde_json::json!({}), &ctx);
        assert!(matches!(r, Err(ToolError::InvalidArgs(_))));
    }

    #[test]
    fn test_read_file_empty_path() {
        let tool = ReadFileTool::new();
        let ctx = ToolContext::new(None);
        let r = tool.call(serde_json::json!({ "path": "" }), &ctx);
        assert!(matches!(r, Err(ToolError::InvalidArgs(_))));
    }

    #[test]
    fn test_read_file_not_found() {
        let tool = ReadFileTool::new();
        let ctx = ToolContext::new(None);
        let r = tool.call(
            serde_json::json!({ "path": "/nonexistent/file_xyz_12345" }),
            &ctx,
        );
        assert!(matches!(r, Err(ToolError::ExecutionFailed(_))));
    }
}
