//! 単発 LLM 完了の標準実装（ストリームを最後まで受け取り全文を返す）

use common::error::Error;
use common::llm::events::LlmEvent;
use std::sync::Arc;

use crate::ports::outbound::{LlmCompletion, LlmEventStreamFactory};

/// 標準の単発完了アダプタ（LlmEventStreamFactory でストリームを生成し、TextDelta を結合）
pub struct StdLlmCompletion {
    stream_factory: Arc<dyn LlmEventStreamFactory>,
}

impl StdLlmCompletion {
    pub fn new(stream_factory: Arc<dyn LlmEventStreamFactory>) -> Self {
        Self { stream_factory }
    }
}

impl LlmCompletion for StdLlmCompletion {
    fn complete(&self, system_instruction: Option<&str>, user_message: &str) -> Result<String, Error> {
        let (stream, _ctx) = self
            .stream_factory
            .create_stream(None, None, None, system_instruction)?;
        let mut out = String::new();
        let mut err_msg: Option<String> = None;
        stream.stream_events(
            user_message,
            system_instruction,
            &[],
            None,
            &mut |ev| {
                match ev {
                    LlmEvent::TextDelta(s) => out.push_str(&s),
                    LlmEvent::Completed { .. } => {}
                    LlmEvent::Failed { message } => err_msg = Some(message),
                    _ => {}
                }
                Ok(())
            },
        )?;
        if let Some(m) = err_msg {
            return Err(Error::invalid_argument(format!("LLM failed: {}", m)));
        }
        Ok(out)
    }
}
