//! LLM イベントストリーム生成アダプタ（common::llm の resolve_provider / create_provider / LlmDriver を使用）

use std::sync::Arc;

use common::llm::factory::AnyProvider;
use common::llm::provider::Message as LlmMessage;
use common::llm::{create_provider, load_profiles_config, resolve_provider, LlmDriver, ResolvedProvider};
use common::ports::outbound::{EnvResolver, FileSystem};

use crate::ports::outbound::{LlmEventStream, LlmEventStreamFactory, LlmStreamContext};

fn llm_error_context(resolved: &ResolvedProvider) -> String {
    let mut extra: Vec<String> = Vec::new();
    if let Some(ref u) = resolved.base_url {
        extra.push(format!("base_url: {}", u));
    }
    if let Some(ref m) = resolved.model {
        extra.push(format!("model: {}", m));
    }
    if extra.is_empty() {
        format!("Provider profile: {}", resolved.profile_name)
    } else {
        format!("Provider profile: {} ({})", resolved.profile_name, extra.join(", "))
    }
}

/// LlmDriver を LlmEventStream として保持するアダプタ（所有で保持）
struct DriverLlmStreamAdapter(Arc<LlmDriver<AnyProvider>>);

impl LlmEventStream for DriverLlmStreamAdapter {
    fn stream_events(
        &self,
        query: &str,
        system_instruction: Option<&str>,
        history: &[LlmMessage],
        tools: Option<&[common::tool::ToolDef]>,
        callback: &mut dyn FnMut(common::llm::events::LlmEvent) -> Result<(), common::error::Error>,
    ) -> Result<(), common::error::Error> {
        self.0.query_stream_events(query, system_instruction, history, tools, callback)
    }
}

/// 標準 LLM ストリームファクトリ（common::llm でプロバイダ解決・ドライバ生成）
pub struct StdLlmEventStreamFactory {
    fs: Arc<dyn FileSystem>,
    env_resolver: Arc<dyn EnvResolver>,
}

impl StdLlmEventStreamFactory {
    pub fn new(fs: Arc<dyn FileSystem>, env_resolver: Arc<dyn EnvResolver>) -> Self {
        Self { fs, env_resolver }
    }
}

impl LlmEventStreamFactory for StdLlmEventStreamFactory {
    fn create_stream(
        &self,
        _session_dir: Option<&common::domain::SessionDir>,
        provider: Option<&common::domain::ProviderName>,
        model: Option<&common::domain::ModelName>,
        system_instruction: Option<&str>,
    ) -> Result<(Arc<dyn LlmEventStream>, LlmStreamContext), common::error::Error> {
        let _ = system_instruction;
        let cfg_opt = load_profiles_config(self.fs.as_ref(), self.env_resolver.as_ref())?;
        let resolved = resolve_provider(provider, cfg_opt.as_ref())?;
        let model_str = model
            .as_ref()
            .map(|m| m.as_ref().to_string())
            .or_else(|| resolved.model.clone());
        let ctx = llm_error_context(&resolved);
        let provider_inst = create_provider(
            resolved.provider_type,
            model_str,
            resolved.base_url.clone(),
            resolved.api_key_env.clone(),
            resolved.temperature,
        )
        .map_err(|e| e.with_context(ctx.clone()))?;
        let driver = Arc::new(LlmDriver::new(provider_inst));
        let stream: Arc<dyn LlmEventStream> = Arc::new(DriverLlmStreamAdapter(driver));
        Ok((stream, LlmStreamContext(ctx)))
    }
}
