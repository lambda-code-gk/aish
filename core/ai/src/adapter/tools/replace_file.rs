//! ファイルブロック置換ツール（adapter 層）
//!
//! ファイル内の「old_block に一致するブロック」を new_block に置換する。
//! old_block はファイル内にちょうど1回だけ出現している必要がある（行番号不要）。
//! 参考: old_impl/_aish/lib/tool_replace_block.sh

use common::tool::{Tool, ToolContext, ToolError};
use serde_json::Value;
use std::path::Path;

/// ファイルブロック置換ツール（API 名 "replace_file"）
pub struct ReplaceFileTool;

impl ReplaceFileTool {
    pub const NAME: &'static str = "replace_file";

    pub fn new() -> Self {
        Self
    }
}

impl Default for ReplaceFileTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for ReplaceFileTool {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn description(&self) -> &'static str {
        "Replace a specific block of text in a file. The old_block must match exactly one location in the file; include enough context in old_block to make it unique. Pass path, old_block (exact text to find), and new_block (replacement)."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to the file to modify" },
                "old_block": { "type": "string", "description": "The exact block of text to be replaced (must appear exactly once in the file)" },
                "new_block": { "type": "string", "description": "The new block of text to replace it with" }
            },
            "required": ["path", "old_block", "new_block"]
        }))
    }

    fn call(&self, args: Value, _ctx: &ToolContext) -> Result<Value, ToolError> {
        let path_str = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing 'path'".to_string()))?
            .to_string();

        let old_block = args
            .get("old_block")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing 'old_block'".to_string()))?
            .to_string();

        let new_block = args
            .get("new_block")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();

        if path_str.trim().is_empty() {
            return Err(ToolError::InvalidArgs("path must not be empty".to_string()));
        }
        if old_block.is_empty() {
            return Err(ToolError::InvalidArgs(
                "old_block must not be empty (provide enough context to match exactly one place)"
                    .to_string(),
            ));
        }

        let path = Path::new(&path_str);
        let content = std::fs::read_to_string(path)
            .map_err(|e| ToolError::ExecutionFailed(format!("read {}: {}", path_str, e)))?;

        let count = content.matches(&old_block).count();
        if count == 0 {
            return Err(ToolError::ExecutionFailed(format!(
                "old_block not found in {}",
                path_str
            )));
        }
        if count > 1 {
            return Err(ToolError::ExecutionFailed(format!(
                "old_block found {} times in {}. Please provide more context in old_block so it matches exactly one location.",
                count, path_str
            )));
        }

        let new_content = content.replacen(&old_block, &new_block, 1);
        std::fs::write(path, &new_content)
            .map_err(|e| ToolError::ExecutionFailed(format!("write {}: {}", path_str, e)))?;

        Ok(serde_json::json!({
            "path": path_str,
            "success": true
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replace_file_block_middle() {
        let tool = ReplaceFileTool::new();
        let ctx = ToolContext::new(None);

        let dir = std::env::temp_dir().join("replace_file_tool_test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("edit.txt");
        std::fs::write(&file, "line1\nline2\nline3\nline4\nline5\n").unwrap();
        let path_str = file.to_string_lossy().to_string();

        let r = tool
            .call(
                serde_json::json!({
                    "path": path_str,
                    "old_block": "line2\nline3\nline4",
                    "new_block": "replaced"
                }),
                &ctx,
            )
            .unwrap();
        assert_eq!(r["success"].as_bool(), Some(true));
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "line1\nreplaced\nline5\n"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_replace_file_block_single_line() {
        let tool = ReplaceFileTool::new();
        let ctx = ToolContext::new(None);

        let dir = std::env::temp_dir().join("replace_file_single_test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("single.txt");
        std::fs::write(&file, "a\nb\nc\n").unwrap();
        let path_str = file.to_string_lossy().to_string();

        tool.call(
            serde_json::json!({
                "path": path_str,
                "old_block": "b",
                "new_block": "B"
            }),
            &ctx,
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "a\nB\nc\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_replace_file_old_block_not_found() {
        let tool = ReplaceFileTool::new();
        let ctx = ToolContext::new(None);

        let dir = std::env::temp_dir().join("replace_file_notfound_test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("x.txt");
        std::fs::write(&file, "hello\n").unwrap();
        let path_str = file.to_string_lossy().to_string();

        let r = tool.call(
            serde_json::json!({
                "path": path_str,
                "old_block": "nonexistent",
                "new_block": "y"
            }),
            &ctx,
        );
        assert!(matches!(r, Err(ToolError::ExecutionFailed(_))));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_replace_file_old_block_multiple_matches() {
        let tool = ReplaceFileTool::new();
        let ctx = ToolContext::new(None);

        let dir = std::env::temp_dir().join("replace_file_multi_test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("multi.txt");
        std::fs::write(&file, "foo\nfoo\n").unwrap();
        let path_str = file.to_string_lossy().to_string();

        let r = tool.call(
            serde_json::json!({
                "path": path_str,
                "old_block": "foo",
                "new_block": "bar"
            }),
            &ctx,
        );
        assert!(matches!(r, Err(ToolError::ExecutionFailed(_))));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "foo\nfoo\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_replace_file_missing_old_block() {
        let tool = ReplaceFileTool::new();
        let ctx = ToolContext::new(None);
        let r = tool.call(
            serde_json::json!({
                "path": "/tmp/x",
                "new_block": "y"
            }),
            &ctx,
        );
        assert!(matches!(r, Err(ToolError::InvalidArgs(_))));
    }

    #[test]
    fn test_replace_file_empty_old_block() {
        let tool = ReplaceFileTool::new();
        let ctx = ToolContext::new(None);
        let r = tool.call(
            serde_json::json!({
                "path": "/tmp/x",
                "old_block": "",
                "new_block": "y"
            }),
            &ctx,
        );
        assert!(matches!(r, Err(ToolError::InvalidArgs(_))));
    }

    #[test]
    fn test_replace_file_new_block_can_be_empty() {
        let tool = ReplaceFileTool::new();
        let ctx = ToolContext::new(None);

        let dir = std::env::temp_dir().join("replace_file_empty_new_test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("del.txt");
        std::fs::write(&file, "a\nremove_me\nb\n").unwrap();
        let path_str = file.to_string_lossy().to_string();

        tool.call(
            serde_json::json!({
                "path": path_str,
                "old_block": "remove_me\n",
                "new_block": ""
            }),
            &ctx,
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "a\nb\n");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
