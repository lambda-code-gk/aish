//! `LlmProfilesConfig` から起動時 `ProfileRegistry` を構築する。

use std::collections::HashMap;
use std::sync::Arc;

use crate::adapters::outbound::llm_backend::HttpBackendContext;
use crate::adapters::outbound::{GeminiLlm, MockLlm, OpenAiCompatibleLlm};
use crate::ports::outbound::{
    ConfigError, LlmBackend, LlmProfilesConfig, LlmProvider, LlmProviderKind, ProfileRegistry,
    TerminationCapability,
};

pub fn termination_capability_for_kind(kind: LlmProviderKind) -> TerminationCapability {
    match kind {
        LlmProviderKind::Mock | LlmProviderKind::OpenAiCompatible | LlmProviderKind::Gemini => {
            TerminationCapability::summary_prompt_only()
        }
    }
}

pub fn build_profile_registry(config: &LlmProfilesConfig) -> Result<ProfileRegistry, ConfigError> {
    if config.profiles.is_empty() {
        return Err(ConfigError::Invalid(
            "at least one [profiles.<name>] is required".into(),
        ));
    }
    if !config.profiles.contains_key(&config.default_profile) {
        return Err(ConfigError::Invalid(format!(
            "default_profile {:?} does not exist",
            config.default_profile
        )));
    }

    let mut http_backends: HashMap<String, Arc<HttpBackendContext>> = HashMap::new();
    let mut mock_backends: HashMap<String, Arc<dyn LlmProvider>> = HashMap::new();

    let mut providers = HashMap::new();
    let mut capabilities = HashMap::new();

    for (profile_name, profile) in &config.profiles {
        let backend = config.backends.get(&profile.llm).ok_or_else(|| {
            ConfigError::Invalid(format!(
                "profile {profile_name:?} references unknown llm {:?}",
                profile.llm
            ))
        })?;

        let provider: Arc<dyn LlmProvider> = match backend.provider {
            LlmProviderKind::Mock => mock_backends
                .entry(profile.llm.clone())
                .or_insert_with(|| Arc::new(MockLlm::new()) as Arc<dyn LlmProvider>)
                .clone(),
            LlmProviderKind::OpenAiCompatible => {
                let ctx = http_context(&mut http_backends, &profile.llm, backend)?;
                Arc::new(OpenAiCompatibleLlm::with_backend(
                    Arc::clone(&ctx),
                    profile.model.clone(),
                    profile.params.clone(),
                ))
            }
            LlmProviderKind::Gemini => {
                let ctx = http_context(&mut http_backends, &profile.llm, backend)?;
                Arc::new(GeminiLlm::with_backend(
                    Arc::clone(&ctx),
                    profile.model.clone(),
                    profile.params.clone(),
                ))
            }
        };

        capabilities.insert(
            profile_name.clone(),
            termination_capability_for_kind(backend.provider),
        );
        providers.insert(profile_name.clone(), provider);
    }

    Ok(ProfileRegistry {
        providers,
        capabilities,
        default_profile: config.default_profile.clone(),
    })
}

fn http_context(
    cache: &mut HashMap<String, Arc<HttpBackendContext>>,
    backend_name: &str,
    backend: &LlmBackend,
) -> Result<Arc<HttpBackendContext>, ConfigError> {
    if let Some(ctx) = cache.get(backend_name) {
        return Ok(Arc::clone(ctx));
    }
    let ctx = HttpBackendContext::new(backend.base_url.clone(), backend.api_key.clone());
    cache.insert(backend_name.to_string(), Arc::clone(&ctx));
    Ok(ctx)
}
