//! grep ツール（adapter 層）
//!
//! ripgrep (rg) または grep でパターン検索する。OS 副作用を伴うため adapter に配置。

use common::tool::{Tool, ToolContext, ToolError};
use serde_json::Value;
use std::process::Command;

/// grep ツール（API 名 "grep"）
/// rg が利用可能なら rg、なければ grep を使用する。
pub struct GrepTool;

impl GrepTool {
    pub const NAME: &'static str = "grep";

    pub fn new() -> Self {
        Self
    }

    fn find_grep_cmd() -> (String, Vec<String>) {
        if Command::new("rg").arg("--version").output().is_ok() {
            (
                "rg".to_string(),
                vec![
                    "--line-number".to_string(),
                    "--no-heading".to_string(),
                    "--color".to_string(),
                    "never".to_string(),
                ],
            )
        } else {
            ("grep".to_string(), vec!["-n".to_string()])
        }
    }
}

impl Default for GrepTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for GrepTool {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn description(&self) -> &'static str {
        "Search for a pattern in files using grep (or ripgrep if available). Pass 'pattern' (required), and optionally 'path' (file or directory, default '.'), 'case_insensitive' (boolean)."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Search pattern (regex)" },
                "path": { "type": "string", "description": "File or directory to search (default: .)" },
                "case_insensitive": { "type": "boolean", "description": "Case-insensitive match (default: false)" }
            },
            "required": ["pattern"]
        }))
    }

    fn call(&self, args: Value, _ctx: &ToolContext) -> Result<Value, ToolError> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing 'pattern'".to_string()))?
            .to_string();

        if pattern.trim().is_empty() {
            return Err(ToolError::InvalidArgs("pattern must not be empty".to_string()));
        }

        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or(".")
            .to_string();

        let case_insensitive = args
            .get("case_insensitive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let (bin, mut base_args) = Self::find_grep_cmd();

        if case_insensitive {
            if bin == "rg" {
                base_args.push("--ignore-case".to_string());
            } else {
                base_args.push("-i".to_string());
            }
        }

        let mut cmd = Command::new(&bin);
        cmd.args(&base_args).arg(&pattern);
        if !path.is_empty() {
            cmd.arg(&path);
        }

        let output = cmd
            .output()
            .map_err(|e| ToolError::ExecutionFailed(format!("{}: {}", bin, e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(1);

        Ok(serde_json::json!({
            "stdout": stdout,
            "stderr": stderr,
            "exit_code": exit_code,
            "command": bin
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grep_name() {
        let tool = GrepTool::new();
        assert_eq!(tool.name(), GrepTool::NAME);
    }

    #[test]
    fn test_grep_missing_pattern() {
        let tool = GrepTool::new();
        let ctx = ToolContext::new(None);
        let r = tool.call(serde_json::json!({}), &ctx);
        assert!(matches!(r, Err(ToolError::InvalidArgs(_))));
    }

    #[test]
    fn test_grep_basic() {
        let tool = GrepTool::new();
        let ctx = ToolContext::new(None);
        let r = tool
            .call(
                serde_json::json!({ "pattern": "nonexistent_pattern_xyz_123", "path": "." }),
                &ctx,
            )
            .unwrap();
        assert!(r["exit_code"].as_i64().is_some());
        assert!(r["command"].as_str().unwrap() == "rg" || r["command"].as_str().unwrap() == "grep");
        assert!(r["stdout"].is_string() || r["stderr"].is_string());
    }
}
