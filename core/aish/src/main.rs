mod adapter;
mod cli;
mod domain;
mod ports;
mod usecase;
mod wiring;

use std::process;
use common::error::Error;
use common::ports::outbound::PathResolverInput;
use cli::{config_to_command, parse_args, print_completion, Config, ParseOutcome};
use domain::command::Command;
use ports::inbound::UseCaseRunner;
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
            Command::Rollout => self.app.rollout_use_case.run(&path_input),
            Command::Mute => self.app.mute_use_case.run(&path_input),
            Command::Unmute => self.app.unmute_use_case.run(&path_input),
            Command::Clear => {
                let session_explicitly_specified = is_session_explicitly_specified(&config);
                self.app.clear_use_case.run(&path_input, session_explicitly_specified)
            }
            Command::Resume { id } => {
                self.app.resume_use_case.run(&path_input, id.as_deref())
            }
            Command::Sessions => {
                let ids = self.app.sessions_use_case.list(&path_input)?;
                for id in ids {
                    println!("{}", id);
                }
                Ok(0)
            }
            Command::Init {
                force,
                dry_run,
                defaults_dir: defaults_dir_opt,
            } => {
                let defaults_dir = defaults_dir_opt
                    .or_else(|| std::env::var("AISH_DEFAULTS_DIR").ok())
                    .ok_or_else(|| Error::invalid_argument(
                        "Defaults directory required: set AISH_DEFAULTS_DIR or use --defaults-dir".to_string(),
                    ))?;
                let input = crate::usecase::InitInput {
                    defaults_dir: std::path::PathBuf::from(&defaults_dir),
                    force,
                    dry_run,
                };
                let result = self.app.init_use_case.run(&input)?;
                if result.dry_run {
                    for p in &result.copied_paths {
                        println!("  {}", p.display());
                    }
                    println!("Would copy {} file(s) to {}", result.copied_count, result.config_dir.display());
                } else {
                    println!("Initialized config at {} ({} file(s))", result.config_dir.display(), result.copied_count);
                }
                Ok(0)
            }
            Command::SysqList => {
                let entries = self.app.sysq_use_case.list()?;
                print_sysq_list(&entries);
                Ok(0)
            }
            Command::SysqEnable { ids } => {
                self.app.sysq_use_case.enable(&ids)?;
                Ok(0)
            }
            Command::SysqDisable { ids } => {
                self.app.sysq_use_case.disable(&ids)?;
                Ok(0)
            }
            Command::MemoryList => {
                let entries = self.app.memory_use_case.list()?;
                print_memory_list(&entries);
                Ok(0)
            }
            Command::MemoryGet { ids } => {
                if ids.is_empty() {
                    return Err(Error::invalid_argument("memory get requires at least one id".to_string()));
                }
                let entries = self.app.memory_use_case.get(&ids)?;
                print_memory_get(&entries);
                Ok(0)
            }
            Command::MemoryRemove { ids } => {
                if ids.is_empty() {
                    return Err(Error::invalid_argument("memory remove requires at least one id".to_string()));
                }
                self.app.memory_use_case.remove(&ids)?;
                Ok(0)
            }
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
    println!("  -h, --help            Display this help message.");
    println!("  -d, --home-dir        Specify a home directory (sets AISH_HOME for this process).");
    println!("  -s, --session-dir     Specify a session directory (for resume). Without -s, a new unique session is used each time.");
    println!("  -v, --verbose         Emit verbose debug logs (for troubleshooting).");
    println!("  --generate <shell>    Generate shell completion script (bash, zsh, fish). Source the output to enable tab completion.");
    println!("  <command>             Command to execute. Omit to start the interactive shell.");
    println!("  [args...]             Arguments for the command.");
    println!();
    println!("Environment:");
    println!("  AISH_HOME       Home directory (config, profiles.json, task.d, system.d). Default: $XDG_CONFIG_HOME/aish or ~/.config/aish.");
    println!("  AISH_SESSION   Session directory; set by aish for child processes (e.g. ai). Use -s or -d to scope clear/resume.");
    println!();
    println!("Implemented commands:");
    println!("  clear                  Clear all part files in the session directory (delete conversation history).");
    println!("  truncate_console_log   Truncate console buffer and log file (used by ai command).");
    println!("  rollout                Flush console buffer and rollover console log (same as sending SIGUSR1 to aish).");
    println!("  mute                   Rollout console log and stop recording console.txt.");
    println!("  unmute                 Resume recording console.txt.");
    println!("  init                   Copy default config (set AISH_DEFAULTS_DIR or use --defaults-dir).");
    println!();
    println!("  sysq list              List system prompts and their enabled state.");
    println!("  sysq enable <id>...    Enable system prompt(s).");
    println!("  sysq disable <id>...   Disable system prompt(s).");
    println!();
    println!("  memory list            List all memories (id, category, subject).");
    println!("  memory get <id> [id...] Get memory content by ID(s).");
    println!("  memory remove <id> [id...] Remove memory by ID(s).");
}

#[cfg(unix)]
fn print_sysq_list(entries: &[crate::ports::outbound::SysqListEntry]) {
    println!("{:8} {:7} {:<20} {}", "SCOPE", "ENABLED", "ID", "TITLE");
    for e in entries {
        let enabled = if e.enabled { "yes" } else { "no" };
        let title = if e.title.len() > 40 { format!("{}...", &e.title[..e.title.floor_char_boundary(37)]) } else { e.title.clone() };
        println!("{:8} {:7} {:<20} {}", e.scope.as_str(), enabled, e.id, title);
    }
}

#[cfg(unix)]
fn print_memory_list(entries: &[crate::domain::MemoryListEntry]) {
    if entries.is_empty() {
        println!("(no memories)");
        return;
    }
    println!("{:18} {:<16} {}", "ID", "CATEGORY", "SUBJECT");
    for e in entries {
        let subject = if e.subject.len() > 50 {
            format!("{}...", &e.subject[..e.subject.floor_char_boundary(47)])
        } else {
            e.subject.clone()
        };
        println!("{:18} {:<16} {}", e.id, e.category, subject);
    }
}

#[cfg(unix)]
fn print_memory_get(entries: &[crate::domain::MemoryEntry]) {
    for (i, e) in entries.iter().enumerate() {
        if entries.len() > 1 {
            println!("--- {} (id={}) ---", e.subject, e.id);
        }
        println!("{}", e.content);
        if i + 1 < entries.len() {
            println!();
        }
    }
}

pub fn run() -> Result<i32, Error> {
    let outcome = parse_args()?;
    let config = match &outcome {
        ParseOutcome::Config(c) => c.clone(),
        ParseOutcome::GenerateCompletion(shell) => {
            print_completion(*shell);
            return Ok(0);
        }
    };
    #[cfg(unix)]
    {
        // -d/--home-dir 指定時は AISH_HOME を設定し、resolve_dirs() がその配下を使うようにする
        if let Some(ref h) = config.home_dir {
            std::env::set_var("AISH_HOME", h);
        }
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
            Command::Rollout => app.rollout_use_case.run(&path_input),
            Command::Mute => app.mute_use_case.run(&path_input),
            Command::Unmute => app.unmute_use_case.run(&path_input),
            Command::Clear => {
                let session_explicitly_specified = is_session_explicitly_specified_for_test(&config);
                app.clear_use_case.run(&path_input, session_explicitly_specified)
            }
            Command::Resume { .. } => Err(Error::invalid_argument(
                "resume command is not available in run_app (use aish binary).".to_string(),
            )),
            Command::Sessions => {
                let ids = app.sessions_use_case.list(&path_input)?;
                // run_app では標準出力の内容は検証しないため、結果の有無にかかわらず成功とみなす
                let _ = ids;
                Ok(0)
            }
            Command::Init { force, dry_run, defaults_dir: defaults_dir_opt } => {
                let defaults_dir = defaults_dir_opt
                    .or_else(|| std::env::var("AISH_DEFAULTS_DIR").ok())
                    .ok_or_else(|| Error::invalid_argument(
                        "Defaults directory required in test: set AISH_DEFAULTS_DIR or use init_defaults_dir in config".to_string(),
                    ))?;
                let input = crate::usecase::InitInput {
                    defaults_dir: std::path::PathBuf::from(&defaults_dir),
                    force,
                    dry_run,
                };
                let _ = app.init_use_case.run(&input)?;
                Ok(0)
            }
            Command::SysqList
            | Command::SysqEnable { .. }
            | Command::SysqDisable { .. } => Err(Error::invalid_argument(
                "sysq commands are not available in run_app (use aish binary).".to_string(),
            )),
            Command::MemoryList => {
                let _ = app.memory_use_case.list()?;
                Ok(0)
            }
            Command::MemoryGet { ids } => {
                if ids.is_empty() {
                    return Err(Error::invalid_argument("memory get requires at least one id".to_string()));
                }
                let _ = app.memory_use_case.get(&ids)?;
                Ok(0)
            }
            Command::MemoryRemove { ids } => {
                if ids.is_empty() {
                    return Err(Error::invalid_argument("memory remove requires at least one id".to_string()));
                }
                app.memory_use_case.remove(&ids)?;
                Ok(0)
            }
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
            let reviewed_dir = session_path.join("reviewed");

            if home_path.exists() {
                fs::remove_dir_all(&home_path).unwrap();
            }
            if session_path.exists() {
                fs::remove_dir_all(&session_path).unwrap();
            }

            fs::create_dir_all(&home_path).unwrap();
            fs::create_dir_all(&session_path).unwrap();
            fs::create_dir_all(&reviewed_dir).unwrap();

            fs::write(session_path.join("part_00000001_user.txt"), "Hello").unwrap();
            fs::write(session_path.join("part_00000002_assistant.txt"), "Hi there").unwrap();
            fs::write(session_path.join("part_00000003_user.txt"), "How are you?").unwrap();
            fs::write(session_path.join("reviewed_ABC12001_user.txt"), "Reviewed user (legacy)").unwrap();
            fs::write(session_path.join("reviewed_ABC12002_assistant.txt"), "Reviewed assistant (legacy)").unwrap();
            fs::write(reviewed_dir.join("reviewed_ABC12003_user.txt"), "Reviewed user").unwrap();
            fs::write(reviewed_dir.join("reviewed_ABC12004_assistant.txt"), "Reviewed assistant").unwrap();
            let evacuated_dir = session_path.join("leakscan_evacuated");
            fs::create_dir_all(&evacuated_dir).unwrap();
            fs::write(evacuated_dir.join("part_old_user.txt"), "evacuated").unwrap();
            fs::write(session_path.join("manifest.jsonl"), "{\"kind\":\"message\"}\n").unwrap();
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
            assert!(!session_path.join("reviewed_ABC12001_user.txt").exists());
            assert!(!session_path.join("reviewed_ABC12002_assistant.txt").exists());
            assert!(!reviewed_dir.exists());
            assert!(!session_path.join("leakscan_evacuated").exists());
            assert!(!session_path.join("manifest.jsonl").exists());
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
