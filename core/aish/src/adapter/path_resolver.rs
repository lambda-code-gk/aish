//! パス解決（環境変数・CLI オプションに基づく）
//!
//! ホームディレクトリ・セッションディレクトリの解決を adapter 層で行う。
//! usecase は解決済みのパスを受け取り、ロジックに集中する。

use common::error::Error;
use std::env;
use std::path::PathBuf;

/// パス解決の入力（CLI の home_dir / session_dir オプション）
#[derive(Debug, Clone, Default)]
pub struct PathResolverInput {
    pub home_dir: Option<String>,
    pub session_dir: Option<String>,
}

/// ホームディレクトリ（論理的な AISH_HOME）を解決する
///
/// 優先順位:
/// 1. コマンドラインオプション -d/--home-dir
/// 2. 環境変数 AISH_HOME
/// 3. XDG_CONFIG_HOME/aish （未設定時は ~/.config/aish）
pub fn resolve_home_dir(input: &PathResolverInput) -> Result<String, Error> {
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
/// 2. 環境変数 AISH_HOME が設定されている場合: その配下の state/session/{ユニークID}
/// 3. XDG_STATE_HOME（未設定時は ~/.local/state）の aish/session/{ユニークID}
pub fn resolve_session_dir(
    input: &PathResolverInput,
    home_dir: &str,
) -> Result<String, Error> {
    if let Some(ref session_dir) = input.session_dir {
        return Ok(session_dir.clone());
    }

    let env_home = env::var("AISH_HOME").ok().filter(|h| !h.is_empty());
    if input.home_dir.is_some() || env_home.is_some() {
        let mut path = PathBuf::from(home_dir);
        path.push("state");
        path.push("session");
        path.push(generate_session_dirname());
        return Ok(path.to_string_lossy().to_string());
    }

    let xdg_state_home = env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let mut path = dirs_home().unwrap_or_else(|| PathBuf::from("~"));
        path.push(".local");
        path.push("state");
        path.to_string_lossy().to_string()
    });

    let mut path = PathBuf::from(xdg_state_home.trim_end_matches('/'));
    path.push("aish");
    path.push("session");
    path.push(generate_session_dirname());

    Ok(path.to_string_lossy().to_string())
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
pub fn generate_session_dirname() -> String {
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
        let input = PathResolverInput {
            home_dir: Some("/tmp/aish_cli_home".to_string()),
            ..Default::default()
        };

        with_env_var("AISH_HOME", Some("/tmp/aish_env_home"), || {
            let home = resolve_home_dir(&input).unwrap();
            assert_eq!(home, "/tmp/aish_cli_home");
        });
    }

    #[test]
    fn test_resolve_home_dir_uses_aish_home_env() {
        let input = PathResolverInput::default();

        with_env_var("AISH_HOME", Some("/tmp/aish_env_home2"), || {
            let home = resolve_home_dir(&input).unwrap();
            assert_eq!(home, "/tmp/aish_env_home2");
        });
    }

    #[test]
    fn test_resolve_home_dir_uses_xdg_config_home() {
        let input = PathResolverInput::default();

        with_env_var("AISH_HOME", None, || {
            with_env_var("XDG_CONFIG_HOME", Some("/tmp/xdg_config"), || {
                let home = resolve_home_dir(&input).unwrap();
                assert_eq!(home, "/tmp/xdg_config/aish");
            });
        });
    }

    #[test]
    fn test_resolve_session_dir_prefers_cli() {
        let input = PathResolverInput {
            session_dir: Some("/tmp/aish_session_cli".to_string()),
            ..Default::default()
        };
        let home_dir = "/tmp/aish_home_any".to_string();

        let session = resolve_session_dir(&input, &home_dir).unwrap();
        assert_eq!(session, "/tmp/aish_session_cli");
    }

    #[test]
    fn test_resolve_session_dir_under_explicit_home() {
        let input = PathResolverInput {
            home_dir: Some("/tmp/aish_cli_home3".to_string()),
            ..Default::default()
        };
        let home_dir = resolve_home_dir(&input).unwrap();

        let session = resolve_session_dir(&input, &home_dir).unwrap();
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
    }

    #[test]
    fn test_resolve_session_dir_uses_xdg_state_home() {
        let input = PathResolverInput::default();

        with_env_var("AISH_HOME", None, || {
            with_env_var("XDG_STATE_HOME", Some("/tmp/xdg_state"), || {
                let home_dir = "/tmp/some_home".to_string();
                let session = resolve_session_dir(&input, &home_dir).unwrap();
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
    }

    #[test]
    fn test_generate_session_dirname_format() {
        let name = generate_session_dirname();
        assert_eq!(name.len(), 8);

        for c in name.chars() {
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
    }
}
