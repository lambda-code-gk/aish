//! 標準パス解決実装（環境変数・CLI オプションに基づく）
//!
//! ホームディレクトリ・セッションディレクトリの解決を adapter 層で行う。
//! セッション配下パスは EnvResolver::resolve_dirs() の Dirs に統一する。

use crate::error::Error;
use crate::ports::outbound::{EnvResolver, PathResolver, PathResolverInput};
use std::env;
use std::path::PathBuf;
use std::sync::Arc;

/// 標準パス解決実装（環境変数・CLI に基づく）
/// セッション新規作成時は EnvResolver::resolve_dirs() の sessions_dir を使用する。
pub struct StdPathResolver {
    env_resolver: Arc<dyn EnvResolver>,
}

impl StdPathResolver {
    pub fn new(env_resolver: Arc<dyn EnvResolver>) -> Self {
        Self { env_resolver }
    }
}

impl PathResolver for StdPathResolver {
    /// ホームディレクトリ（論理的な AISH_HOME）を解決する
    ///
    /// 優先順位:
    /// 1. コマンドラインオプション -d/--home-dir
    /// 2. 環境変数 AISH_HOME
    /// 3. XDG_CONFIG_HOME/aish （未設定時は ~/.config/aish）
    fn resolve_home_dir(&self, input: &PathResolverInput) -> Result<String, Error> {
        if let Some(ref home) = input.home_dir {
            return Ok(home.clone());
        }

        if let Ok(env_home) = env::var("AISH_HOME") {
            if !env_home.is_empty() {
                return Ok(env_home);
            }
        }

        let xdg_config_home = env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| {
            let mut path = dirs_home().unwrap_or_else(|| PathBuf::from("~"));
            path.push(".config");
            path.to_string_lossy().to_string()
        });

        Ok(format!("{}/aish", xdg_config_home.trim_end_matches('/')))
    }

    /// セッションディレクトリを解決する
    ///
    /// 優先順位:
    /// 1. コマンドラインオプション -s/--session-dir（指定ディレクトリをそのまま使用・再開用）
    /// 2. 環境変数 AISH_SESSION（既存セッションを参照する場合）
    /// 3. それ以外: Dirs（EnvResolver::resolve_dirs）の sessions_dir に新規 ID を生成
    ///    （AISH_HOME または -d 設定時はその配下、なければ XDG）
    fn resolve_session_dir(
        &self,
        input: &PathResolverInput,
        _home_dir: &str,
    ) -> Result<String, Error> {
        // 1. CLI オプション -s/--session-dir が最優先
        if let Some(ref session_dir) = input.session_dir {
            return Ok(session_dir.clone());
        }

        // 2. 環境変数 AISH_SESSION
        if let Ok(env_session) = env::var("AISH_SESSION") {
            if !env_session.is_empty() {
                return Ok(env_session);
            }
        }

        // 3. Dirs 経由で新規セッションディレクトリを生成（AISH_HOME/XDG は resolve_dirs に集約）
        let dirs = self.env_resolver.resolve_dirs()?;
        let path = dirs.sessions_dir().join(generate_session_dirname());
        Ok(path.to_string_lossy().to_string())
    }
}

/// ホームディレクトリ (~) を取得する簡易ヘルパ
fn dirs_home() -> Option<PathBuf> {
    if let Ok(home) = env::var("HOME") {
        if !home.is_empty() {
            return Some(PathBuf::from(home));
        }
    }
    None
}

/// セッションディレクトリ名を生成する
///
/// 形式: base64urlエンコードされた48bit時刻（ミリ秒）8文字
fn generate_session_dirname() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    const BASE64URL: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();

    let ms_since_epoch = dur.as_millis() as u64;
    let ts48 = ms_since_epoch & ((1u64 << 48) - 1);

    let mut buf = [0u8; 8];
    for i in 0..8 {
        let shift = 6 * (7 - i);
        let index = ((ts48 >> shift) & 0x3F) as usize;
        buf[i] = BASE64URL[index];
    }

    String::from_utf8_lossy(&buf).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::StdEnvResolver;
    use crate::ports::outbound::EnvResolver;

    fn resolver() -> StdPathResolver {
        StdPathResolver::new(Arc::new(StdEnvResolver))
    }

    fn with_env_var<F: FnOnce()>(key: &str, value: Option<&str>, f: F) {
        let original = env::var(key).ok();
        match value {
            Some(v) => env::set_var(key, v),
            None => env::remove_var(key),
        }
        f();
        match original {
            Some(v) => env::set_var(key, v),
            None => env::remove_var(key),
        }
    }

    #[test]
    fn test_resolve_home_dir_prefers_cli() {
        let resolver = resolver();
        let input = PathResolverInput {
            home_dir: Some("/tmp/aish_cli_home".to_string()),
            ..Default::default()
        };

        with_env_var("AISH_HOME", Some("/tmp/aish_env_home"), || {
            let home = resolver.resolve_home_dir(&input).unwrap();
            assert_eq!(home, "/tmp/aish_cli_home");
        });
    }

    #[test]
    fn test_resolve_home_dir_uses_aish_home_env() {
        let resolver = resolver();
        let input = PathResolverInput::default();

        with_env_var("AISH_HOME", Some("/tmp/aish_env_home2"), || {
            let home = resolver.resolve_home_dir(&input).unwrap();
            assert_eq!(home, "/tmp/aish_env_home2");
        });
    }

    #[test]
    fn test_resolve_home_dir_uses_xdg_config_home() {
        let resolver = resolver();
        let input = PathResolverInput::default();

        with_env_var("AISH_HOME", None, || {
            with_env_var("XDG_CONFIG_HOME", Some("/tmp/xdg_config"), || {
                let home = resolver.resolve_home_dir(&input).unwrap();
                assert_eq!(home, "/tmp/xdg_config/aish");
            });
        });
    }

    #[test]
    fn test_resolve_session_dir_prefers_cli() {
        let resolver = resolver();
        let input = PathResolverInput {
            session_dir: Some("/tmp/aish_session_cli".to_string()),
            ..Default::default()
        };
        let home_dir = "/tmp/aish_home_any".to_string();

        let session = resolver.resolve_session_dir(&input, &home_dir).unwrap();
        assert_eq!(session, "/tmp/aish_session_cli");
    }

    #[test]
    fn test_resolve_session_dir_under_explicit_home() {
        let resolver = resolver();
        let input = PathResolverInput {
            home_dir: Some("/tmp/aish_cli_home3".to_string()),
            ..Default::default()
        };
        let home_dir = resolver.resolve_home_dir(&input).unwrap();

        // 新規セッションは resolve_dirs() の sessions_dir。AISH_HOME を設定するとその配下になる
        with_env_var("AISH_SESSION", None, || {
            with_env_var("AISH_HOME", Some("/tmp/aish_cli_home3"), || {
                with_env_var("HOME", Some("/tmp/fallback"), || {
                    let session = resolver.resolve_session_dir(&input, &home_dir).unwrap();
                    let prefix = "/tmp/aish_cli_home3/state/session/";
                    assert!(session.starts_with(prefix));

                    let suffix = &session[prefix.len()..];
                    assert_eq!(suffix.len(), 8);
                    for c in suffix.chars() {
                        assert!(
                            ('A'..='Z').contains(&c)
                                || ('a'..='z').contains(&c)
                                || ('0'..='9').contains(&c)
                                || c == '-'
                                || c == '_',
                            "invalid base64url char in session id: {}",
                            c
                        );
                    }
                });
            });
        });
    }

    #[test]
    fn test_resolve_session_dir_uses_xdg_state_home() {
        let resolver = resolver();
        let input = PathResolverInput::default();

        with_env_var("AISH_SESSION", None, || {
            with_env_var("AISH_HOME", None, || {
                with_env_var("XDG_STATE_HOME", Some("/tmp/xdg_state"), || {
                    let home_dir = "/tmp/some_home".to_string();
                    let session = resolver.resolve_session_dir(&input, &home_dir).unwrap();
                    let prefix = "/tmp/xdg_state/aish/session/";
                    assert!(session.starts_with(prefix), "session={}", session);
                    let suffix = &session[prefix.len()..];
                    assert_eq!(suffix.len(), 8);
                    for c in suffix.chars() {
                        assert!(
                            ('A'..='Z').contains(&c)
                                || ('a'..='z').contains(&c)
                                || ('0'..='9').contains(&c)
                                || c == '-'
                                || c == '_',
                            "invalid base64url char in session id: {}",
                            c
                        );
                    }
                });
            });
        });
    }

    #[test]
    fn test_generate_session_dirname_format() {
        // generate_session_dirname is private, test via resolve_session_dir
        let resolver = resolver();
        let input = PathResolverInput::default();
        with_env_var("AISH_SESSION", None, || {
            with_env_var("AISH_HOME", Some("/tmp/test"), || {
                with_env_var("HOME", Some("/tmp/fallback"), || {
                    let session = resolver
                        .resolve_session_dir(&input, "/tmp/test")
                        .unwrap();
                    let suffix = session.strip_prefix("/tmp/test/state/session/").unwrap();
                    assert_eq!(suffix.len(), 8);
                    for c in suffix.chars() {
                        assert!(
                            ('A'..='Z').contains(&c)
                                || ('a'..='z').contains(&c)
                                || ('0'..='9').contains(&c)
                                || c == '-'
                                || c == '_',
                            "invalid base64url char in session dirname: {}",
                            c
                        );
                    }
                });
            });
        });
    }

    #[test]
    fn test_resolve_session_dir_uses_aish_session_env() {
        let resolver = resolver();
        let input = PathResolverInput::default();
        let home_dir = "/tmp/some_home".to_string();

        with_env_var("AISH_SESSION", Some("/tmp/aish_session_from_env"), || {
            with_env_var("AISH_HOME", None, || {
                let session = resolver.resolve_session_dir(&input, &home_dir).unwrap();
                assert_eq!(session, "/tmp/aish_session_from_env");
            });
        });
    }

    #[test]
    fn test_resolve_session_dir_cli_overrides_aish_session_env() {
        let resolver = resolver();
        let input = PathResolverInput {
            session_dir: Some("/tmp/aish_session_cli".to_string()),
            ..Default::default()
        };
        let home_dir = "/tmp/some_home".to_string();

        with_env_var("AISH_SESSION", Some("/tmp/aish_session_from_env"), || {
            let session = resolver.resolve_session_dir(&input, &home_dir).unwrap();
            assert_eq!(session, "/tmp/aish_session_cli");
        });
    }
}
