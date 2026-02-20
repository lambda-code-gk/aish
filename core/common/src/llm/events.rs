//! LLMストリームの共通イベント型
//!
//! プロバイダごとの差異をadapter層で吸収し、共通のイベント列に正規化する。

use serde::{Deserialize, Serialize};

/// ストリーム終了理由
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinishReason {
    /// 通常終了
    Stop,
    /// ツール呼び出しあり
    ToolCalls,
    /// 長さ制限
    Length,
    /// その他（プロバイダ固有）
    Other(String),
}

/// LLMストリームから来る正規化済みイベント
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LlmEvent {
    /// アシスタントテキストの増分
    TextDelta(String),
    /// 思考過程（推論）の増分（表示はグレーにする想定）
    ReasoningDelta(String),
    /// ツール呼び出し開始
    ToolCallBegin {
        call_id: String,
        name: String,
        /// Gemini 3 で必須の thought_signature（関数呼び出しの文脈を保持）
        thought_signature: Option<String>,
    },
    /// ツール引数（JSON断片）の増分
    ToolCallArgsDelta {
        call_id: String,
        json_fragment: String,
    },
    /// ツール呼び出し終了（この時点でargsをJSONとして解釈可能）
    ToolCallEnd { call_id: String },
    /// ストリーム完了
    Completed { finish: FinishReason },
    /// ストリーム失敗
    Failed { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_finish_reason_stop() {
        let r = FinishReason::Stop;
        assert_eq!(r, FinishReason::Stop);
    }

    #[test]
    fn test_llm_event_text_delta() {
        let ev = LlmEvent::TextDelta("hello".to_string());
        assert!(matches!(ev, LlmEvent::TextDelta(s) if s == "hello"));
    }

    #[test]
    fn test_llm_event_reasoning_delta() {
        let ev = LlmEvent::ReasoningDelta("thinking...".to_string());
        assert!(matches!(ev, LlmEvent::ReasoningDelta(s) if s == "thinking..."));
    }

    #[test]
    fn test_llm_event_tool_call_begin() {
        let ev = LlmEvent::ToolCallBegin {
            call_id: "call_1".to_string(),
            name: "run_shell".to_string(),
            thought_signature: Some("sig123".to_string()),
        };
        assert!(matches!(ev, LlmEvent::ToolCallBegin { call_id, name, thought_signature } if call_id == "call_1" && name == "run_shell" && thought_signature == Some("sig123".to_string())));
    }

    #[test]
    fn test_llm_event_completed() {
        let ev = LlmEvent::Completed {
            finish: FinishReason::Stop,
        };
        assert!(matches!(ev, LlmEvent::Completed { finish: FinishReason::Stop }));
    }
}
