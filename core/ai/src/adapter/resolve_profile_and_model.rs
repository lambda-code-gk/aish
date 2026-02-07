//! プロファイル・モデル解決アダプタ（common::llm の resolve_provider を使用）

use std::sync::Arc;

use common::llm::{load_profiles_config, resolve_provider};
use common::ports::outbound::{EnvResolver, FileSystem};

use crate::ports::outbound::ResolveProfileAndModel;

/// 標準プロファイル・モデル解決（profiles.json + resolve_provider）
pub struct StdResolveProfileAndModel {
    fs: Arc<dyn FileSystem>,
    env_resolver: Arc<dyn EnvResolver>,
}

impl StdResolveProfileAndModel {
    pub fn new(fs: Arc<dyn FileSystem>, env_resolver: Arc<dyn EnvResolver>) -> Self {
        Self { fs, env_resolver }
    }
}

impl ResolveProfileAndModel for StdResolveProfileAndModel {
    fn resolve(
        &self,
        provider: Option<&common::domain::ProviderName>,
        model: Option<&common::domain::ModelName>,
    ) -> Result<(String, String), common::error::Error> {
        let cfg_opt = load_profiles_config(self.fs.as_ref(), self.env_resolver.as_ref())?;
        let resolved = resolve_provider(provider, cfg_opt.as_ref())?;
        let model_str = model
            .as_ref()
            .map(|m| m.as_ref().to_string())
            .or_else(|| resolved.model.clone());
        let profile_name = resolved.profile_name;
        let model_name = model_str.unwrap_or_else(|| "(default)".to_string());
        Ok((profile_name, model_name))
    }
}
