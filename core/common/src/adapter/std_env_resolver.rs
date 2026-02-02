//! 標準環境変数解決実装（std::env を委譲）

use crate::domain::{HomeDir, SessionDir};
use crate::error::Error;
use crate::ports::outbound::EnvResolver;
use std::env;
use std::path::PathBuf;

/// 標準環境変数解決実装
#[derive(Debug, Clone, Default)]
pub struct StdEnvResolver;

impl EnvResolver for StdEnvResolver {
    fn session_dir_from_env(&self) -> Option<SessionDir> {
        env::var("AISH_SESSION")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .map(SessionDir::new)
    }

    fn resolve_home_dir(&self) -> Result<HomeDir, Error> {
        if let Ok(home) = env::var("AISH_HOME") {
            if !home.is_empty() {
                return Ok(HomeDir::new(PathBuf::from(home)));
            }
        }

        let config_base = env::var("XDG_CONFIG_HOME")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                env::var("HOME")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .map(|h| PathBuf::from(h).join(".config"))
            })
            .ok_or_else(|| Error::env("HOME is not set"))?;

        let mut path = config_base;
        path.push("aish");
        Ok(HomeDir::new(path))
    }
}
