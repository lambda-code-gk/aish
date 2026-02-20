//! シェルコマンド実行ツール（adapter 層）
//!
//! OS 副作用（sh -c 実行）を伴うため、common ではなく adapter に配置。
//! allowlist 判定を行い、allow_unsafe=false かつ不一致なら PermissionDenied を返す。

use common::domain::event::Event;
use common::tool::{is_command_allowed, Tool, ToolContext, ToolError};
use serde_json::Value;

/// シェルコマンド実行ツール（API 名 "run_shell"）
pub struct ShellTool;

impl ShellTool {
    pub const NAME: &'static str = "run_shell";

    pub fn new() -> Self {
        Self
    }

    /// コマンドが許可リストにマッチするか判定する
    pub fn is_allowed(&self, command: &str, ctx: &ToolContext) -> bool {
        is_command_allowed(command, &ctx.command_allow_rules)
    }
}

impl Default for ShellTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for ShellTool {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn description(&self) -> &'static str {
        "Execute a shell command. Use when you need to run a command on the user's machine (e.g. list files, run a script). Pass a single string 'command' to run via sh -c."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "Shell command to execute (run with sh -c)" }
            },
            "required": ["command"]
        }))
    }

    fn call(&self, args: Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing 'command'".to_string()))?
            .to_string();

        if command.trim().is_empty() {
            return Err(ToolError::InvalidArgs(
                "command must not be empty".to_string(),
            ));
        }

        let tool_start = std::time::Instant::now();
        if let (Some(ref hub), Some(ref sid), Some(ref rid)) =
            (&ctx.event_hub, &ctx.session_id, &ctx.run_id)
        {
            hub.emit(Event {
                v: 1,
                session_id: sid.clone(),
                run_id: rid.clone(),
                kind: "tool.started".to_string(),
                payload: serde_json::json!({ "tool": Self::NAME }),
            });
        }

        // allowlist 判定: allow_unsafe=false かつ不一致なら PermissionDenied
        if !ctx.allow_unsafe && !self.is_allowed(&command, ctx) {
            let duration_ms = tool_start.elapsed().as_millis() as u64;
            if let (Some(ref hub), Some(ref sid), Some(ref rid)) =
                (&ctx.event_hub, &ctx.session_id, &ctx.run_id)
            {
                hub.emit(Event {
                    v: 1,
                    session_id: sid.clone(),
                    run_id: rid.clone(),
                    kind: "tool.failed".to_string(),
                    payload: serde_json::json!({
                        "tool": Self::NAME,
                        "reason": "permission_denied",
                        "duration_ms": duration_ms,
                    }),
                });
            }
            return Err(ToolError::PermissionDenied(format!(
                "Command not in allowlist: {}",
                command
            )));
        }

        // コマンド実行
        let result = std::process::Command::new("sh")
            .arg("-c")
            .arg(&command)
            .output()
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()));

        let duration_ms = tool_start.elapsed().as_millis() as u64;
        if let (Some(ref hub), Some(ref sid), Some(ref rid)) =
            (&ctx.event_hub, &ctx.session_id, &ctx.run_id)
        {
            match &result {
                Ok(_) => {
                    hub.emit(Event {
                        v: 1,
                        session_id: sid.clone(),
                        run_id: rid.clone(),
                        kind: "tool.finished".to_string(),
                        payload: serde_json::json!({
                            "tool": Self::NAME,
                            "duration_ms": duration_ms,
                            "ok": true,
                        }),
                    });
                }
                Err(_) => {
                    hub.emit(Event {
                        v: 1,
                        session_id: sid.clone(),
                        run_id: rid.clone(),
                        kind: "tool.failed".to_string(),
                        payload: serde_json::json!({
                            "tool": Self::NAME,
                            "duration_ms": duration_ms,
                        }),
                    });
                }
            }
        }

        let output = result?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(1);

        Ok(serde_json::json!({
            "stdout": stdout,
            "stderr": stderr,
            "exit_code": exit_code
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::tool::CommandAllowRule;
    use regex::Regex;

    #[test]
    fn test_shell_tool_basic() {
        let shell = ShellTool::new();
        assert_eq!(shell.name(), ShellTool::NAME);

        // allow_unsafe=true で実行できる
        let ctx = ToolContext::new(None).with_allow_unsafe(true);
        let r = shell
            .call(serde_json::json!({ "command": "echo hello" }), &ctx)
            .unwrap();
        assert_eq!(r["stdout"].as_str(), Some("hello\n"));
        assert_eq!(r["exit_code"].as_i64(), Some(0));
    }

    #[test]
    fn test_shell_tool_missing_command() {
        let shell = ShellTool::new();
        let ctx = ToolContext::new(None).with_allow_unsafe(true);
        let r = shell.call(serde_json::json!({}), &ctx);
        assert!(matches!(r, Err(ToolError::InvalidArgs(_))));
    }

    #[test]
    fn test_shell_tool_empty_command() {
        let shell = ShellTool::new();
        let ctx = ToolContext::new(None).with_allow_unsafe(true);
        let r = shell.call(serde_json::json!({ "command": "" }), &ctx);
        assert!(matches!(r, Err(ToolError::InvalidArgs(_))));
    }

    #[test]
    fn test_shell_tool_permission_denied() {
        let shell = ShellTool::new();
        // allow_unsafe=false、allowlist 空で PermissionDenied
        let ctx = ToolContext::new(None);
        let r = shell.call(serde_json::json!({ "command": "rm -rf /" }), &ctx);
        assert!(matches!(r, Err(ToolError::PermissionDenied(_))));
    }

    #[test]
    fn test_shell_tool_allowed_by_rules() {
        let shell = ShellTool::new();
        let ctx = ToolContext::new(None)
            .with_command_allow_rules(vec![CommandAllowRule::Prefix("echo".to_string())]);
        // allowlist にマッチすれば allow_unsafe=false でも実行可能
        let r = shell
            .call(serde_json::json!({ "command": "echo hello" }), &ctx)
            .unwrap();
        assert_eq!(r["stdout"].as_str(), Some("hello\n"));
    }

    #[test]
    fn test_shell_tool_is_allowed() {
        let shell = ShellTool::new();
        let ctx = ToolContext::new(None).with_command_allow_rules(vec![
            CommandAllowRule::Regex(Regex::new(r"^echo .*").unwrap()),
            CommandAllowRule::Prefix("ls".to_string()),
            CommandAllowRule::Prefix("sed".to_string()),
            CommandAllowRule::NotRegex(Regex::new(r"sed .*-i ").unwrap()),
        ]);

        assert!(shell.is_allowed("echo hello", &ctx));
        assert!(shell.is_allowed("ls", &ctx));
        assert!(shell.is_allowed("ls -la", &ctx));
        assert!(shell.is_allowed("sed 's/a/b/' file", &ctx));
        assert!(!shell.is_allowed("sed -i 's/a/b/' file", &ctx));
        assert!(!shell.is_allowed("rm -rf /", &ctx));
        // 前方一致は「ルール+スペース」で判定するため、ls では lss は許可されない
        assert!(!shell.is_allowed("lss", &ctx));
    }

    #[test]
    fn test_shell_tool_allow_unsafe_overrides() {
        let shell = ShellTool::new();
        // allowlist にマッチしなくても allow_unsafe=true なら実行可能
        let ctx = ToolContext::new(None)
            .with_command_allow_rules(vec![CommandAllowRule::Prefix("ls".to_string())])
            .with_allow_unsafe(true);
        let r = shell
            .call(serde_json::json!({ "command": "echo allowed" }), &ctx)
            .unwrap();
        assert_eq!(r["stdout"].as_str(), Some("allowed\n"));
    }
}
