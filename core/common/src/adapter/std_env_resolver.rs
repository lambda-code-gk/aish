//! 標準環境変数解決実装（std::env を委譲）

use crate::domain::{Dirs, HomeDir, SessionDir};
use crate::error::Error;
use crate::ports::outbound::EnvResolver;
use std::env;
use std::path::PathBuf;

const SYSTEM_D_SUBDIR: &str = "system.d";
const CONFIG_SYSTEM_D: &str = "config/system.d";
const PROFILES_CONFIG_FILENAME: &str = "profiles.json";
const CONFIG_PROFILES: &str = "config/profiles.json";
const LOG_FILENAME: &str = "log.jsonl";
const TRANSCRIPT_FILENAME: &str = "transcript.jsonl";
const STATE_SUBDIR: &str = "state";
const CONFIG_SUBDIR: &str = "config";
const DATA_SUBDIR: &str = "data";
const CACHE_SUBDIR: &str = "cache";
const AISH_SUBDIR: &str = "aish";

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
        let dirs = self.resolve_dirs()?;
        Ok(dirs.state_dir.join(LOG_FILENAME))
    }

    fn resolve_transcript_file_path(&self) -> Result<PathBuf, Error> {
        let dirs = self.resolve_dirs()?;
        Ok(dirs.state_dir.join(TRANSCRIPT_FILENAME))
    }

    fn resolve_dirs(&self) -> Result<Dirs, Error> {
        let home = env::var("HOME")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| Error::env("HOME is not set"))?;

        if let Ok(aish_home) = env::var("AISH_HOME") {
            if !aish_home.is_empty() {
                let base = PathBuf::from(aish_home);
                return Ok(Dirs {
                    config_dir: base.join(CONFIG_SUBDIR),
                    data_dir: base.join(DATA_SUBDIR),
                    state_dir: base.join(STATE_SUBDIR),
                    cache_dir: base.join(CACHE_SUBDIR),
                });
            }
        }

        let config_dir = env::var("XDG_CONFIG_HOME")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        let data_dir = env::var("XDG_DATA_HOME")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local").join("share"));
        let state_dir = env::var("XDG_STATE_HOME")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local").join("state"));
        let cache_dir = env::var("XDG_CACHE_HOME")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".cache"));

        Ok(Dirs {
            config_dir: config_dir.join(AISH_SUBDIR),
            data_dir: data_dir.join(AISH_SUBDIR),
            state_dir: state_dir.join(AISH_SUBDIR),
            cache_dir: cache_dir.join(AISH_SUBDIR),
        })
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

    #[test]
    fn test_resolve_transcript_file_path() {
        let r = StdEnvResolver;
        // 1) AISH_HOME が設定されていれば state/transcript.jsonl を返す
        with_env_var("AISH_HOME", None, || {
            with_env_var("XDG_STATE_HOME", None, || {
                with_env_var("AISH_HOME", Some("/tmp/aish_transcript_home"), || {
                    let path = r.resolve_transcript_file_path().unwrap();
                    assert_eq!(
                        path.to_string_lossy(),
                        "/tmp/aish_transcript_home/state/transcript.jsonl"
                    );
                });
            });
        });
        // 2) XDG のみの場合は XDG_STATE_HOME/aish/transcript.jsonl
        with_env_var("AISH_HOME", None, || {
            with_env_var("XDG_STATE_HOME", Some("/tmp/xdg_state_transcript"), || {
                with_env_var("HOME", Some("/tmp/fallback"), || {
                    let path = r.resolve_transcript_file_path().unwrap();
                    assert_eq!(
                        path.to_string_lossy(),
                        "/tmp/xdg_state_transcript/aish/transcript.jsonl"
                    );
                });
            });
        });
    }

    #[test]
    fn test_resolve_dirs_aish_home() {
        let r = StdEnvResolver;
        with_env_var("AISH_HOME", None, || {
            with_env_var("HOME", Some("/home/user"), || {
                with_env_var("AISH_HOME", Some("/opt/aish_home"), || {
                    let dirs = r.resolve_dirs().unwrap();
                    assert_eq!(dirs.config_dir.to_string_lossy(), "/opt/aish_home/config");
                    assert_eq!(dirs.data_dir.to_string_lossy(), "/opt/aish_home/data");
                    assert_eq!(dirs.state_dir.to_string_lossy(), "/opt/aish_home/state");
                    assert_eq!(dirs.cache_dir.to_string_lossy(), "/opt/aish_home/cache");
                    assert_eq!(
                        dirs.sessions_dir().to_string_lossy(),
                        "/opt/aish_home/state/session"
                    );
                });
            });
        });
    }

    #[test]
    fn test_resolve_dirs_xdg() {
        let r = StdEnvResolver;
        with_env_var("AISH_HOME", None, || {
            with_env_var("XDG_CONFIG_HOME", Some("/xdg/config"), || {
                with_env_var("XDG_DATA_HOME", Some("/xdg/data"), || {
                    with_env_var("XDG_STATE_HOME", Some("/xdg/state"), || {
                        with_env_var("XDG_CACHE_HOME", Some("/xdg/cache"), || {
                            with_env_var("HOME", Some("/home/user"), || {
                                let dirs = r.resolve_dirs().unwrap();
                                assert_eq!(dirs.config_dir.to_string_lossy(), "/xdg/config/aish");
                                assert_eq!(dirs.data_dir.to_string_lossy(), "/xdg/data/aish");
                                assert_eq!(dirs.state_dir.to_string_lossy(), "/xdg/state/aish");
                                assert_eq!(dirs.cache_dir.to_string_lossy(), "/xdg/cache/aish");
                            });
                        });
                    });
                });
            });
        });
    }
}
