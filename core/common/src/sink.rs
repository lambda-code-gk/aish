//! イベント Sink の re-export（定義は ports/outbound/sink）

pub use crate::ports::outbound::sink::{AgentEvent, EventSink};

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
            args: serde_json::json!({"cmd": "ls"}),
            result: serde_json::json!({"ok": true}),
        };
        assert!(matches!(ev, AgentEvent::ToolResult { call_id, name, .. } if call_id == "c1" && name == "run"));
    }
}
