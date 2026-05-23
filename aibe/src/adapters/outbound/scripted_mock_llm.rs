//! テスト用: あらかじめ積んだ応答を順に返す LLM。

use std::sync::Mutex;

use async_trait::async_trait;

use crate::domain::{ChatMessage, LlmStepResult};
use crate::ports::outbound::{LlmError, LlmProvider, ToolDefinition};

pub struct ScriptedMockLlm {
    steps: Mutex<Vec<LlmStepResult>>,
}

impl ScriptedMockLlm {
    pub fn new(steps: Vec<LlmStepResult>) -> Self {
        Self {
            steps: Mutex::new(steps),
        }
    }

    fn pop_step(&self) -> Result<LlmStepResult, LlmError> {
        let mut guard = self
            .steps
            .lock()
            .map_err(|e| LlmError::Provider(e.to_string()))?;
        if guard.is_empty() {
            return Err(LlmError::Provider(
                "ScriptedMockLlm: no more scripted steps".into(),
            ));
        }
        Ok(guard.remove(0))
    }
}

#[async_trait]
impl LlmProvider for ScriptedMockLlm {
    async fn complete(&self, _messages: &[ChatMessage]) -> Result<ChatMessage, LlmError> {
        let step = self.pop_step()?;
        Ok(step.assistant)
    }

    async fn complete_with_tools(
        &self,
        _messages: &[ChatMessage],
        _tools: &[ToolDefinition],
    ) -> Result<LlmStepResult, LlmError> {
        self.pop_step()
    }
}
