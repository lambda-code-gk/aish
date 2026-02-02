//! イベント Sink Outbound ポート
//!
//! AgentEvent を受け取り、stdout 表示・JSONL ログ・part ファイル保存などに振り分ける。

use crate::error::Error;
use crate::llm::events::LlmEvent;
use serde_json::Value;

/// AgentLoop から Sink へ流すイベント
#[derive(Debug, Clone, PartialEq)]
pub enum AgentEvent {
    /// LLM ストリーム由来
    Llm(LlmEvent),
    /// ツール実行結果
    ToolResult {
        call_id: String,
        name: String,
        args: Value,
        result: Value,
    },
    /// ツール実行エラー
    ToolError {
        call_id: String,
        name: String,
        args: Value,
        message: String,
    },
}

/// イベントを受け取る Sink（Outbound ポート）
pub trait EventSink: Send + Sync {
    /// 1 イベントを処理（表示 or 永続化）
    fn on_event(&mut self, ev: &AgentEvent) -> Result<(), Error>;
    /// ストリーム終了時（オプションで flush 等）
    fn on_end(&mut self) -> Result<(), Error> {
        Ok(())
    }
}
