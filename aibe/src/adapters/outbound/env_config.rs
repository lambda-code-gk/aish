//! 環境変数のみの設定（後方互換）。新規は [`TomlConfig`] を使う。

use std::path::PathBuf;

use crate::ports::outbound::{
    default_conversation_store_root_with_home, AppConfig, ConfigError, ConfigLoader,
    LlmProfilesConfig, RouterConfig, ToolsConfig,
};
use aibe_client::default_socket_path;

/// `AIBE_SOCKET_PATH` のみ。LLM は mock。
pub struct EnvConfig;

impl EnvConfig {
    pub fn load() -> Result<AppConfig, ConfigError> {
        Self::load_from_env()
    }

    fn load_from_env() -> Result<AppConfig, ConfigError> {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let socket_path = std::env::var("AIBE_SOCKET_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| default_socket_path());
        Ok(AppConfig {
            socket_path,
            conversation_store_root: default_conversation_store_root_with_home(&home),
            router: RouterConfig::default(),
            llm: LlmProfilesConfig::default_mock(),
            tools: ToolsConfig::default(),
            external_commands: Vec::new(),
        })
    }
}

impl ConfigLoader for EnvConfig {
    fn load(&self) -> Result<AppConfig, ConfigError> {
        Self::load_from_env()
    }
}
