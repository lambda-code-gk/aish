//! aish — シェルコマンド実行と JSONL ログ記録。

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

use aish::adapters::inbound::{AishCli, AishCommand, ReplayCommand};
use aish::adapters::outbound::toml_config::AishConfig;
use aish::adapters::outbound::{
    create_shell_session, pick_entry, prune_old_sessions, read_log_events, read_session_info,
    require_interactive_tty, resolve_replay_log_path, resolve_sessions_parent,
    session_dir_from_env, JsonlFileLog, PickerEntry, ProcessShell, PtyShell, ReplayLogReadError,
    ReplayLogResolveError, SessionReadError, SessionStoreError,
};
use aish::application::{
    format_picker_line, format_session, replay_list, replay_show, replay_span_views,
    resolve_replay_index, ExecuteAndRecord, ReplayError, RunShell,
};
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
        AishCommand::HumanShell { result_file } => {
            aish::human_shell::run_human_shell(&result_file)?;
            Ok(0)
        }
        AishCommand::CollaborativePrompt => {
            let prefix = aish::collaborative_prompt::render_collaborative_prompt_prefix();
            io::stdout()
                .write_all(prefix.as_bytes())
                .map_err(|e| anyhow::anyhow!(e))?;
            Ok(0)
        }
        AishCommand::Session { format } => run_session(format.into()),
        AishCommand::Replay { command } => run_replay(command),
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

fn run_replay(command: ReplayCommand) -> anyhow::Result<u8> {
    match command {
        ReplayCommand::List { log, index, format } => {
            let path = resolve_replay_log_path(log.as_deref()).map_err(replay_resolve_to_anyhow)?;
            let events = read_log_events(&path).map_err(replay_read_to_anyhow)?;
            let out = replay_list(&events, index, format).map_err(replay_to_anyhow)?;
            io::stdout()
                .write_all(out.as_bytes())
                .map_err(|e| anyhow::anyhow!(e))?;
            Ok(0)
        }
        ReplayCommand::Show {
            log,
            index,
            index_long,
            stderr,
        } => {
            let spec = index
                .or(index_long)
                .ok_or_else(|| anyhow::anyhow!(ReplayError::IndexRequired))?;
            let path = resolve_replay_log_path(log.as_deref()).map_err(replay_resolve_to_anyhow)?;
            let events = read_log_events(&path).map_err(replay_read_to_anyhow)?;
            let views = replay_span_views(&events).map_err(replay_to_anyhow)?;
            let index = resolve_replay_index(&views, spec).map_err(replay_to_anyhow)?;
            let out = replay_show(&events, index, stderr).map_err(replay_to_anyhow)?;
            io::stdout()
                .write_all(out.as_bytes())
                .map_err(|e| anyhow::anyhow!(e))?;
            Ok(0)
        }
        ReplayCommand::Pick { log, index, stderr } => {
            require_interactive_tty().map_err(replay_picker_to_anyhow)?;
            let path = resolve_replay_log_path(log.as_deref()).map_err(replay_resolve_to_anyhow)?;
            let events = read_log_events(&path).map_err(replay_read_to_anyhow)?;
            let picked = if let Some(index) = index {
                if !replay_span_views(&events)
                    .map_err(replay_to_anyhow)?
                    .iter()
                    .any(|view| view.index == index)
                {
                    return Err(anyhow::anyhow!(ReplayError::IndexNotFound(index)));
                }
                index
            } else {
                let views = replay_span_views(&events).map_err(replay_to_anyhow)?;
                let entries: Vec<PickerEntry> = views
                    .iter()
                    .map(|view| PickerEntry {
                        index: view.index,
                        line: format_picker_line(view),
                    })
                    .collect();
                pick_entry(&entries).map_err(replay_picker_to_anyhow)?
            };
            let out = replay_show(&events, picked, stderr).map_err(replay_to_anyhow)?;
            io::stdout()
                .write_all(out.as_bytes())
                .map_err(|e| anyhow::anyhow!(e))?;
            Ok(0)
        }
    }
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

fn replay_resolve_to_anyhow(e: ReplayLogResolveError) -> anyhow::Error {
    anyhow::anyhow!(e)
}

fn replay_to_anyhow(e: ReplayError) -> anyhow::Error {
    anyhow::anyhow!(e)
}

fn replay_read_to_anyhow(e: ReplayLogReadError) -> anyhow::Error {
    anyhow::anyhow!(e)
}

fn replay_picker_to_anyhow(e: aish::adapters::outbound::ReplayPickerError) -> anyhow::Error {
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

    #[test]
    fn cli_includes_replay_subcommand() {
        let cmd = AishCli::command();
        assert!(cmd.find_subcommand("replay").is_some());
    }
}
