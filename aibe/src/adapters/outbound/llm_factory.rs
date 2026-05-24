//! `AppConfig` から `LlmProvider` を構築する。

use std::sync::Arc;

use crate::adapters::outbound::{GeminiLlm, MockLlm, OpenAiCompatibleLlm};
use crate::ports::outbound::{
    AppConfig, ConfigError, LlmConfig, LlmProvider, TerminationCapability,
};

/// LLM プロバイダ種別に応じた終端 capability（`LlmProvider` trait には載せない）。
pub fn termination_capability(llm: &LlmConfig) -> TerminationCapability {
    match llm {
        LlmConfig::Mock => TerminationCapability::summary_prompt_only(),
        // OpenAI 互換は安全側: tool role を plain complete で送らない。
        LlmConfig::OpenAiCompatible { .. } => TerminationCapability::summary_prompt_only(),
        LlmConfig::Gemini { .. } => TerminationCapability::summary_prompt_only(),
    }
}

pub fn build_llm(config: &AppConfig) -> Result<Arc<dyn LlmProvider>, ConfigError> {
    let provider: Arc<dyn LlmProvider> = match &config.llm {
        LlmConfig::Mock => Arc::new(MockLlm::new()),
        LlmConfig::OpenAiCompatible {
            base_url,
            api_key,
            model,
        } => Arc::new(OpenAiCompatibleLlm::new(
            base_url.clone(),
            api_key.clone(),
            model.clone(),
        )),
        LlmConfig::Gemini {
            base_url,
            api_key,
            model,
        } => Arc::new(GeminiLlm::new(
            base_url.clone(),
            api_key.clone(),
            model.clone(),
        )),
    };
    Ok(provider)
}
