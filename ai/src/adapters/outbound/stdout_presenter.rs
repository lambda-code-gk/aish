//! 標準出力プレゼンター。

use aibe::ports::outbound::DEFAULT_MAX_TOOL_OUTPUT_BYTES;
use aibe::protocol::ClientResponse;

use crate::ports::outbound::Presenter;

pub struct StdoutPresenter;

/// `show_response` が書き込む内容（テスト・契約検証用）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PresenterOutput {
    pub stdout: Option<String>,
    pub stderr: Vec<String>,
}

impl Presenter for StdoutPresenter {
    fn show_response(&self, response: &ClientResponse, verbose_tools: bool) {
        let out = render_response(response, verbose_tools);
        if let Some(s) = out.stdout {
            println!("{s}");
        }
        for line in out.stderr {
            eprintln!("{line}");
        }
    }

    fn show_error(&self, message: &str) {
        eprintln!("ai: {message}");
    }
}

pub fn render_response(response: &ClientResponse, verbose_tools: bool) -> PresenterOutput {
    match response {
        ClientResponse::AgentTurnResult {
            status,
            assistant_message,
            tool_calls,
            ..
        } => {
            let mut stderr = Vec::new();
            if status == "max_tool_rounds" {
                stderr.push(
                    "warning: ai: max tool rounds reached; showing partial assistant reply"
                        .to_string(),
                );
            }
            if verbose_tools {
                for tc in tool_calls {
                    stderr.push(format_tool_call_line(tc));
                }
            }
            PresenterOutput {
                stdout: Some(assistant_message.content.clone()),
                stderr,
            }
        }
        ClientResponse::Pong { id } => PresenterOutput {
            stdout: None,
            stderr: vec![format!("ai: pong ({id})")],
        },
        ClientResponse::Error { message, .. } => PresenterOutput {
            stdout: None,
            stderr: vec![format!("aibe error: {message}")],
        },
    }
}

pub fn format_tool_call_line(tc: &serde_json::Value) -> String {
    let name = tc.get("name").and_then(|v| v.as_str()).unwrap_or("?");
    let status = tc.get("status").and_then(|v| v.as_str()).unwrap_or("?");
    let args = tc
        .get("arguments")
        .map(|v| truncate_bytes(&v.to_string(), DEFAULT_MAX_TOOL_OUTPUT_BYTES))
        .unwrap_or_else(|| "?".to_string());
    let detail = if status == "ok" {
        tc.get("output")
            .and_then(|v| v.as_str())
            .map(|s| truncate_bytes(s, DEFAULT_MAX_TOOL_OUTPUT_BYTES))
            .unwrap_or_default()
    } else {
        let err = tc.get("error").and_then(|v| v.as_str()).unwrap_or("");
        let msg = tc.get("message").and_then(|v| v.as_str()).unwrap_or("");
        format!("{err}: {msg}")
    };
    format!("ai: tool {name} {status} args={args} output={detail}")
}

pub fn truncate_bytes(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… [truncated]", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use aibe::protocol::ProtocolMessageOut;
    use serde_json::json;

    #[test]
    fn truncates_multibyte_safe() {
        let s = "あ".repeat(20_000);
        let out = truncate_bytes(&s, 10);
        assert!(out.ends_with("[truncated]"));
        assert!(out.len() < s.len());
    }

    #[test]
    fn pong_never_writes_stdout() {
        let out = render_response(&ClientResponse::Pong { id: "x".into() }, false);
        assert!(out.stdout.is_none());
        assert_eq!(out.stderr, vec!["ai: pong (x)"]);
    }

    #[test]
    fn max_tool_rounds_warning_on_stderr_only() {
        let out = render_response(
            &ClientResponse::AgentTurnResult {
                id: "id".into(),
                status: "max_tool_rounds".into(),
                assistant_message: ProtocolMessageOut {
                    role: "assistant".into(),
                    content: "partial".into(),
                },
                tool_calls: vec![],
            },
            false,
        );
        assert_eq!(out.stdout.as_deref(), Some("partial"));
        assert_eq!(out.stderr.len(), 1);
        assert!(out.stderr[0].contains("max tool rounds"));
    }

    #[test]
    fn verbose_tools_on_stderr_not_stdout() {
        let huge = "x".repeat(DEFAULT_MAX_TOOL_OUTPUT_BYTES + 500);
        let out = render_response(
            &ClientResponse::AgentTurnResult {
                id: "id".into(),
                status: "ok".into(),
                assistant_message: ProtocolMessageOut {
                    role: "assistant".into(),
                    content: "final".into(),
                },
                tool_calls: vec![json!({
                    "name": "read_file",
                    "status": "ok",
                    "arguments": {"path": "a"},
                    "output": huge
                })],
            },
            true,
        );
        assert_eq!(out.stdout.as_deref(), Some("final"));
        assert_eq!(out.stderr.len(), 1);
        let line = &out.stderr[0];
        assert!(line.starts_with("ai: tool read_file ok"));
        assert!(line.contains("[truncated]"));
        assert!(line.len() < huge.len());
    }

    #[test]
    fn format_tool_call_line_truncates_output() {
        let huge = "y".repeat(DEFAULT_MAX_TOOL_OUTPUT_BYTES + 100);
        let line = format_tool_call_line(&json!({
            "name": "shell_exec",
            "status": "ok",
            "arguments": {},
            "output": huge
        }));
        assert!(line.contains("[truncated]"));
        assert!(line.len() < huge.len() + 80);
    }
}
