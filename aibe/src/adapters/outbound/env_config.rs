//! 環境変数ベースの設定アダプタ。

use std::path::PathBuf;

use crate::default_socket_path;
use crate::ports::outbound::{ConfigError, ConfigLoader, ServerConfig};

/// `AIBE_SOCKET_PATH` があればそれを使い、なければデフォルトパス。
pub struct EnvConfig;

impl EnvConfig {
    pub fn load() -> Result<ServerConfig, ConfigError> {
        Self::load_from_env()
    }
}

impl ConfigLoader for EnvConfig {
    fn load(&self) -> Result<ServerConfig, ConfigError> {
        Self::load_from_env()
    }
}

impl EnvConfig {
    fn load_from_env() -> Result<ServerConfig, ConfigError> {
        let socket_path = std::env::var("AIBE_SOCKET_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| default_socket_path());
        Ok(ServerConfig { socket_path })
    }
}
