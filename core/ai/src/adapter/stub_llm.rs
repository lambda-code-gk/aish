//! テスト用: 固定の LlmEvent 列を返す LlmEventStream 実装

#[cfg(test)]
mod stub {
    use common::error::Error;
    use common::llm::events::{FinishReason, LlmEvent};
    use common::llm::provider::Message;
    use common::tool::ToolDef;

    use crate::ports::outbound::LlmEventStream;

    /// テスト用: 固定の LlmEvent 列を返す Stub
    pub struct StubLlm {
        pub(super) events: Vec<LlmEvent>,
    }

    impl StubLlm {
        pub fn new(events: Vec<LlmEvent>) -> Self {
            Self { events }
        }

        pub fn text_only(text: &str) -> Self {
            Self::new(vec![
                LlmEvent::TextDelta(text.to_string()),
                LlmEvent::Completed {
                    finish: FinishReason::Stop,
                },
            ])
        }
    }

    impl LlmEventStream for StubLlm {
        fn stream_events(
            &self,
            _query: &str,
            _system_instruction: Option<&str>,
            _history: &[Message],
            _tools: Option<&[ToolDef]>,
            callback: &mut dyn FnMut(LlmEvent) -> Result<(), Error>,
        ) -> Result<(), Error> {
            for ev in &self.events {
                callback(ev.clone())?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
pub use stub::StubLlm;
