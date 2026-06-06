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

/// テスト用: `complete_streaming` が複数 delta を emit する LLM。
pub struct DeltaStreamingMockLlm {
    deltas: Vec<String>,
    final_content: String,
}

impl DeltaStreamingMockLlm {
    pub fn new(deltas: Vec<String>, final_content: impl Into<String>) -> Self {
        Self {
            deltas,
            final_content: final_content.into(),
        }
    }
}

#[async_trait]
impl LlmProvider for DeltaStreamingMockLlm {
    async fn complete(&self, _messages: &[ChatMessage]) -> Result<ChatMessage, LlmError> {
        Ok(ChatMessage::assistant(self.final_content.clone()))
    }

    async fn complete_with_tools(
        &self,
        _messages: &[ChatMessage],
        _tools: &[ToolDefinition],
    ) -> Result<LlmStepResult, LlmError> {
        Ok(LlmStepResult::text_only(self.final_content.clone()))
    }

    async fn complete_streaming(
        &self,
        _messages: &[ChatMessage],
        on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<ChatMessage, LlmError> {
        for delta in &self.deltas {
            on_delta(delta.clone());
        }
        Ok(ChatMessage::assistant(self.final_content.clone()))
    }
}
