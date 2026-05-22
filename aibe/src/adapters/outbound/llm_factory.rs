//! `AppConfig` から `LlmProvider` を構築する。

use std::sync::Arc;

use crate::adapters::outbound::{MockLlm, OpenAiCompatibleLlm};
use crate::ports::outbound::{AppConfig, ConfigError, LlmConfig, LlmProvider};

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
    };
    Ok(provider)
}
