use crate::adapter::{path_resolver, run_shell, UnixPtySpawn, UnixSignal};
use crate::cli::Config;
use crate::domain::command::Command;
use common::adapter::{FileSystem, PtySpawn, Signal};
use common::error::Error;
use common::part_id::IdGenerator;
use common::session::Session;
use std::path::Path;
use std::sync::Arc;

/// aish のユースケース（アダプター経由で I/O を行う）
#[cfg(unix)]
pub struct AishUseCase {
    pub fs: Arc<dyn FileSystem>,
    pub id_gen: Arc<dyn IdGenerator>,
    pub signal: Arc<dyn Signal>,
    pub pty_spawn: Arc<dyn PtySpawn>,
}

#[cfg(unix)]
impl AishUseCase {
    pub fn new(
        fs: Arc<dyn FileSystem>,
        id_gen: Arc<dyn IdGenerator>,
        signal: Arc<dyn Signal>,
        pty_spawn: Arc<dyn PtySpawn>,
    ) -> Self {
        Self {
            fs,
            id_gen,
            signal,
            pty_spawn,
        }
    }

    pub fn run(&self, config: Config) -> Result<i32, Error> {
        if config.help {
            print_help();
            return Ok(0);
        }

        // clear コマンドはセッションが明示的に指定されている必要がある
        if config.command == Command::Clear {
            if !is_session_explicitly_specified(&config) {
                return Err(Error::invalid_argument(
                    "The 'clear' command requires a session to be specified. \
                     Use -s/--session-dir, -d/--home-dir, or set AISH_SESSION environment variable.",
                ));
            }
        }

        let path_input = path_resolver::PathResolverInput {
            home_dir: config.home_dir.clone(),
            session_dir: config.session_dir.clone(),
        };
        let home_dir = path_resolver::resolve_home_dir(&path_input)?;
        let session_path = path_resolver::resolve_session_dir(&path_input, &home_dir)?;
        let session = Session::new(&session_path, &home_dir)?;

        match &config.command {
            Command::Shell => run_shell(
                session.session_dir().as_ref(),
                session.aish_home().as_ref(),
                self.fs.as_ref(),
                self.id_gen.as_ref(),
                self.signal.as_ref(),
                self.pty_spawn.as_ref(),
            ),
            Command::TruncateConsoleLog => truncate_console_log(
                session.session_dir().as_ref(),
                self.fs.as_ref(),
                self.signal.as_ref(),
            ),
            Command::Clear => clear_parts(session.session_dir().as_ref(), self.fs.as_ref()),
            Command::Resume
            | Command::Sessions
            | Command::Rollout
            | Command::Ls
            | Command::RmLast
            | Command::Memory
            | Command::Models => Err(Error::invalid_argument(format!(
                "Command '{}' is not implemented.",
                config.command.as_str()
            ))),
            Command::Unknown(name) => Err(Error::invalid_argument(format!(
                "Command '{}' is not implemented.",
                name
            ))),
        }
    }
}

/// セッションが明示的に指定されているかをチェック
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

/// 配線: 標準アダプタで AishUseCase を組み立てる（Unix 専用）
#[cfg(unix)]
pub fn wire_aish() -> AishUseCase {
    let fs = Arc::new(common::adapter::StdFileSystem);
    let id_gen = Arc::new(common::part_id::StdIdGenerator::new(Arc::new(
        common::adapter::StdClock,
    )));
    let signal = Arc::new(UnixSignal);
    let pty_spawn = Arc::new(UnixPtySpawn);
    AishUseCase::new(fs, id_gen, signal, pty_spawn)
}

/// 標準アダプターで AishUseCase を組み立てて run する（テスト用の入口）
#[cfg(unix)]
#[allow(dead_code)] // テストで使用
pub fn run_app(config: Config) -> Result<i32, Error> {
    wire_aish().run(config)
}

/// run_app の非 Unix 用ダミー（aish は Unix 専用のため通常は使わない）
#[cfg(not(unix))]
pub fn run_app(config: Config) -> Result<i32, Error> {
    let _ = config;
    Err(Error::system("aish is only supported on Unix"))
}

/// console.txtとメモリ上のバッファをトランケートする（アダプター経由）
#[cfg(unix)]
fn truncate_console_log<F: FileSystem + ?Sized, S: Signal + ?Sized>(
    session_dir: &Path,
    fs: &F,
    signal: &S,
) -> Result<i32, Error> {
    let pid_file_path = session_dir.join("AISH_PID");

    if !fs.exists(&pid_file_path) {
        return Ok(0);
    }

    let pid_str = fs.read_to_string(&pid_file_path)?;
    let pid: i32 = pid_str
        .trim()
        .parse()
        .map_err(|e| Error::io_msg(format!("Invalid PID in AISH_PID file: {}", e)))?;

    signal.send_signal(pid, libc::SIGUSR2)?;
    Ok(0)
}

/// セッションディレクトリ内のすべての part ファイルを削除する（アダプター経由）
#[cfg(unix)]
fn clear_parts<F: FileSystem + ?Sized>(session_dir: &Path, fs: &F) -> Result<i32, Error> {
    if !fs.exists(session_dir) {
        return Ok(0);
    }

    // ディレクトリ内のファイル一覧を取得
    let entries = fs.read_dir(session_dir)?;

    // part_ で始まるファイルを削除
    for entry in entries {
        if let Some(file_name) = entry.file_name().and_then(|n| n.to_str()) {
            if file_name.starts_with("part_") {
                // ファイルかどうか確認
                if fs.metadata(&entry).map(|m| m.is_file()).unwrap_or(false) {
                    fs.remove_file(&entry)?;
                }
            }
        }
    }

    Ok(0)
}

fn print_help() {
    println!("Usage: aish [-h] [-s|--session-dir directory] [-d|--home-dir directory] [<command> [args...]]");
    println!("  -h                    Display this help message.");
    println!("  -d, --home-dir        Specify a home directory (sets AISH_HOME environment variable).");
    println!("  -s, --session-dir     Specify a session directory (for resume). Without -s, a new unique session is used each time.");
    println!("  <command>             Command to execute. Omit to start the interactive shell.");
    println!("  [args...]             Arguments for the command.");
    println!("");
    println!("Implemented commands:");
    println!("  clear                  Clear all part files in the session directory (delete conversation history).");
    println!("  truncate_console_log   Truncate console buffer and log file (used by ai command).");
    println!("");
    println!("Not yet implemented: resume, sessions, rollout, ls, rm_last, memory, models.");
}

/// 引数不正時に stderr へ出力する usage 行（main から呼ぶ）
pub fn print_usage() {
    eprintln!("Usage: aish [-h] [-s|--session-dir directory] [-d|--home-dir directory] [<command> [args...]]");
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
            command: Command::Sessions,
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
            command: Command::TruncateConsoleLog,
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
            command: Command::Clear,
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
            command: Command::Clear,
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
            command: Command::Clear,
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
            command: Command::Clear,
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
            command: Command::Clear,
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
