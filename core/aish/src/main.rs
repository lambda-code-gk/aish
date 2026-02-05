mod adapter;
mod cli;
mod domain;
mod ports;
mod usecase;
mod wiring;

use std::process;
use common::error::Error;
use cli::{config_to_command, parse_args, Config};
use domain::command::Command;
use ports::inbound::UseCaseRunner;
#[cfg(unix)]
use usecase::{ClearUseCase, ShellUseCase, SysqUseCase, TruncateConsoleLogUseCase};
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

        match command {
            Command::Help => {
                print_help();
                Ok(0)
            }
            Command::Shell => {
                let use_case = ShellUseCase::from_app(&self.app);
                use_case.run(&config)
            }
            Command::TruncateConsoleLog => {
                let use_case = TruncateConsoleLogUseCase::from_app(&self.app);
                use_case.run(&config)
            }
            Command::Clear => {
                let use_case = ClearUseCase::from_app(&self.app);
                let session_explicitly_specified = is_session_explicitly_specified(&config);
                use_case.run(&config, session_explicitly_specified)
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
    // 実際の引数解析は環境変数に依存するため、
    // 統合テストで確認する
}
