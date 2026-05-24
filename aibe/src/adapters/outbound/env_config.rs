//! 環境変数のみの設定（後方互換）。新規は [`TomlConfig`] を使う。

use std::path::PathBuf;

use crate::default_socket_path;
use crate::ports::outbound::{
    AppConfig, ConfigError, ConfigLoader, LlmProfilesConfig, ToolsConfig,
};

/// `AIBE_SOCKET_PATH` のみ。LLM は mock。
pub struct EnvConfig;

impl EnvConfig {
    pub fn load() -> Result<AppConfig, ConfigError> {
        Self::load_from_env()
    }

    fn load_from_env() -> Result<AppConfig, ConfigError> {
        let socket_path = std::env::var("AIBE_SOCKET_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| default_socket_path());
        Ok(AppConfig {
            socket_path,
            llm: LlmProfilesConfig::default_mock(),
            tools: ToolsConfig::default(),
        })
    }
}

impl ConfigLoader for EnvConfig {
    fn load(&self) -> Result<AppConfig, ConfigError> {
        Self::load_from_env()
    }
}
