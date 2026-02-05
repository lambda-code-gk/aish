//! テスト用のエントリーポイント（run_app）
//!
//! 本番のコマンドディスパッチは main.rs の Runner が行う。
//! このモジュールはテストで使用する run_app のみを提供する。

use crate::cli::Config;
use common::error::Error;

/// セッションが明示的に指定されているかをチェック（テスト用）
///
/// 以下のいずれかが設定されていれば true:
/// - `-s/--session-dir` オプション
/// - `-d/--home-dir` オプション
/// - `AISH_SESSION` 環境変数
/// - `AISH_HOME` 環境変数
#[cfg(unix)]
fn is_session_explicitly_specified(config: &Config) -> bool {
    if config.session_dir.is_some() || config.home_dir.is_some() {
        return true;
    }
    
    if let Ok(env_session) = std::env::var("AISH_SESSION") {
        if !env_session.is_empty() {
            return true;
        }
    }
    
    if let Ok(env_home) = std::env::var("AISH_HOME") {
        if !env_home.is_empty() {
            return true;
        }
    }
    
    false
}

/// 標準アダプターで App を組み立てて Config を実行する（テスト用の入口）
#[cfg(unix)]
#[allow(dead_code)] // テストで使用
pub fn run_app(config: Config) -> Result<i32, Error> {
    use crate::cli::config_to_command;
    use crate::domain::command::Command;
    use super::{ClearUseCase, ShellUseCase, TruncateConsoleLogUseCase};
    
    let app = crate::wiring::wire_aish();
    let command = config_to_command(&config);
    
    match command {
        Command::Help => Ok(0), // Help はテストでは単に成功とする
        Command::Shell => {
            let use_case = ShellUseCase::from_app(&app);
            use_case.run(&config)
        }
        Command::TruncateConsoleLog => {
            let use_case = TruncateConsoleLogUseCase::from_app(&app);
            use_case.run(&config)
        }
        Command::Clear => {
            let use_case = ClearUseCase::from_app(&app);
            let session_explicitly_specified = is_session_explicitly_specified(&config);
            use_case.run(&config, session_explicitly_specified)
        }
        Command::SysqList
        | Command::SysqEnable { .. }
        | Command::SysqDisable { .. } => Err(Error::invalid_argument(
            "sysq commands are not available in run_app (use aish binary).".to_string(),
        )),
        Command::Resume
        | Command::Sessions
        | Command::Rollout
        | Command::Ls
        | Command::RmLast
        | Command::Memory
        | Command::Models => Err(Error::invalid_argument(format!(
            "Command '{}' is not implemented.",
            command.as_str()
        ))),
        Command::Unknown(name) => Err(Error::invalid_argument(format!(
            "Command '{}' is not implemented.",
            name
        ))),
    }
}

/// run_app の非 Unix 用ダミー（aish は Unix 専用のため通常は使わない）
#[cfg(not(unix))]
pub fn run_app(config: Config) -> Result<i32, Error> {
    let _ = config;
    Err(Error::system("aish is only supported on Unix"))
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
    fn test_run_app_with_unimplemented_command() {
        use std::fs;
        let temp_dir = std::env::temp_dir();
        let home_path = temp_dir.join("aish_test_home_unimpl");
        
        if home_path.exists() {
            fs::remove_dir_all(&home_path).unwrap();
        }
        fs::create_dir_all(&home_path).unwrap();
        
        let config = Config {
            command_name: Some("sessions".to_string()),
            home_dir: Some(home_path.to_string_lossy().to_string()),
            ..Default::default()
        };
        let result = run_app(config);
        assert!(result.is_err(), "unimplemented command must return error");
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not implemented"));
        assert_eq!(err.exit_code(), 64);
        
        fs::remove_dir_all(&home_path).unwrap();
    }

    #[test]
    fn test_run_app_with_truncate_console_log() {
        use std::fs;
        let temp_dir = std::env::temp_dir();
        let home_path = temp_dir.join("aish_test_home_truncate");
        
        if home_path.exists() {
            fs::remove_dir_all(&home_path).unwrap();
        }
        fs::create_dir_all(&home_path).unwrap();
        
        let config = Config {
            command_name: Some("truncate_console_log".to_string()),
            home_dir: Some(home_path.to_string_lossy().to_string()),
            ..Default::default()
        };
        let result = run_app(config);
        assert!(result.is_ok(), "truncate_console_log (no PID file) should succeed");
        assert_eq!(result.unwrap(), 0);
        
        fs::remove_dir_all(&home_path).unwrap();
    }
}

#[cfg(test)]
mod clear_parts_tests {
    use super::*;
    use std::fs;

    /// partファイルがあるセッションで clear を実行するとすべて削除される
    #[test]
    fn test_clear_parts_removes_part_files() {
        let temp_dir = std::env::temp_dir();
        let home_path = temp_dir.join("aish_test_clear_home");
        let session_path = temp_dir.join("aish_test_clear_session");
        
        // クリーンアップ
        if home_path.exists() {
            fs::remove_dir_all(&home_path).unwrap();
        }
        if session_path.exists() {
            fs::remove_dir_all(&session_path).unwrap();
        }
        
        fs::create_dir_all(&home_path).unwrap();
        fs::create_dir_all(&session_path).unwrap();
        
        // partファイルを作成
        fs::write(session_path.join("part_00000001_user.txt"), "Hello").unwrap();
        fs::write(session_path.join("part_00000002_assistant.txt"), "Hi there").unwrap();
        fs::write(session_path.join("part_00000003_user.txt"), "How are you?").unwrap();
        // 他のファイル（削除されてはいけない）
        fs::write(session_path.join("console.txt"), "console log").unwrap();
        fs::write(session_path.join("AISH_PID"), "12345").unwrap();
        
        // clear コマンドを実行
        let config = Config {
            command_name: Some("clear".to_string()),
            home_dir: Some(home_path.to_string_lossy().to_string()),
            session_dir: Some(session_path.to_string_lossy().to_string()),
            ..Default::default()
        };
        let result = run_app(config);
        assert!(result.is_ok(), "clear command should succeed: {:?}", result.err());
        assert_eq!(result.unwrap(), 0);
        
        // partファイルが削除されていることを確認
        assert!(!session_path.join("part_00000001_user.txt").exists(), "part file 1 should be deleted");
        assert!(!session_path.join("part_00000002_assistant.txt").exists(), "part file 2 should be deleted");
        assert!(!session_path.join("part_00000003_user.txt").exists(), "part file 3 should be deleted");
        
        // 他のファイルは残っていることを確認
        assert!(session_path.join("console.txt").exists(), "console.txt should remain");
        assert!(session_path.join("AISH_PID").exists(), "AISH_PID should remain");
        
        // クリーンアップ
        fs::remove_dir_all(&home_path).unwrap();
        fs::remove_dir_all(&session_path).unwrap();
    }

    /// partファイルがないセッションで clear を実行しても正常終了
    #[test]
    fn test_clear_parts_with_no_part_files() {
        let temp_dir = std::env::temp_dir();
        let home_path = temp_dir.join("aish_test_clear_empty_home");
        let session_path = temp_dir.join("aish_test_clear_empty_session");
        
        // クリーンアップ
        if home_path.exists() {
            fs::remove_dir_all(&home_path).unwrap();
        }
        if session_path.exists() {
            fs::remove_dir_all(&session_path).unwrap();
        }
        
        fs::create_dir_all(&home_path).unwrap();
        fs::create_dir_all(&session_path).unwrap();
        
        // partファイルなし、他のファイルのみ
        fs::write(session_path.join("console.txt"), "console log").unwrap();
        
        // clear コマンドを実行
        let config = Config {
            command_name: Some("clear".to_string()),
            home_dir: Some(home_path.to_string_lossy().to_string()),
            session_dir: Some(session_path.to_string_lossy().to_string()),
            ..Default::default()
        };
        let result = run_app(config);
        assert!(result.is_ok(), "clear command should succeed even with no part files");
        assert_eq!(result.unwrap(), 0);
        
        // 他のファイルは残っていることを確認
        assert!(session_path.join("console.txt").exists(), "console.txt should remain");
        
        // クリーンアップ
        fs::remove_dir_all(&home_path).unwrap();
        fs::remove_dir_all(&session_path).unwrap();
    }

    /// 空のセッションディレクトリで clear を実行しても正常終了
    #[test]
    fn test_clear_parts_with_empty_session() {
        let temp_dir = std::env::temp_dir();
        let home_path = temp_dir.join("aish_test_clear_really_empty_home");
        let session_path = temp_dir.join("aish_test_clear_really_empty_session");
        
        // クリーンアップ
        if home_path.exists() {
            fs::remove_dir_all(&home_path).unwrap();
        }
        if session_path.exists() {
            fs::remove_dir_all(&session_path).unwrap();
        }
        
        fs::create_dir_all(&home_path).unwrap();
        fs::create_dir_all(&session_path).unwrap();
        
        // clear コマンドを実行
        let config = Config {
            command_name: Some("clear".to_string()),
            home_dir: Some(home_path.to_string_lossy().to_string()),
            session_dir: Some(session_path.to_string_lossy().to_string()),
            ..Default::default()
        };
        let result = run_app(config);
        assert!(result.is_ok(), "clear command should succeed with empty session");
        assert_eq!(result.unwrap(), 0);
        
        // クリーンアップ
        fs::remove_dir_all(&home_path).unwrap();
        fs::remove_dir_all(&session_path).unwrap();
    }

    /// セッションが明示的に指定されていない場合（-s, -d, AISH_SESSION のいずれもなし）はエラー
    #[test]
    fn test_clear_fails_without_explicit_session() {
        use std::env;
        
        // 環境変数をクリア
        let orig_aish_session = env::var("AISH_SESSION").ok();
        let orig_aish_home = env::var("AISH_HOME").ok();
        env::remove_var("AISH_SESSION");
        env::remove_var("AISH_HOME");
        
        // -s も -d も指定しない
        let config = Config {
            command_name: Some("clear".to_string()),
            session_dir: None,
            home_dir: None,
            ..Default::default()
        };
        let result = run_app(config);
        
        // 環境変数を復元
        match orig_aish_session {
            Some(v) => env::set_var("AISH_SESSION", v),
            None => env::remove_var("AISH_SESSION"),
        }
        match orig_aish_home {
            Some(v) => env::set_var("AISH_HOME", v),
            None => env::remove_var("AISH_HOME"),
        }
        
        assert!(result.is_err(), "clear without session specification should fail");
        let err = result.unwrap_err();
        assert!(err.to_string().contains("session"), "error message should mention session: {}", err);
        assert_eq!(err.exit_code(), 64);
    }

    /// AISH_SESSION 環境変数が設定されている場合は成功
    #[test]
    fn test_clear_succeeds_with_aish_session_env() {
        use std::env;
        
        let temp_dir = std::env::temp_dir();
        let home_path = temp_dir.join("aish_test_clear_env_home");
        let session_path = temp_dir.join("aish_test_clear_env_session");
        
        // クリーンアップ
        if home_path.exists() {
            fs::remove_dir_all(&home_path).unwrap();
        }
        if session_path.exists() {
            fs::remove_dir_all(&session_path).unwrap();
        }
        
        fs::create_dir_all(&home_path).unwrap();
        fs::create_dir_all(&session_path).unwrap();
        
        // 環境変数を設定
        let orig_aish_session = env::var("AISH_SESSION").ok();
        let orig_aish_home = env::var("AISH_HOME").ok();
        env::set_var("AISH_SESSION", session_path.to_string_lossy().to_string());
        env::set_var("AISH_HOME", home_path.to_string_lossy().to_string());
        
        // -s も -d も指定しないが、AISH_SESSION 環境変数がある
        let config = Config {
            command_name: Some("clear".to_string()),
            session_dir: None,
            home_dir: None,
            ..Default::default()
        };
        let result = run_app(config);
        
        // 環境変数を復元
        match orig_aish_session {
            Some(v) => env::set_var("AISH_SESSION", v),
            None => env::remove_var("AISH_SESSION"),
        }
        match orig_aish_home {
            Some(v) => env::set_var("AISH_HOME", v),
            None => env::remove_var("AISH_HOME"),
        }
        
        assert!(result.is_ok(), "clear with AISH_SESSION env should succeed: {:?}", result.err());
        assert_eq!(result.unwrap(), 0);
        
        // クリーンアップ
        fs::remove_dir_all(&home_path).unwrap();
        fs::remove_dir_all(&session_path).unwrap();
    }
}
