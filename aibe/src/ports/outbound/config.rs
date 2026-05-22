//! サーバ設定 outbound port。

use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub socket_path: PathBuf,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("invalid configuration: {0}")]
    Invalid(String),
}

/// 設定の読み込み。
pub trait ConfigLoader {
    fn load(&self) -> Result<ServerConfig, ConfigError>;
}
