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
            Command::Resume
            | Command::Sessions
            | Command::Rollout
            | Command::Clear
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

fn print_help() {
    println!("Usage: aish [-h] [-s|--session-dir directory] [-d|--home-dir directory] [<command> [args...]]");
    println!("  -h                    Display this help message.");
    println!("  -d, --home-dir        Specify a home directory (sets AISH_HOME environment variable).");
    println!("  -s, --session-dir     Specify a session directory (for resume). Without -s, a new unique session is used each time.");
    println!("  <command>             Command to execute. Omit to start the interactive shell.");
    println!("  [args...]             Arguments for the command.");
    println!("");
    println!("Implemented commands:");
    println!("  truncate_console_log   Truncate console buffer and log file (used by ai command).");
    println!("");
    println!("Not yet implemented: resume, sessions, rollout, clear, ls, rm_last, memory, models.");
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

