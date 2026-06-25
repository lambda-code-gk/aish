//! aish replay の共有 parser とログイベント定義。

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// 1 コマンド実行の指定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
}

/// replay 用のコマンド種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandKind {
    Shell,
    Exec,
    Session,
}

/// ログ 1 行（1 イベント）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum LogEvent {
    CommandStart {
        command: String,
        args: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        command_index: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        started_at: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<CommandKind>,
    },
    Stdout {
        data: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        command_index: Option<u32>,
    },
    Stderr {
        data: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        command_index: Option<u32>,
    },
    CommandEnd {
        command_index: u32,
        exit_code: Option<i32>,
        finished_at: String,
    },
    Exit {
        code: Option<i32>,
    },
}

impl LogEvent {
    /// `command` / `args` を `sanitize_log_text` してから記録する。呼び出し側での直構築は避ける。
    pub fn command_start(spec: &CommandSpec) -> Self {
        Self::CommandStart {
            command: sanitize_log_text(&spec.program),
            args: spec.args.iter().map(|arg| sanitize_log_text(arg)).collect(),
            command_index: None,
            started_at: None,
            kind: None,
        }
    }

    pub fn command_start_span(
        spec: &CommandSpec,
        command_index: u32,
        started_at: &str,
        kind: CommandKind,
    ) -> Self {
        Self::CommandStart {
            command: sanitize_log_text(&spec.program),
            args: spec.args.iter().map(|arg| sanitize_log_text(arg)).collect(),
            command_index: Some(command_index),
            started_at: Some(started_at.to_string()),
            kind: Some(kind),
        }
    }

    pub fn shell_command_start(command_index: u32, started_at: &str, command_line: &str) -> Self {
        Self::CommandStart {
            command: sanitize_log_text(command_line),
            args: vec![],
            command_index: Some(command_index),
            started_at: Some(started_at.to_string()),
            kind: Some(CommandKind::Shell),
        }
    }

    pub fn stdout_indexed(data: &str, command_index: u32) -> Self {
        Self::Stdout {
            data: sanitize_log_text(data),
            command_index: Some(command_index),
        }
    }

    pub fn stderr_indexed(data: &str, command_index: u32) -> Self {
        Self::Stderr {
            data: sanitize_log_text(data),
            command_index: Some(command_index),
        }
    }

    pub fn command_end(command_index: u32, exit_code: Option<i32>, finished_at: &str) -> Self {
        Self::CommandEnd {
            command_index,
            exit_code,
            finished_at: finished_at.to_string(),
        }
    }
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

pub fn replay_list(
    events: &[LogEvent],
    index: Option<u32>,
    format: impl Into<OutputFormat>,
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
    Ok(render_list(&filtered, format.into()))
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
    let stdout = if span.kind == CommandKind::Shell {
        strip_shell_prompt_echo_prefix(&span.stdout, &span.command)
    } else {
        span.stdout
    };
    Ok(ensure_trailing_newline(stdout))
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Tsv,
    Json,
    Env,
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

fn strip_shell_prompt_echo_prefix(data: &str, command: &str) -> String {
    if command.is_empty() {
        return data.to_string();
    }
    let mut offset = 0;
    for line in data.split_inclusive('\n') {
        let display_line = line.trim_end_matches(['\r', '\n']);
        if line_looks_like_prompt_echo(display_line, command) {
            return data[offset + line.len()..].to_string();
        }
        offset += line.len();
    }
    data.to_string()
}

fn line_looks_like_prompt_echo(line: &str, command: &str) -> bool {
    let Some(prefix) = line.strip_suffix(command) else {
        return false;
    };
    let prompt = prefix.trim_end();
    prompt.ends_with('$') || prompt.ends_with('#') || prompt.ends_with('%') || prompt.ends_with('>')
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
            command,
            args,
            command_index,
            started_at,
            kind,
        } => {
            let Some(index) = command_index else {
                return;
            };
            let started_at = started_at.unwrap_or_default();
            let kind = kind.unwrap_or(CommandKind::Shell);
            spans.push(ReplaySpan {
                index,
                started_at,
                finished_at: None,
                exit_code: None,
                kind,
                command: format_command_preview(&command, &args),
                stdout: String::new(),
                stderr: String::new(),
                complete: false,
            });
        }
        LogEvent::Stdout {
            data,
            command_index: Some(index),
        } => {
            if let Some(span) = spans.iter_mut().rev().find(|span| span.index == index) {
                span.stdout.push_str(&data);
            }
        }
        LogEvent::Stderr {
            data,
            command_index: Some(index),
        } => {
            if let Some(span) = spans.iter_mut().rev().find(|span| span.index == index) {
                span.stderr.push_str(&data);
            }
        }
        LogEvent::CommandEnd {
            command_index,
            exit_code,
            finished_at,
        } => {
            if let Some(span) = spans
                .iter_mut()
                .rev()
                .find(|span| span.index == command_index)
            {
                span.finished_at = Some(finished_at);
                span.exit_code = exit_code;
                span.complete = true;
            }
        }
        LogEvent::Exit { .. } => {}
        LogEvent::Stdout {
            command_index: None,
            ..
        }
        | LogEvent::Stderr {
            command_index: None,
            ..
        } => {}
    }
}

fn format_command_preview(command: &str, args: &[String]) -> String {
    if args.is_empty() {
        command.to_string()
    } else {
        format!("{} {}", command, args.join(" "))
    }
}

/// ログ・コンテキストへ書く前に機微らしき部分を置換する。
pub fn sanitize_log_text(input: &str) -> String {
    static RE_SK: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"sk-[a-zA-Z0-9]{8,}").expect("regex"));
    static RE_BEARER: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)bearer\s+[a-zA-Z0-9._\-]+").expect("regex"));
    static RE_ENV_SECRET: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?m)([A-Za-z0-9_]*(?:KEY|TOKEN|SECRET)[A-Za-z0-9_]*)=([^\s\\]+)")
            .expect("regex")
    });
    static RE_AIZA: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"AIza[0-9A-Za-z_\-]{10,}").expect("regex"));

    let mut s = input.to_string();
    s = RE_SK.replace_all(&s, "sk-[REDACTED]").into_owned();
    s = RE_BEARER.replace_all(&s, "Bearer [REDACTED]").into_owned();
    s = RE_AIZA.replace_all(&s, "AIza[REDACTED]").into_owned();
    s = RE_ENV_SECRET.replace_all(&s, "$1=[REDACTED]").into_owned();
    s
}

/// 端末表示で次のプロンプトと連結しないよう、記録どおりの本文の末尾に改行がなければ付与する。
pub fn ensure_trailing_newline(mut data: String) -> String {
    if !data.is_empty() && !data.ends_with('\n') {
        data.push('\n');
    }
    data
}

pub fn rfc3339_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let time_of_day = secs % 86_400;
    let (year, month, day) = civil_from_days(days as i64);
    let hour = time_of_day / 3600;
    let minute = (time_of_day % 3600) / 60;
    let second = time_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}
