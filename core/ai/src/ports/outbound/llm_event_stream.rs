//! LLM イベントストリーム Outbound ポート
//!
//! テストでは StubLlm で差し替え可能。

use common::error::Error;
use common::llm::events::LlmEvent;
use common::llm::provider::Message;
use common::tool::ToolDef;

/// LLM ストリームを LlmEvent 列で受け取る Outbound ポート
pub trait LlmEventStream: Send {
    fn stream_events(
        &self,
        query: &str,
        system_instruction: Option<&str>,
        history: &[Message],
        tools: Option<&[ToolDef]>,
        callback: &mut dyn FnMut(LlmEvent) -> Result<(), Error>,
    ) -> Result<(), Error>;
}
