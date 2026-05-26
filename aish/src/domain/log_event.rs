//! JSONL ログイベント。

use crate::domain::sanitize_log_text;
use serde::{Deserialize, Serialize};

/// 1 コマンド実行の指定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
}

/// ログ 1 行（1 イベント）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum LogEvent {
    CommandStart { command: String, args: Vec<String> },
    Stdout { data: String },
    Stderr { data: String },
    Exit { code: Option<i32> },
}

impl LogEvent {
    /// `command` / `args` を `sanitize_log_text` してから記録する。呼び出し側での直構築は避ける。
    pub fn command_start(spec: &CommandSpec) -> Self {
        Self::CommandStart {
            command: sanitize_log_text(&spec.program),
            args: spec.args.iter().map(|arg| sanitize_log_text(arg)).collect(),
        }
    }
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
        let LogEvent::CommandStart { command, args } = event else {
            panic!("expected CommandStart");
        };
        assert_eq!(command, "curl");
        assert!(!args.iter().any(|a| a.contains("abc.def")));
        assert!(!args.iter().any(|a| a.contains("secret123")));
        assert!(args.iter().any(|a| a.contains("Bearer [REDACTED]")));
        assert!(args.iter().any(|a| a.contains("APP_SECRET=[REDACTED]")));
    }
}
