use crate::args::Config;
use crate::shell::run_shell;
use common::error::Error;
use common::session::Session;
use std::env;
use std::path::PathBuf;

pub fn run_app(config: Config) -> Result<i32, Error> {
    if config.help {
        print_help();
        return Ok(0);
    }

    // ホームディレクトリ（論理的な AISH_HOME）を解決
    let home_dir = resolve_home_dir(&config)?;

    // セッションディレクトリを決定
    let session_path = resolve_session_dir(&config, &home_dir)?;

    // セッション管理を初期化（ホームディレクトリとセッションディレクトリを指定）
    let session = Session::new(&session_path, &home_dir)?;
    
    // コマンドが指定されている場合は、将来的にコマンド処理を実装
    if let Some(ref _command) = config.command {
        // TODO: コマンド処理を実装
        Ok(0)
    } else {
        // 引数なしの場合はシェルを起動
        run_shell(&session)
    }
}

fn print_help() {
    println!("Usage: aish [-h] [-s|--session-dir directory] [-d|--home-dir directory] <command> [args...]");
    println!("  -h                    Display this help message.");
    println!("  -d, --home-dir        Specify a home directory (sets AISH_HOME environment variable).");
    println!("  -s, --session-dir      Specify a directory for the session.");
    println!("  <command>     Command to execute (e.g., ls, start, stop).");
    println!("  [args...]     Arguments for the command.");
    println!("");
    println!("Available commands:");
    println!("  resume [id]            Resume a session (default: latest).");
    println!("  sessions               List available sessions.");
    println!("  rollout                Write the terminal log to the part file.");
    println!("  clear                  Clear the console and part files.");
    println!("  ls                     List the part files.");
    println!("  rm_last                Remove the last part file.");
    println!("  memory                 Manage memories (--list, --show <id>, --revoke <id>).");
    println!("  models                 Manage models (--provider, --unsupported, --available).");
}

/// ホームディレクトリ（論理的な AISH_HOME）を解決する
///
/// 優先順位:
/// 1. コマンドラインオプション -d/--home-dir
/// 2. 環境変数 AISH_HOME
/// 3. XDG_CONFIG_HOME/aish （未設定時は ~/.config/aish）
fn resolve_home_dir(config: &Config) -> Result<String, Error> {
    // 1. CLI オプション
    if let Some(ref home) = config.home_dir {
        return Ok(home.clone());
    }

    // 2. 環境変数 AISH_HOME
    if let Ok(env_home) = env::var("AISH_HOME") {
        if !env_home.is_empty() {
            return Ok(env_home);
        }
    }

    // 3. XDG_CONFIG_HOME/aish （デフォルトは ~/.config/aish）
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
/// 1. コマンドラインオプション -s/--session-dir
/// 2. AISH_HOME が明示されている場合: その配下の state/session
/// 3. XDG_STATE_HOME/aish/session （未設定時は ~/.local/state/aish/session）
fn resolve_session_dir(config: &Config, home_dir: &str) -> Result<String, Error> {
    // 1. CLI オプション
    if let Some(ref session_dir) = config.session_dir {
        return Ok(session_dir.clone());
    }

    // 2. AISH_HOME が CLI or 環境変数で明示されている場合は、
    //    その配下の state/session/{YYYYMMDD_HHMMSS_mmm} を使う
    //    ここでは「非 XDG デフォルト」のホームを判定するために、直接環境変数と CLI を見る
    let cli_home = config.home_dir.as_ref();
    let env_home = env::var("AISH_HOME").ok();
    if cli_home.is_some() || env_home.is_some() {
        let mut path = PathBuf::from(home_dir);
        path.push("state");
        path.push("session");
        path.push(generate_session_dirname());
        return Ok(path.to_string_lossy().to_string());
    }

    // 3. XDG_STATE_HOME/aish/session （デフォルトは ~/.local/state/aish/session）
    let xdg_state_home = env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let mut path = dirs_home().unwrap_or_else(|| PathBuf::from("~"));
        path.push(".local");
        path.push("state");
        path.to_string_lossy().to_string()
    });

    let mut path = PathBuf::from(xdg_state_home);
    path.push("aish");
    path.push("session");

    Ok(path.to_string_lossy().to_string())
}

/// ホームディレクトリ (~) を取得する簡易ヘルパ
///
/// 標準ライブラリのみで実装するため、$HOME 環境変数に依存する。
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

    // base64url 文字テーブル
    const BASE64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();

    // ミリ秒単位に変換（u64にキャスト）
    let ms_since_epoch = dur.as_millis() as u64;

    // 下位48bitのみを使用
    let ts48 = ms_since_epoch & ((1u64 << 48) - 1);

    // 48bit = 6bit * 8文字 → 8文字固定長
    let mut buf = [0u8; 8];
    for i in 0..8 {
        let shift = 6 * (7 - i);
        let index = ((ts48 >> shift) & 0x3F) as usize;
        buf[i] = BASE64URL[index];
    }

    String::from_utf8_lossy(&buf).to_string()
}

#[cfg(test)]
mod path_tests {
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
        let config = Config {
            home_dir: Some("/tmp/aish_cli_home".to_string()),
            ..Default::default()
        };

        // AISH_HOME が設定されていても CLI が優先されること
        with_env_var("AISH_HOME", Some("/tmp/aish_env_home"), || {
            let home = resolve_home_dir(&config).unwrap();
            assert_eq!(home, "/tmp/aish_cli_home");
        });
    }

    #[test]
    fn test_resolve_home_dir_uses_aish_home_env() {
        let config = Config::default();

        with_env_var("AISH_HOME", Some("/tmp/aish_env_home2"), || {
            let home = resolve_home_dir(&config).unwrap();
            assert_eq!(home, "/tmp/aish_env_home2");
        });
    }

    #[test]
    fn test_resolve_home_dir_uses_xdg_config_home() {
        let config = Config::default();

        with_env_var("AISH_HOME", None, || {
            with_env_var("XDG_CONFIG_HOME", Some("/tmp/xdg_config"), || {
                let home = resolve_home_dir(&config).unwrap();
                assert_eq!(home, "/tmp/xdg_config/aish");
            });
        });
    }

    #[test]
    fn test_resolve_session_dir_prefers_cli() {
        let config = Config {
            session_dir: Some("/tmp/aish_session_cli".to_string()),
            ..Default::default()
        };
        let home_dir = "/tmp/aish_home_any".to_string();

        let session = resolve_session_dir(&config, &home_dir).unwrap();
        assert_eq!(session, "/tmp/aish_session_cli");
    }

    #[test]
    fn test_resolve_session_dir_under_explicit_home() {
        let config = Config {
            home_dir: Some("/tmp/aish_cli_home3".to_string()),
            ..Default::default()
        };
        let home_dir = resolve_home_dir(&config).unwrap();

        let session = resolve_session_dir(&config, &home_dir).unwrap();
        let prefix = "/tmp/aish_cli_home3/state/session/";
        assert!(session.starts_with(prefix));

        // 末尾8文字が base64url のセッションIDであることを確認
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
        let config = Config::default();

        with_env_var("AISH_HOME", None, || {
            with_env_var("XDG_STATE_HOME", Some("/tmp/xdg_state"), || {
                // home_dir は XDG_CONFIG_HOME 由来だが、ここでは値そのものは重要ではない
                let home_dir = "/tmp/some_home".to_string();
                let session = resolve_session_dir(&config, &home_dir).unwrap();
                assert_eq!(session, "/tmp/xdg_state/aish/session");
            });
        });
    }

    #[test]
    fn test_generate_session_dirname_format() {
        let name = generate_session_dirname();
        // 長さは8文字固定
        assert_eq!(name.len(), 8);

        // base64url 文字のみで構成されていること
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

#[cfg(test)]
mod app_tests {
    use super::*;

    #[test]
    fn test_run_app_with_help() {
        let config = Config {
            help: true,
            ..Default::default()
        };
        let result = run_app(config);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[test]
    fn test_run_app_with_command() {
        use std::fs;
        let temp_dir = std::env::temp_dir();
        let home_path = temp_dir.join("aish_test_home_command");
        
        // ホームディレクトリを作成
        if home_path.exists() {
            fs::remove_dir_all(&home_path).unwrap();
        }
        fs::create_dir_all(&home_path).unwrap();
        
        let config = Config {
            command: Some("sessions".to_string()),
            home_dir: Some(home_path.to_string_lossy().to_string()),
            ..Default::default()
        };
        let result = run_app(config);
        // コマンド処理は未実装のため、現時点では成功を返す
        assert!(result.is_ok());
        
        // クリーンアップ
        fs::remove_dir_all(&home_path).unwrap();
    }
}

