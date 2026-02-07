mod adapter;
mod cli;
mod domain;
mod ports;
mod usecase;
mod wiring;

use std::process;
use common::error::Error;
use common::ports::outbound::PathResolverInput;
use cli::{config_to_command, parse_args, Config};
use domain::command::Command;
use ports::inbound::UseCaseRunner;
#[cfg(unix)]
use usecase::SysqUseCase;
#[cfg(unix)]
use wiring::{wire_aish, App};

/// Command をディスパッチする Runner（match は main レイヤーに集約）
#[cfg(unix)]
struct Runner {
    app: App,
}

#[cfg(unix)]
impl UseCaseRunner for Runner {
    fn run(&self, config: Config) -> Result<i32, Error> {
        let command = config_to_command(&config);
        let path_input = PathResolverInput {
            home_dir: config.home_dir.clone(),
            session_dir: config.session_dir.clone(),
        };

        match command {
            Command::Help => {
                print_help();
                Ok(0)
            }
            Command::Shell => self.app.shell_use_case.run(&path_input),
            Command::TruncateConsoleLog => self.app.truncate_console_log_use_case.run(&path_input),
            Command::Clear => {
                let session_explicitly_specified = is_session_explicitly_specified(&config);
                self.app.clear_use_case.run(&path_input, session_explicitly_specified)
            }
            Command::SysqList => {
                let use_case = SysqUseCase::new(std::sync::Arc::clone(&self.app.sysq_repository));
                let entries = use_case.list()?;
                print_sysq_list(&entries);
                Ok(0)
            }
            Command::SysqEnable { ids } => {
                let use_case = SysqUseCase::new(std::sync::Arc::clone(&self.app.sysq_repository));
                use_case.enable(&ids)?;
                Ok(0)
            }
            Command::SysqDisable { ids } => {
                let use_case = SysqUseCase::new(std::sync::Arc::clone(&self.app.sysq_repository));
                use_case.disable(&ids)?;
                Ok(0)
            }
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
}

/// セッションが明示的に指定されているかをチェック（CLI 境界）
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

fn main() {
    let exit_code = match run() {
        Ok(code) => code,
        Err(e) => {
            if e.is_usage() {
                print_usage();
            }
            eprintln!("aish: {}", e);
            e.exit_code()
        }
    };
    process::exit(exit_code);
}

fn print_usage() {
    eprintln!("Usage: aish [-h] [-s|--session-dir directory] [-d|--home-dir directory] [<command> [args...]]");
}

fn print_help() {
    println!("Usage: aish [-h] [-s|--session-dir directory] [-d|--home-dir directory] [<command> [args...]]");
    println!("  -h                    Display this help message.");
    println!("  -d, --home-dir        Specify a home directory (sets AISH_HOME environment variable).");
    println!("  -s, --session-dir     Specify a session directory (for resume). Without -s, a new unique session is used each time.");
    println!("  <command>             Command to execute. Omit to start the interactive shell.");
    println!("  [args...]             Arguments for the command.");
    println!();
    println!("Implemented commands:");
    println!("  clear                  Clear all part files in the session directory (delete conversation history).");
    println!("  truncate_console_log   Truncate console buffer and log file (used by ai command).");
    println!();
    println!("  sysq list              List system prompts and their enabled state.");
    println!("  sysq enable <id>...    Enable system prompt(s).");
    println!("  sysq disable <id>...   Disable system prompt(s).");
    println!();
    println!("Not yet implemented: resume, sessions, rollout, ls, rm_last, memory, models.");
}

#[cfg(unix)]
fn print_sysq_list(entries: &[crate::ports::outbound::SysqListEntry]) {
    println!("{:8} {:7} {:<20} {}", "SCOPE", "ENABLED", "ID", "TITLE");
    for e in entries {
        let enabled = if e.enabled { "yes" } else { "no" };
        let title = if e.title.len() > 40 { format!("{}...", &e.title[..37]) } else { e.title.clone() };
        println!("{:8} {:7} {:<20} {}", e.scope.as_str(), enabled, e.id, title);
    }
}

pub fn run() -> Result<i32, Error> {
    let config = parse_args()?;
    #[cfg(unix)]
    {
        let app = wire_aish();
        let runner = Runner { app };
        runner.run(config)
    }
    #[cfg(not(unix))]
    {
        let _ = config;
        Err(Error::system("aish is only supported on Unix"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{config_to_command, Config};
    use crate::wiring::wire_aish;
    use domain::command::Command;

    fn is_session_explicitly_specified_for_test(config: &Config) -> bool {
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

    /// テスト用: Config から path_input を組み立てて usecase を実行する（usecase は cli に依存しない）
    #[cfg(unix)]
    fn run_app(config: Config) -> Result<i32, Error> {
        let app = wire_aish();
        let command = config_to_command(&config);
        let path_input = PathResolverInput {
            home_dir: config.home_dir.clone(),
            session_dir: config.session_dir.clone(),
        };
        match command {
            Command::Help => Ok(0),
            Command::Shell => app.shell_use_case.run(&path_input),
            Command::TruncateConsoleLog => app.truncate_console_log_use_case.run(&path_input),
            Command::Clear => {
                let session_explicitly_specified = is_session_explicitly_specified_for_test(&config);
                app.clear_use_case.run(&path_input, session_explicitly_specified)
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

    #[cfg(not(unix))]
    fn run_app(config: Config) -> Result<i32, Error> {
        let _ = config;
        Err(Error::system("aish is only supported on Unix"))
    }

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

    mod clear_parts_tests {
        use super::*;
        use std::fs;

        #[test]
        fn test_clear_parts_removes_part_files() {
            let temp_dir = std::env::temp_dir();
            let home_path = temp_dir.join("aish_test_clear_home");
            let session_path = temp_dir.join("aish_test_clear_session");

            if home_path.exists() {
                fs::remove_dir_all(&home_path).unwrap();
            }
            if session_path.exists() {
                fs::remove_dir_all(&session_path).unwrap();
            }

            fs::create_dir_all(&home_path).unwrap();
            fs::create_dir_all(&session_path).unwrap();

            fs::write(session_path.join("part_00000001_user.txt"), "Hello").unwrap();
            fs::write(session_path.join("part_00000002_assistant.txt"), "Hi there").unwrap();
            fs::write(session_path.join("part_00000003_user.txt"), "How are you?").unwrap();
            fs::write(session_path.join("console.txt"), "console log").unwrap();
            fs::write(session_path.join("AISH_PID"), "12345").unwrap();

            let config = Config {
                command_name: Some("clear".to_string()),
                home_dir: Some(home_path.to_string_lossy().to_string()),
                session_dir: Some(session_path.to_string_lossy().to_string()),
                ..Default::default()
            };
            let result = run_app(config);
            assert!(result.is_ok(), "clear command should succeed: {:?}", result.err());
            assert_eq!(result.unwrap(), 0);

            assert!(!session_path.join("part_00000001_user.txt").exists());
            assert!(!session_path.join("part_00000002_assistant.txt").exists());
            assert!(!session_path.join("part_00000003_user.txt").exists());
            assert!(session_path.join("console.txt").exists());
            assert!(session_path.join("AISH_PID").exists());

            fs::remove_dir_all(&home_path).unwrap();
            fs::remove_dir_all(&session_path).unwrap();
        }

        #[test]
        fn test_clear_parts_with_no_part_files() {
            let temp_dir = std::env::temp_dir();
            let home_path = temp_dir.join("aish_test_clear_empty_home");
            let session_path = temp_dir.join("aish_test_clear_empty_session");

            if home_path.exists() {
                fs::remove_dir_all(&home_path).unwrap();
            }
            if session_path.exists() {
                fs::remove_dir_all(&session_path).unwrap();
            }

            fs::create_dir_all(&home_path).unwrap();
            fs::create_dir_all(&session_path).unwrap();
            fs::write(session_path.join("console.txt"), "console log").unwrap();

            let config = Config {
                command_name: Some("clear".to_string()),
                home_dir: Some(home_path.to_string_lossy().to_string()),
                session_dir: Some(session_path.to_string_lossy().to_string()),
                ..Default::default()
            };
            let result = run_app(config);
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), 0);
            assert!(session_path.join("console.txt").exists());

            fs::remove_dir_all(&home_path).unwrap();
            fs::remove_dir_all(&session_path).unwrap();
        }

        #[test]
        fn test_clear_parts_with_empty_session() {
            let temp_dir = std::env::temp_dir();
            let home_path = temp_dir.join("aish_test_clear_really_empty_home");
            let session_path = temp_dir.join("aish_test_clear_really_empty_session");

            if home_path.exists() {
                fs::remove_dir_all(&home_path).unwrap();
            }
            if session_path.exists() {
                fs::remove_dir_all(&session_path).unwrap();
            }

            fs::create_dir_all(&home_path).unwrap();
            fs::create_dir_all(&session_path).unwrap();

            let config = Config {
                command_name: Some("clear".to_string()),
                home_dir: Some(home_path.to_string_lossy().to_string()),
                session_dir: Some(session_path.to_string_lossy().to_string()),
                ..Default::default()
            };
            let result = run_app(config);
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), 0);

            fs::remove_dir_all(&home_path).unwrap();
            fs::remove_dir_all(&session_path).unwrap();
        }

        #[test]
        fn test_clear_fails_without_explicit_session() {
            use std::env;

            let orig_aish_session = env::var("AISH_SESSION").ok();
            let orig_aish_home = env::var("AISH_HOME").ok();
            env::remove_var("AISH_SESSION");
            env::remove_var("AISH_HOME");

            let config = Config {
                command_name: Some("clear".to_string()),
                session_dir: None,
                home_dir: None,
                ..Default::default()
            };
            let result = run_app(config);

            if let Some(v) = orig_aish_session {
                env::set_var("AISH_SESSION", v);
            } else {
                env::remove_var("AISH_SESSION");
            }
            if let Some(v) = orig_aish_home {
                env::set_var("AISH_HOME", v);
            } else {
                env::remove_var("AISH_HOME");
            }

            assert!(result.is_err());
            let err = result.unwrap_err();
            assert!(err.to_string().contains("session"), "error message should mention session: {}", err);
            assert_eq!(err.exit_code(), 64);
        }

        #[test]
        fn test_clear_succeeds_with_aish_session_env() {
            use std::env;

            let temp_dir = std::env::temp_dir();
            let home_path = temp_dir.join("aish_test_clear_env_home");
            let session_path = temp_dir.join("aish_test_clear_env_session");

            if home_path.exists() {
                fs::remove_dir_all(&home_path).unwrap();
            }
            if session_path.exists() {
                fs::remove_dir_all(&session_path).unwrap();
            }

            fs::create_dir_all(&home_path).unwrap();
            fs::create_dir_all(&session_path).unwrap();

            let orig_aish_session = env::var("AISH_SESSION").ok();
            let orig_aish_home = env::var("AISH_HOME").ok();
            env::set_var("AISH_SESSION", session_path.to_string_lossy().to_string());
            env::set_var("AISH_HOME", home_path.to_string_lossy().to_string());

            let config = Config {
                command_name: Some("clear".to_string()),
                session_dir: None,
                home_dir: None,
                ..Default::default()
            };
            let result = run_app(config);

            if let Some(v) = orig_aish_session {
                env::set_var("AISH_SESSION", v);
            } else {
                env::remove_var("AISH_SESSION");
            }
            if let Some(v) = orig_aish_home {
                env::set_var("AISH_HOME", v);
            } else {
                env::remove_var("AISH_HOME");
            }

            assert!(result.is_ok(), "clear with AISH_SESSION env should succeed: {:?}", result.err());
            assert_eq!(result.unwrap(), 0);

            fs::remove_dir_all(&home_path).unwrap();
            fs::remove_dir_all(&session_path).unwrap();
        }
    }
}
