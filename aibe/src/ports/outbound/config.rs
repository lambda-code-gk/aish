//! サーバ設定 outbound port。

use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub socket_path: PathBuf,
    pub llm: LlmConfig,
}

#[derive(Debug, Clone)]
pub enum LlmConfig {
    Mock,
    OpenAiCompatible {
        base_url: String,
        api_key: String,
        model: String,
    },
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("invalid configuration: {0}")]
    Invalid(String),
    #[error("failed to read config: {0}")]
    Io(String),
}

/// 設定の読み込み。
pub trait ConfigLoader {
    fn load(&self) -> Result<AppConfig, ConfigError>;
}
