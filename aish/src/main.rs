//! aish — シェルコマンド実行と JSONL ログ記録。

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

use aish::adapters::inbound::{AishCli, AishCommand};
use aish::adapters::outbound::toml_config::AishConfig;
use aish::adapters::outbound::{
    create_shell_session, prune_old_sessions, read_session_info, resolve_sessions_parent,
    session_dir_from_env, JsonlFileLog, ProcessShell, PtyShell, SessionReadError,
    SessionStoreError,
};
use aish::application::{format_session, ExecuteAndRecord, RunShell};
use aish::domain::{CommandSpec, LogEvent};
use aish::ports::outbound::SessionLog;

fn main() -> ExitCode {
    if AishCli::try_complete_env() {
        return ExitCode::SUCCESS;
    }

    match run() {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("aish: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<u8> {
    let cli = AishCli::parse();
    match cli.command {
        AishCommand::Exec {
            format: _,
            log,
            command,
        } => run_exec(log, command),
        AishCommand::Shell { format: _ } => run_shell(),
        AishCommand::Session { format } => run_session(format.into()),
        AishCommand::Complete { shell } => {
            AishCli::run_complete(shell)?;
            Ok(0)
        }
    }
}

fn run_session(format: aish::domain::OutputFormat) -> anyhow::Result<u8> {
    let dir = session_dir_from_env().map_err(session_read_to_anyhow)?;
    let info = read_session_info(&dir).map_err(session_read_to_anyhow)?;
    let out = format_session(&info, format);
    io::stdout()
        .write_all(out.as_bytes())
        .map_err(|e| anyhow::anyhow!(e))?;
    Ok(0)
}

fn run_exec(log: Option<PathBuf>, command: Vec<String>) -> anyhow::Result<u8> {
    let cfg = AishConfig::load();
    let log_path = log.unwrap_or_else(|| cfg.default_exec_log());
    let program = command
        .first()
        .ok_or_else(|| anyhow::anyhow!("missing command after --"))?;
    let command_args: Vec<String> = command[1..].to_vec();

    let spec = CommandSpec {
        program: program.clone(),
        args: command_args,
    };

    let log = JsonlFileLog::new(log_path);
    let log_path_display = log.path().display().to_string();
    let mut app = ExecuteAndRecord::new(ProcessShell, log);
    let result = app.run(spec)?;

    eprintln!("aish: log written to {log_path_display}");
    Ok(result.exit_code.unwrap_or(0) as u8)
}

fn run_shell() -> anyhow::Result<u8> {
    let cfg = AishConfig::load();
    let parent = resolve_sessions_parent(&cfg);
    let layout = create_shell_session(&parent).map_err(session_store_to_anyhow)?;
    prune_old_sessions(&parent, cfg.max_sessions).map_err(session_store_to_anyhow)?;

    eprintln!("aish: session {} (dir {})", layout.id, layout.dir.display());

    let shell = cfg.shell;
    let mut log = JsonlFileLog::new(layout.log_path.clone());
    log.append(&LogEvent::command_start(&CommandSpec {
        program: "interactive_shell".to_string(),
        args: vec![shell.clone()],
    }))
    .map_err(|e| anyhow::anyhow!(e))?;
    let mut runner = PtyShell::new(&mut log);
    let mut app = RunShell::new(&mut runner);
    let code = app.run(&shell, &layout.dir)?;
    log.append(&LogEvent::Exit { code: Some(code) })
        .map_err(|e| anyhow::anyhow!(e))?;

    eprintln!("aish: log written to {}", layout.log_path.display());
    Ok(code as u8)
}

fn session_store_to_anyhow(e: SessionStoreError) -> anyhow::Error {
    anyhow::anyhow!(e)
}

fn session_read_to_anyhow(e: SessionReadError) -> anyhow::Error {
    anyhow::anyhow!(e)
}

#[cfg(test)]
mod cli_tests {
    use clap::CommandFactory;

    use aish::adapters::inbound::AishCli;

    #[test]
    fn cli_includes_complete_subcommand() {
        let cmd = AishCli::command();
        assert!(cmd.find_subcommand("complete").is_some());
    }
}
