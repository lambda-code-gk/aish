//! 標準環境変数解決実装（std::env を委譲）

use crate::domain::{HomeDir, SessionDir};
use crate::error::Error;
use crate::ports::outbound::EnvResolver;
use std::env;
use std::path::PathBuf;

const SYSTEM_D_SUBDIR: &str = "system.d";
const CONFIG_SYSTEM_D: &str = "config/system.d";
const PROFILES_CONFIG_FILENAME: &str = "profiles.json";
const CONFIG_PROFILES: &str = "config/profiles.json";
const LOG_FILENAME: &str = "log.jsonl";
const STATE_SUBDIR: &str = "state";

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

    fn resolve_profiles_config_path(&self) -> Result<PathBuf, Error> {
        if let Ok(home) = env::var("AISH_HOME") {
            if !home.is_empty() {
                let mut p = PathBuf::from(home);
                p.push(CONFIG_PROFILES);
                return Ok(p);
            }
        }
        let home_dir = self.resolve_home_dir()?;
        Ok(home_dir.as_ref().join(PROFILES_CONFIG_FILENAME))
    }

    fn resolve_log_file_path(&self) -> Result<PathBuf, Error> {
        if let Ok(home) = env::var("AISH_HOME") {
            if !home.is_empty() {
                let mut p = PathBuf::from(home);
                p.push(STATE_SUBDIR);
                p.push(LOG_FILENAME);
                return Ok(p);
            }
        }
        let xdg_state_home = env::var("XDG_STATE_HOME")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                env::var("HOME")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .map(|h| PathBuf::from(h).join(".local").join("state"))
            })
            .ok_or_else(|| Error::env("HOME is not set"))?;
        let mut path = xdg_state_home;
        path.push("aish");
        path.push(LOG_FILENAME);
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_env_var<F: FnOnce()>(key: &str, value: Option<&str>, f: F) {
        let prev = std::env::var(key).ok();
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
        f();
        if let Some(p) = prev {
            std::env::set_var(key, p);
        } else {
            std::env::remove_var(key);
        }
    }

    /// 環境変数に依存するため 1 本のテストで順に実行し、並列時の競合を避ける
    #[test]
    fn test_resolve_log_file_path() {
        let r = StdEnvResolver;
        // 1) AISH_HOME が設定されていれば state/log.jsonl を返す
        with_env_var("AISH_HOME", None, || {
            with_env_var("XDG_STATE_HOME", None, || {
                with_env_var("AISH_HOME", Some("/tmp/aish_log_home"), || {
                    let path = r.resolve_log_file_path().unwrap();
                    assert_eq!(
                        path.to_string_lossy(),
                        "/tmp/aish_log_home/state/log.jsonl"
                    );
                });
            });
        });
        // 2) XDG のみの場合は XDG_STATE_HOME/aish/log.jsonl
        with_env_var("AISH_HOME", None, || {
            with_env_var("XDG_STATE_HOME", Some("/tmp/xdg_state_log"), || {
                with_env_var("HOME", Some("/tmp/fallback"), || {
                    let path = r.resolve_log_file_path().unwrap();
                    assert_eq!(
                        path.to_string_lossy(),
                        "/tmp/xdg_state_log/aish/log.jsonl"
                    );
                });
            });
        });
    }
}
