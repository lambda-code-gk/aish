//! command output replay のユースケース。

use serde::Serialize;

use crate::domain::{CommandKind, LogEvent, OutputFormat};

#[derive(Debug, thiserror::Error)]
pub enum ReplayError {
    #[error("command index {0} not found or incomplete")]
    IndexNotFound(u32),
    #[error("replay index required (positional INDEX or --index)")]
    IndexRequired,
    #[error("invalid replay index {0}")]
    InvalidIndex(i64),
    #[error("--stderr is only valid for exec command spans")]
    ShellStderrNotSupported,
    #[error("no replayable command spans in log")]
    NoSpans,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReplaySpanView {
    pub index: u32,
    pub started_at: String,
    pub finished_at: String,
    pub exit_code: Option<i32>,
    pub kind: CommandKind,
    pub command: String,
}

#[derive(Debug, Clone)]
struct ReplaySpan {
    index: u32,
    started_at: String,
    finished_at: Option<String>,
    exit_code: Option<i32>,
    kind: CommandKind,
    command: String,
    stdout: String,
    stderr: String,
    complete: bool,
}

pub fn replay_list(
    events: &[LogEvent],
    index: Option<u32>,
    format: OutputFormat,
) -> Result<String, ReplayError> {
    let spans = complete_spans_from_events(events);
    let filtered: Vec<_> = spans
        .into_iter()
        .filter(|s| index.is_none_or(|i| s.index == i))
        .map(|s| s.into_view())
        .collect();
    if filtered.is_empty() {
        return Err(ReplayError::NoSpans);
    }
    Ok(render_list(&filtered, format))
}

pub fn replay_show(
    events: &[LogEvent],
    index: u32,
    stderr_only: bool,
) -> Result<String, ReplayError> {
    let spans = complete_spans_from_events(events);
    let span = spans
        .into_iter()
        .find(|s| s.index == index)
        .ok_or(ReplayError::IndexNotFound(index))?;
    if stderr_only {
        if span.kind != CommandKind::Exec {
            return Err(ReplayError::ShellStderrNotSupported);
        }
        return Ok(ensure_trailing_newline(span.stderr));
    }
    Ok(ensure_trailing_newline(span.stdout))
}

/// 端末表示で次のプロンプトと連結しないよう、記録どおりの本文の末尾に改行がなければ付与する。
fn ensure_trailing_newline(mut data: String) -> String {
    if !data.is_empty() && !data.ends_with('\n') {
        data.push('\n');
    }
    data
}

pub fn replay_span_views(events: &[LogEvent]) -> Result<Vec<ReplaySpanView>, ReplayError> {
    let spans = complete_spans_from_events(events);
    if spans.is_empty() {
        return Err(ReplayError::NoSpans);
    }
    Ok(spans.into_iter().map(|s| s.into_view()).collect())
}

/// `replay show` 向け index 解決。正数は `command_index`、負数は list 末尾からの offset（`-1` = 最後）。
pub fn resolve_replay_index(views: &[ReplaySpanView], spec: i64) -> Result<u32, ReplayError> {
    if views.is_empty() {
        return Err(ReplayError::NoSpans);
    }
    if spec == 0 {
        return Err(ReplayError::InvalidIndex(spec));
    }
    if spec > 0 {
        let index = u32::try_from(spec).map_err(|_| ReplayError::InvalidIndex(spec))?;
        if views.iter().any(|view| view.index == index) {
            Ok(index)
        } else {
            Err(ReplayError::IndexNotFound(index))
        }
    } else {
        let offset = spec
            .checked_neg()
            .and_then(|value| usize::try_from(value).ok())
            .filter(|&value| value > 0)
            .ok_or(ReplayError::InvalidIndex(spec))?;
        if offset > views.len() {
            return Err(ReplayError::InvalidIndex(spec));
        }
        let pos = views.len() - offset;
        Ok(views[pos].index)
    }
}

impl ReplaySpan {
    fn into_view(self) -> ReplaySpanView {
        ReplaySpanView {
            index: self.index,
            started_at: self.started_at,
            finished_at: self.finished_at.unwrap_or_default(),
            exit_code: self.exit_code,
            kind: self.kind,
            command: self.command,
        }
    }
}

fn render_list(spans: &[ReplaySpanView], format: OutputFormat) -> String {
    match format {
        OutputFormat::Tsv => {
            spans
                .iter()
                .map(|s| {
                    format!(
                        "{}\t{}\t{}\t{}\t{}\t{}",
                        s.index,
                        s.started_at,
                        s.finished_at,
                        s.exit_code.map(|c| c.to_string()).unwrap_or_default(),
                        match s.kind {
                            CommandKind::Shell => "shell",
                            CommandKind::Exec => "exec",
                            CommandKind::Session => "session",
                        },
                        sanitize_single_line_field(&s.command)
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
                + "\n"
        }
        OutputFormat::Json => serde_json::to_string(spans).unwrap_or_else(|_| "[]".to_string()),
        OutputFormat::Env => {
            let mut out = String::new();
            for s in spans {
                out.push_str(&format!(
                    "REPLAY_INDEX='{}'\n",
                    shell_quote(&s.index.to_string())
                ));
            }
            out
        }
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// TSV / picker 表示向けに 1 行へ潰す。記録済み文字列の意味は変えず、区切り文字だけ無害化する。
pub fn sanitize_single_line_field(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch == '\t' || ch == '\n' || ch == '\r' || ch.is_control() {
                ' '
            } else {
                ch
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn format_picker_line(span: &ReplaySpanView) -> String {
    let exit = span
        .exit_code
        .map(|c| c.to_string())
        .unwrap_or_else(|| "?".to_string());
    let kind = match span.kind {
        CommandKind::Shell => "shell",
        CommandKind::Exec => "exec",
        CommandKind::Session => "session",
    };
    let preview = {
        let flat = sanitize_single_line_field(&span.command);
        if flat.chars().count() > 80 {
            let truncated: String = flat.chars().take(77).collect();
            format!("{truncated}…")
        } else {
            flat
        }
    };
    format!(
        "{:>4}  {}  exit={}  {}  {}",
        span.index, span.started_at, exit, kind, preview
    )
}

fn complete_spans_from_events(events: &[LogEvent]) -> Vec<ReplaySpan> {
    let mut spans: Vec<ReplaySpan> = Vec::new();
    for event in events {
        apply_event(&mut spans, event.clone());
    }
    spans.into_iter().filter(|s| s.complete).collect()
}

fn apply_event(spans: &mut Vec<ReplaySpan>, event: LogEvent) {
    match event {
        LogEvent::CommandStart {
            command_index: Some(index),
            started_at: Some(started_at),
            kind: Some(kind),
            command,
            args,
            ..
        } if kind == CommandKind::Shell || kind == CommandKind::Exec => {
            let display = if args.is_empty() {
                command
            } else {
                format!("{command} {}", args.join(" "))
            };
            spans.push(ReplaySpan {
                index,
                started_at,
                finished_at: None,
                exit_code: None,
                kind,
                command: display,
                stdout: String::new(),
                stderr: String::new(),
                complete: false,
            });
        }
        LogEvent::Stdout {
            data,
            command_index: Some(index),
        } => {
            if let Some(span) = spans.iter_mut().find(|s| s.index == index && !s.complete) {
                span.stdout.push_str(&data);
            }
        }
        LogEvent::Stderr {
            data,
            command_index: Some(index),
        } => {
            if let Some(span) = spans.iter_mut().find(|s| s.index == index && !s.complete) {
                span.stderr.push_str(&data);
            }
        }
        LogEvent::CommandEnd {
            command_index,
            exit_code,
            finished_at,
        } => {
            if let Some(span) = spans.iter_mut().find(|s| s.index == command_index) {
                span.finished_at = Some(finished_at);
                span.exit_code = exit_code;
                span.complete = true;
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CommandSpec, LogEvent};

    fn sample_exec_events() -> Vec<LogEvent> {
        vec![
            LogEvent::command_start_span(
                &CommandSpec {
                    program: "echo".to_string(),
                    args: vec!["hello".to_string()],
                },
                1,
                "2026-01-01T00:00:00Z",
                CommandKind::Exec,
            ),
            LogEvent::stdout_indexed("hello\n", 1),
            LogEvent::command_end(1, Some(0), "2026-01-01T00:00:01Z"),
        ]
    }

    #[test]
    fn replay_list_shows_only_complete_spans() {
        let mut events = sample_exec_events();
        events.push(LogEvent::shell_command_start(
            2,
            "2026-01-01T00:00:00Z",
            "partial",
        ));

        let spans = complete_spans_from_events(&events);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].index, 1);
        assert_eq!(spans[0].stdout, "hello\n");
    }

    #[test]
    fn replay_show_emits_recorded_streams_without_resanitizing() {
        let events = sample_exec_events();
        let out = replay_show(&events, 1, false).expect("show");
        assert_eq!(out, "hello\n");
    }

    #[test]
    fn replay_show_preserves_multiline_stdout() {
        let events = vec![
            LogEvent::shell_command_start(1, "2026-01-01T00:00:00Z", "ls"),
            LogEvent::stdout_indexed("line1\n", 1),
            LogEvent::stdout_indexed("line2\n", 1),
            LogEvent::command_end(1, Some(0), "2026-01-01T00:00:01Z"),
        ];
        let out = replay_show(&events, 1, false).expect("show");
        assert_eq!(out, "line1\nline2\n");
    }

    #[test]
    fn replay_show_appends_trailing_newline_when_missing() {
        let events = vec![
            LogEvent::shell_command_start(1, "2026-01-01T00:00:00Z", "printf hi"),
            LogEvent::stdout_indexed("hi", 1),
            LogEvent::command_end(1, Some(0), "2026-01-01T00:00:01Z"),
        ];
        let out = replay_show(&events, 1, false).expect("show");
        assert_eq!(out, "hi\n");
    }

    #[test]
    fn replay_show_rejects_shell_stderr() {
        let events = vec![
            LogEvent::shell_command_start(1, "2026-01-01T00:00:00Z", "echo hi"),
            LogEvent::stdout_indexed("hi\n", 1),
            LogEvent::command_end(1, Some(0), "2026-01-01T00:00:01Z"),
        ];

        let err = replay_show(&events, 1, true).expect_err("shell stderr");
        assert!(matches!(err, ReplayError::ShellStderrNotSupported));
    }

    #[test]
    fn replay_list_tsv_flattens_multiline_command() {
        let events = vec![
            LogEvent::command_start_span(
                &CommandSpec {
                    program: "echo".to_string(),
                    args: vec!["a\nb".to_string()],
                },
                1,
                "2026-01-01T00:00:00Z",
                CommandKind::Exec,
            ),
            LogEvent::stdout_indexed("ok\n", 1),
            LogEvent::command_end(1, Some(0), "2026-01-01T00:00:01Z"),
        ];
        let out = replay_list(&events, None, OutputFormat::Tsv).expect("list");
        let data_lines: Vec<_> = out.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(data_lines.len(), 1);
        assert!(data_lines[0].contains("echo a b"));
    }

    #[test]
    fn resolve_replay_index_supports_negative_offset_from_list_end() {
        let views = vec![
            ReplaySpanView {
                index: 1,
                started_at: String::new(),
                finished_at: String::new(),
                exit_code: Some(0),
                kind: CommandKind::Shell,
                command: "ls".to_string(),
            },
            ReplaySpanView {
                index: 2,
                started_at: String::new(),
                finished_at: String::new(),
                exit_code: Some(0),
                kind: CommandKind::Shell,
                command: "pwd".to_string(),
            },
        ];
        assert_eq!(resolve_replay_index(&views, 1).expect("positive"), 1);
        assert_eq!(resolve_replay_index(&views, -1).expect("last"), 2);
        assert_eq!(resolve_replay_index(&views, -2).expect("second last"), 1);
        assert!(matches!(
            resolve_replay_index(&views, -3),
            Err(ReplayError::InvalidIndex(-3))
        ));
        assert!(matches!(
            resolve_replay_index(&views, 0),
            Err(ReplayError::InvalidIndex(0))
        ));
    }

    #[test]
    fn sanitize_single_line_field_replaces_control_chars() {
        assert_eq!(sanitize_single_line_field("a\tb\nc"), "a b c");
    }
}
