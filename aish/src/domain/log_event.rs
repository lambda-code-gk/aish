//! JSONL ログイベント。

use crate::domain::sanitize_log_text;
use serde::{Deserialize, Serialize};

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

pub fn rfc3339_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // UTC RFC3339 without external chrono dependency.
    let days = secs / 86_400;
    let time_of_day = secs % 86_400;
    let (year, month, day) = civil_from_days(days as i64);
    let hour = time_of_day / 3600;
    let minute = (time_of_day % 3600) / 60;
    let second = time_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Algorithm from http://howardhinnant.github.io/date_algorithms.html (civil_from_days).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_start_masks_program_and_args() {
        let event = LogEvent::command_start(&CommandSpec {
            program: "curl".to_string(),
            args: vec![
                "-H".to_string(),
                "Authorization: Bearer abc.def".to_string(),
                "APP_SECRET=secret123".to_string(),
            ],
        });
        let LogEvent::CommandStart { command, args, .. } = event else {
            panic!("expected CommandStart");
        };
        assert_eq!(command, "curl");
        assert!(!args.iter().any(|a| a.contains("abc.def")));
        assert!(!args.iter().any(|a| a.contains("secret123")));
        assert!(args.iter().any(|a| a.contains("Bearer [REDACTED]")));
        assert!(args.iter().any(|a| a.contains("APP_SECRET=[REDACTED]")));
    }

    #[test]
    fn log_event_serde_is_backward_compatible() {
        let legacy = r#"{"event":"command_start","command":"echo","args":["hi"]}"#;
        let event: LogEvent = serde_json::from_str(legacy).expect("legacy deserialize");
        let LogEvent::CommandStart {
            command_index,
            started_at,
            kind,
            ..
        } = event
        else {
            panic!("expected CommandStart");
        };
        assert!(command_index.is_none());
        assert!(started_at.is_none());
        assert!(kind.is_none());

        let legacy_stdout = r#"{"event":"stdout","data":"hello"}"#;
        let event: LogEvent = serde_json::from_str(legacy_stdout).expect("stdout");
        let LogEvent::Stdout { command_index, .. } = event else {
            panic!("expected Stdout");
        };
        assert!(command_index.is_none());

        let legacy_exit = r#"{"event":"exit","code":0}"#;
        let event: LogEvent = serde_json::from_str(legacy_exit).expect("exit");
        assert!(matches!(event, LogEvent::Exit { code: Some(0) }));

        let span = LogEvent::command_start_span(
            &CommandSpec {
                program: "echo".to_string(),
                args: vec!["x".to_string()],
            },
            1,
            "2026-01-01T00:00:00Z",
            CommandKind::Exec,
        );
        let line = serde_json::to_string(&span).expect("serialize");
        let roundtrip: LogEvent = serde_json::from_str(&line).expect("roundtrip");
        assert_eq!(span, roundtrip);
    }
}
