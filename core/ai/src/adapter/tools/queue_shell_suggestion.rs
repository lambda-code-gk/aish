//! queue_shell_suggestion ツール実装（adapter 層）
//!
//! - StructuredCommand を受け取り、安全な 1 コマンドへ shell-quote（引数内改行は許可）
//! - command_rules によるポリシー評価
//! - SessionDir 配下の pending_input.json に PendingInput を保存（AISH が読んで注入後に削除）

use crate::adapter::agent_state_storage::FileAgentStateStorage;
use common::adapter::StdFileSystem;
use common::domain::{PendingInput, PolicyStatus, StructuredCommand};
use common::ports::outbound::FileSystem;
use common::tool::{is_command_allowed, Tool, ToolContext, ToolError};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Deserialize)]
struct QueueShellSuggestionArgs {
    command: StructuredCommand,
    /// 任意の表示用ヒント（現状ロジックでは未使用だが、将来のために保持）
    #[serde(default, rename = "display_hint")]
    _display_hint: Option<String>,
}

pub struct QueueShellSuggestionTool;

impl QueueShellSuggestionTool {
    pub const NAME: &'static str = "queue_shell_suggestion";

    pub fn new() -> Self {
        Self
    }
}

impl Default for QueueShellSuggestionTool {
    fn default() -> Self {
        Self::new()
    }
}

fn shell_quote(arg: &str) -> Result<String, ToolError> {
    // 制御文字チェック（タブ・改行・CR は許可し、ESC 等の危険な制御文字のみ拒否）
    if arg.chars().any(|c| {
        ((c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r') || c == '\x7f'
    }) {
        return Err(ToolError::InvalidArgs(
            "command contains control characters (newlines in arguments are allowed)".to_string(),
        ));
    }
    // ダブルクォートで囲む。\n \r \t は実際の制御文字に変換（Bracketed Paste ではシェルが \n を解釈しないため）。
    // " $ ` と、\ の直後が ", $, `, \ のときは \ を前置。それ以外の \ は \\ に。
    if arg.is_empty() {
        return Ok("\"\"".to_string());
    }
    let mut out = String::from("\"");
    let mut chars = arg.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => {
                match chars.peek().copied() {
                    Some('n') => {
                        chars.next();
                        out.push('\n');
                    }
                    Some('r') => {
                        chars.next();
                        out.push('\r');
                    }
                    Some('t') => {
                        chars.next();
                        out.push('\t');
                    }
                    Some('"') | Some('$') | Some('`') | Some('\\') => {
                        out.push('\\');
                        out.push(chars.next().unwrap());
                    }
                    _ => out.push_str("\\\\"),
                }
            }
            '$' => out.push_str("\\$"),
            '`' => out.push_str("\\`"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    Ok(out)
}

/// 注入用文字列をサニタイズ。改行・CR・タブは許可し、ESC 等の危険な制御文字のみ拒否。長さ制限あり。
fn sanitize_for_inject(s: &str, max_len: usize) -> Result<String, ToolError> {
    let mut out = String::with_capacity(s.len().min(max_len));
    let mut count = 0usize;
    for ch in s.chars() {
        if (ch as u32) < 0x20 && ch != '\t' && ch != '\n' && ch != '\r' {
            return Err(ToolError::InvalidArgs(
                "command contains control characters (ESC etc.). Newlines in arguments are allowed.".to_string(),
            ));
        }
        if ch == '\x7f' {
            return Err(ToolError::InvalidArgs(
                "command contains control characters (DEL). Newlines in arguments are allowed.".to_string(),
            ));
        }
        out.push(ch);
        count += 1;
        if count >= max_len {
            out.push('…');
            break;
        }
    }
    Ok(out)
}

impl Tool for QueueShellSuggestionTool {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn description(&self) -> &'static str {
        "Queue a shell command suggestion to be injected into the next shell prompt (without executing it). Arguments may contain newlines (e.g. multi-line commit messages); control characters such as ESC are not allowed."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "object",
                    "properties": {
                        "program": { "type": "string" },
                        "args": { "type": "array", "items": { "type": "string" } },
                        // Google Gemini の tools スキーマは `type: ["string","null"]` のような
                        // union を受け付けないため、schema 上は string のみとし、
                        // 未指定 (= フィールドごと省略) を「なし」として扱う。
                        "cwd": { "type": "string", "description": "Optional working directory. Omit this field to use the default." }
                    },
                    "required": ["program", "args"]
                },
                "display_hint": { "type": "string" }
            },
            "required": ["command"]
        }))
    }

    fn call(&self, args: Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let parsed: QueueShellSuggestionArgs =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs(e.to_string()))?;

        let StructuredCommand { program, args, .. } = parsed.command;

        if program.trim().is_empty() {
            return Err(ToolError::InvalidArgs(
                "command.program must not be empty".to_string(),
            ));
        }
        // program に改行を含めると注入後に別コマンドとして実行されるため禁止
        if program.contains('\n') || program.contains('\r') {
            return Err(ToolError::InvalidArgs(
                "command.program must not contain newlines".to_string(),
            ));
        }

        // 1) StructuredCommand → shell-quoted 1 コマンド文字列（注入用。引数内改行は引用内に含まれる）
        //    program は /^[A-Za-z0-9_./-]+$/ の場合のみクォートを省略する
        let mut line = if program
            .chars()
            .all(|c| matches!(c, 'A'..='Z' | 'a'..='z' | '0'..='9' | '_' | '.' | '/' | '-'))
        {
            program.clone()
        } else {
            shell_quote(&program)?
        };
        for arg in &args {
            line.push(' ');
            line.push_str(&shell_quote(arg)?);
        }
        let line = sanitize_for_inject(&line, 4096)?;

        // 2) policy 判定（command_rules は「引用なし」のコマンド行で比較する）
        let line_for_allowlist: String =
            std::iter::once(program.as_str())
                .chain(args.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(" ");
        let allowed = is_command_allowed(line_for_allowlist.trim(), &ctx.command_allow_rules);
        let policy = if allowed {
            PolicyStatus::Allowed
        } else {
            PolicyStatus::Blocked {
                reason: "not in command_rules allowlist".to_string(),
            }
        };

        // 3) PendingInput を構築
        let created_at_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        let pending = PendingInput {
            text: line.clone(),
            policy,
            created_at_unix_ms,
            source: "tool:queue_shell_suggestion".to_string(),
        };

        // 4) セッション dir に pending_input.json を保存（session が無い場合は queued: false）
        let session_path_opt = ctx
            .session_dir
            .as_ref()
            .map(|p| p.to_path_buf())
            .or_else(|| std::env::var("AISH_SESSION").ok().map(Into::into));

        let (queued, no_session_reason) = if let Some(session_dir) = session_path_opt.as_ref() {
            let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
            let storage = FileAgentStateStorage::new(Arc::clone(&fs));
            match storage.save_pending_input(
                &common::domain::SessionDir::new(session_dir.clone()),
                Some(pending.clone()),
            ) {
                Ok(()) => (true, None),
                Err(e) => return Err(ToolError::ExecutionFailed(e.to_string())),
            }
        } else {
            (false, Some("no session dir"))
        };

        // 5) LLM への軽量応答
        let mut out = serde_json::json!({
            "queued": queued,
            "policy": match pending.policy {
                PolicyStatus::Allowed => "allowed",
                PolicyStatus::NeedsWarning { .. } => "needs_warning",
                PolicyStatus::Blocked { .. } => "blocked",
            },
            "text": pending.text,
        });
        if let Some(reason) = no_session_reason {
            out["reason"] = serde_json::Value::String(reason.to_string());
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::tool::ToolContext;

    #[test]
    fn test_shell_quote_double_quote_and_escape() {
        assert_eq!(shell_quote("echo").unwrap(), "\"echo\"");
        assert_eq!(shell_quote("a'b").unwrap(), "\"a'b\""); // ' はダブルクォート内ではそのまま
        // 入力 a + " + b → 出力は " で囲み、内部の " は \" にエスケープ
        let q = shell_quote("a\"b").unwrap();
        assert!(q.starts_with('"') && q.ends_with('"'));
        assert!(q.contains("\\\""));
        // 入力 a + \ + b → \ の次が n,r,t," 等でないので \\ にエスケープ
        let r = shell_quote("a\\b").unwrap();
        assert!(r.starts_with('"') && r.ends_with('"'));
        assert!(r.contains("\\\\"));
        // \n は実際の改行文字に変換（Bracketed Paste で確実に改行になる）
        let s = shell_quote("line1\\n\\nline2").unwrap();
        assert!(s.contains('\n'));
        assert!(s.starts_with('"') && s.ends_with('"'));
    }

    #[test]
    fn test_sanitize_for_inject_allows_newline_rejects_esc() {
        assert!(sanitize_for_inject("echo\nx", 100).is_ok());
        assert_eq!(sanitize_for_inject("echo\nx", 100).unwrap(), "echo\nx");
        assert!(sanitize_for_inject("echo\x1b[31m", 100).is_err());
    }

    #[test]
    fn test_shell_quote_allows_newline_in_arg() {
        let q = shell_quote("line1\n\nline2").unwrap();
        assert!(q.starts_with('"'));
        assert!(q.ends_with('"'));
        assert!(q.contains("line1"));
        assert!(q.contains("line2"));
        assert!(q.contains('\n')); // 改行はそのまま（シェルで \n として解釈される）
    }

    #[test]
    fn test_queue_shell_suggestion_rejects_program_with_newline() {
        let tmp = tempfile::tempdir().unwrap();
        let session_dir = tmp.path().to_path_buf();
        let tool = QueueShellSuggestionTool::new();
        let ctx = ToolContext::new(Some(session_dir)).with_allow_unsafe(true);
        let args = serde_json::json!({
            "command": {
                "program": "git\nrm",
                "args": ["-rf", "/"],
                "cwd": null
            }
        });
        let result = tool.call(args, &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_queue_shell_suggestion_allows_basic_command() {
        let tmp = tempfile::tempdir().unwrap();
        let session_dir = tmp.path().to_path_buf();
        let tool = QueueShellSuggestionTool::new();
        let ctx = ToolContext::new(Some(session_dir.clone())).with_allow_unsafe(true);
        let args = serde_json::json!({
            "command": {
                "program": "git",
                "args": ["status"],
                "cwd": null
            }
        });
        let result = tool.call(args, &ctx).unwrap();
        assert_eq!(result["queued"], true);
        // command_rules が空のため allowlist 判定では Blocked になる
        assert_eq!(result["policy"], "blocked");
        assert!(result["text"].as_str().unwrap().starts_with("git \"status\""));
    }

    #[test]
    fn test_queue_shell_suggestion_no_session_dir_returns_queued_false() {
        // 実行環境に AISH_SESSION が設定されていると、ToolContext::new(None) でも
        // env 経由でセッションディレクトリが解決されてしまい、テストが不安定になる。
        // 明示的に AISH_SESSION を一時的に無効化して「セッションなし」状態を再現する。
        let prev_aish_session = std::env::var("AISH_SESSION").ok();
        std::env::remove_var("AISH_SESSION");

        let tool = QueueShellSuggestionTool::new();
        let ctx = ToolContext::new(None).with_allow_unsafe(true);
        let args = serde_json::json!({
            "command": {
                "program": "echo",
                "args": ["hi"],
                "cwd": null
            }
        });
        let result = tool.call(args, &ctx).unwrap();
        assert_eq!(result["queued"], false);
        assert_eq!(result["reason"], "no session dir");

        // 環境変数を元に戻す
        if let Some(v) = prev_aish_session {
            std::env::set_var("AISH_SESSION", v);
        } else {
            std::env::remove_var("AISH_SESSION");
        }
    }
}

