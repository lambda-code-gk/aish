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
/// 形式: YYYYMMDD_HHMMSS_mmm
fn generate_session_dirname() -> String {
    unsafe {
        use libc::{time, localtime, tm};
        let mut now: libc::time_t = 0;
        time(&mut now);
        let tm_ptr = localtime(&now);
        if tm_ptr.is_null() {
            // フォールバック: SystemTime ベースの秒＋ミリ秒
            use std::time::{SystemTime, UNIX_EPOCH};
            let dur = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default();
            let secs = dur.as_secs();
            let millis = (dur.subsec_nanos() / 1_000_000) as u32;
            return format!("{}_{}", secs, format!("{:03}", millis));
        }
        let tm: &tm = &*tm_ptr;

        // ミリ秒を取得
        use libc::{clock_gettime, CLOCK_REALTIME, timespec};
        let mut ts: timespec = std::mem::zeroed();
        let millis = if clock_gettime(CLOCK_REALTIME, &mut ts) == 0 {
            (ts.tv_nsec / 1_000_000) as u32
        } else {
            0
        };

        format!(
            "{:04}{:02}{:02}_{:02}{:02}{:02}_{:03}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec,
            millis
        )
    }
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
        assert!(session.starts_with("/tmp/aish_cli_home3/state/session/"));
        // タイムスタンプ部分が何らかの非空文字列であることを確認
        let suffix = session.trim_start_matches("/tmp/aish_cli_home3/state/session/");
        assert!(!suffix.is_empty());
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

