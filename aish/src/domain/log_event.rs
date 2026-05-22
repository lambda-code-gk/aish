//! JSONL ログイベント。

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
    pub fn command_start(spec: &CommandSpec) -> Self {
        Self::CommandStart {
            command: spec.program.clone(),
            args: spec.args.clone(),
        }
    }
}
