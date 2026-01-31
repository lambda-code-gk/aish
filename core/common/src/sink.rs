//! ストリーミングの「消費」側（表示・保存）を分離する EventSink
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
        result: Value,
    },
    /// ツール実行エラー
    ToolError {
        call_id: String,
        name: String,
        message: String,
    },
}

/// イベントを受け取る Sink（表示・保存の責務を分離）
pub trait EventSink: Send {
    /// 1 イベントを処理（表示 or 永続化）
    fn on_event(&mut self, ev: &AgentEvent) -> Result<(), Error>;
    /// ストリーム終了時（オプションで flush 等）
    fn on_end(&mut self) -> Result<(), Error> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::events::LlmEvent;

    #[test]
    fn test_agent_event_llm() {
        let ev = AgentEvent::Llm(LlmEvent::TextDelta("hi".to_string()));
        assert!(matches!(ev, AgentEvent::Llm(LlmEvent::TextDelta(s)) if s == "hi"));
    }

    #[test]
    fn test_agent_event_tool_result() {
        let ev = AgentEvent::ToolResult {
            call_id: "c1".to_string(),
            name: "run".to_string(),
            result: serde_json::json!({"ok": true}),
        };
        assert!(matches!(ev, AgentEvent::ToolResult { call_id, name, .. } if call_id == "c1" && name == "run"));
    }
}
