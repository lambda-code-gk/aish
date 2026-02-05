//! 標準環境変数解決実装（std::env を委譲）

use crate::domain::{HomeDir, SessionDir};
use crate::error::Error;
use crate::ports::outbound::EnvResolver;
use std::env;
use std::path::PathBuf;

const SYSTEM_D_SUBDIR: &str = "system.d";
const CONFIG_SYSTEM_D: &str = "config/system.d";

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

    fn current_dir(&self) -> Result<PathBuf, Error> {
        env::current_dir().map_err(|e| Error::io_msg(format!("current_dir: {}", e)))
    }

    fn resolve_global_system_d_dir(&self) -> Result<Option<PathBuf>, Error> {
        if let Ok(home) = env::var("AISH_HOME") {
            if !home.is_empty() {
                let mut p = PathBuf::from(&home);
                p.push(CONFIG_SYSTEM_D);
                return Ok(Some(p));
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
            });
        let mut path = config_base.ok_or_else(|| Error::env("HOME is not set"))?;
        path.push("aish");
        path.push(SYSTEM_D_SUBDIR);
        Ok(Some(path))
    }

    fn resolve_user_system_d_dir(&self) -> Result<Option<PathBuf>, Error> {
        let home = env::var("HOME").ok().filter(|s| !s.is_empty());
        let path = home.map(|h| PathBuf::from(h).join(".aish").join(SYSTEM_D_SUBDIR));
        Ok(path)
    }
}
