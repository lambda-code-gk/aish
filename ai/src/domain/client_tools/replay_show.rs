//! `aish.replay_show` client tool の実行と wrapper 生成。

use aibe_client::ClientToolCallRequest;
use aibe_protocol::{ClientToolErrorKind, ClientToolResult, ClientToolResultStatus};
use aish_replay::{
    ensure_trailing_newline, replay_show, replay_span_views, sanitize_single_line_field, LogEvent,
};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayShowStream {
    Stdout,
    Stderr,
    Both,
}

impl ReplayShowStream {
    pub fn parse(raw: Option<&str>) -> Self {
        match raw {
            Some("stderr") => Self::Stderr,
            Some("stdout") => Self::Stdout,
            _ => Self::Both,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayShowArgs {
    pub index: i64,
    pub stream: ReplayShowStream,
    pub tail_bytes: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum ReplayShowError {
    #[error("invalid arguments: {0}")]
    InvalidArguments(String),
    #[error("span not found")]
    SpanNotFound,
    #[error("span incomplete")]
    SpanIncomplete,
    #[error("source unavailable")]
    SourceUnavailable,
}

pub fn parse_replay_show_args(arguments: &Value) -> Result<ReplayShowArgs, ReplayShowError> {
    let index = arguments
        .get("index")
        .and_then(Value::as_i64)
        .ok_or_else(|| ReplayShowError::InvalidArguments("missing index".into()))?;
    let stream = ReplayShowStream::parse(arguments.get("stream").and_then(Value::as_str));
    let tail_bytes = arguments
        .get("tail_bytes")
        .and_then(Value::as_u64)
        .unwrap_or(8192)
        .clamp(1, 16_384) as usize;
    Ok(ReplayShowArgs {
        index,
        stream,
        tail_bytes,
    })
}

pub fn execute_replay_show(
    request: &ClientToolCallRequest,
    events: &[LogEvent],
) -> Result<ClientToolResult, ReplayShowError> {
    let args = parse_replay_show_args(&request.arguments)?;
    let views = replay_span_views(events)
        .map_err(|_| ReplayShowError::InvalidArguments("no spans".into()))?;
    let index = aish_replay::resolve_replay_index(&views, args.index)
        .map_err(|_| ReplayShowError::SpanNotFound)?;
    let span = views
        .iter()
        .find(|view| view.index == index)
        .ok_or(ReplayShowError::SpanNotFound)?;

    if matches!(args.stream, ReplayShowStream::Stderr)
        && span.kind != aish_replay::CommandKind::Exec
    {
        return Err(ReplayShowError::InvalidArguments(
            "stderr is only valid for exec spans".into(),
        ));
    }

    let stdout = match args.stream {
        ReplayShowStream::Stdout => {
            replay_show(events, index, false).map_err(|_| ReplayShowError::SpanNotFound)?
        }
        ReplayShowStream::Stderr => {
            replay_show(events, index, true).map_err(|_| ReplayShowError::SpanNotFound)?
        }
        ReplayShowStream::Both => {
            let stdout =
                replay_show(events, index, false).map_err(|_| ReplayShowError::SpanNotFound)?;
            let stderr = replay_show(events, index, true).unwrap_or_default();
            format!("{stdout}\n{stderr}")
        }
    };

    let truncated = stdout.len() > args.tail_bytes;
    let rendered = if truncated {
        tail_bytes(&stdout, args.tail_bytes)
    } else {
        stdout
    };

    Ok(ClientToolResult {
        id: request.id.clone(),
        turn_id: request.turn_id.clone(),
        call_id: request.call_id.clone(),
        status: ClientToolResultStatus::Ok,
        error_kind: None,
        content: format!(
            "[untrusted terminal output]\n\
tool: aish.replay_show\n\
index: {}\n\
command: {}\n\
exit_code: {}\n\
stream: {}\n\
truncated: {}\n\
tail_bytes: {}\n\n{}",
            index,
            sanitize_single_line_field(&span.command),
            span.exit_code.map(|c| c.to_string()).unwrap_or_default(),
            match args.stream {
                ReplayShowStream::Stdout => "stdout",
                ReplayShowStream::Stderr => "stderr",
                ReplayShowStream::Both => "both",
            },
            truncated,
            args.tail_bytes,
            rendered
        ),
    })
}

pub fn replay_show_error_kind(err: &ReplayShowError) -> ClientToolErrorKind {
    match err {
        ReplayShowError::InvalidArguments(_) => ClientToolErrorKind::InvalidArguments,
        ReplayShowError::SpanNotFound => ClientToolErrorKind::SpanNotFound,
        ReplayShowError::SpanIncomplete => ClientToolErrorKind::SpanIncomplete,
        ReplayShowError::SourceUnavailable => ClientToolErrorKind::SessionDirMissing,
    }
}

pub fn replay_tool_error_to_result(
    request: &ClientToolCallRequest,
    kind: ClientToolErrorKind,
    message: impl Into<String>,
) -> ClientToolResult {
    ClientToolResult {
        id: request.id.clone(),
        turn_id: request.turn_id.clone(),
        call_id: request.call_id.clone(),
        status: ClientToolResultStatus::Error,
        error_kind: Some(kind),
        content: message.into(),
    }
}

fn tail_bytes(text: &str, tail_bytes: usize) -> String {
    if text.len() <= tail_bytes {
        return ensure_trailing_newline(text.to_string());
    }
    let start = text.len().saturating_sub(tail_bytes);
    let start = text
        .char_indices()
        .find(|(idx, _)| *idx >= start)
        .map(|(idx, _)| idx)
        .unwrap_or(start);
    ensure_trailing_newline(text[start..].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(arguments: serde_json::Value) -> ClientToolCallRequest {
        ClientToolCallRequest {
            id: "id-1".into(),
            turn_id: "turn-1".into(),
            call_id: "call-1".into(),
            name: "aish.replay_show".into(),
            arguments,
        }
    }

    #[test]
    fn replay_show_returns_untrusted_terminal_output_wrapper() {
        let events = vec![
            LogEvent::command_start_span(
                &aish_replay::CommandSpec {
                    program: "echo".into(),
                    args: vec!["hello".into()],
                },
                1,
                "2026-01-01T00:00:00Z",
                aish_replay::CommandKind::Exec,
            ),
            LogEvent::stdout_indexed("hello\n", 1),
            LogEvent::command_end(1, Some(0), "2026-01-01T00:00:01Z"),
        ];
        let result = execute_replay_show(
            &request(serde_json::json!({"index": 1, "tail_bytes": 8192})),
            &events,
        )
        .expect("execute");
        assert!(result.content.starts_with("[untrusted terminal output]"));
        assert!(result.content.contains("tool: aish.replay_show"));
        assert!(result.content.contains("stream: both"));
    }

    #[test]
    fn replay_show_rejects_shell_span_stderr() {
        let err = execute_replay_show(
            &request(serde_json::json!({"index": 1, "stream": "stderr"})),
            &[
                LogEvent::shell_command_start(1, "2026-01-01T00:00:00Z", "echo hi"),
                LogEvent::stdout_indexed("hi\n", 1),
                LogEvent::command_end(1, Some(0), "2026-01-01T00:00:01Z"),
            ],
        )
        .expect_err("stderr");
        assert!(matches!(err, ReplayShowError::InvalidArguments(_)));
        assert_eq!(
            replay_show_error_kind(&err),
            ClientToolErrorKind::InvalidArguments
        );
    }

    #[test]
    fn client_tool_error_kinds_cover_missing_session_and_span_states() {
        assert_eq!(
            replay_show_error_kind(&ReplayShowError::SourceUnavailable),
            ClientToolErrorKind::SessionDirMissing
        );
        assert_eq!(
            replay_show_error_kind(&ReplayShowError::SpanNotFound),
            ClientToolErrorKind::SpanNotFound
        );
        assert_eq!(
            replay_show_error_kind(&ReplayShowError::SpanIncomplete),
            ClientToolErrorKind::SpanIncomplete
        );
    }
}
