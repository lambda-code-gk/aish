//! aish — シェルコマンド実行と JSONL ログ記録。

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use aish::adapters::inbound::{strip_common_options, CommonOptionsError};
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
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("aish: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<u8> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first().map(String::as_str) else {
        print_usage();
        anyhow::bail!("missing subcommand");
    };
    let cmd = cmd.to_string();
    args.remove(0);
    match cmd.as_str() {
        "exec" => run_exec(&mut args),
        "shell" => run_shell(&mut args),
        "session" => run_session(&mut args),
        _ => {
            print_usage();
            anyhow::bail!("unknown subcommand: {cmd}");
        }
    }
}

fn run_session(args: &mut Vec<String>) -> anyhow::Result<u8> {
    let common = strip_common_options(args).map_err(common_options_to_anyhow)?;
    if !args.is_empty() {
        anyhow::bail!("usage: aish session [--format tsv|json|env]");
    }
    let dir = session_dir_from_env().map_err(session_read_to_anyhow)?;
    let info = read_session_info(&dir).map_err(session_read_to_anyhow)?;
    let out = format_session(&info, common.format);
    io::stdout()
        .write_all(out.as_bytes())
        .map_err(|e| anyhow::anyhow!(e))?;
    Ok(0)
}

fn run_exec(args: &mut Vec<String>) -> anyhow::Result<u8> {
    let _common = strip_common_options(args).map_err(common_options_to_anyhow)?;
    let (log_path, cmd_args) = parse_exec_log_args(args)?;
    let program = cmd_args
        .first()
        .ok_or_else(|| anyhow::anyhow!("missing command after --"))?;
    let command_args: Vec<String> = cmd_args[1..].to_vec();

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

fn run_shell(args: &mut Vec<String>) -> anyhow::Result<u8> {
    let _common = strip_common_options(args).map_err(common_options_to_anyhow)?;
    reject_shell_log_flag(args)?;
    let cfg = AishConfig::load();
    let parent = resolve_sessions_parent(&cfg);
    prune_old_sessions(&parent, cfg.max_sessions).map_err(session_store_to_anyhow)?;
    let layout = create_shell_session(&parent).map_err(session_store_to_anyhow)?;

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

fn reject_shell_log_flag(rest: &[String]) -> anyhow::Result<()> {
    let mut i = 0;
    while i < rest.len() {
        if rest[i] == "--log" {
            anyhow::bail!(
                "aish shell: --log is not supported; logs are written under the session directory (see aish config log_dir)"
            );
        }
        i += 1;
    }
    Ok(())
}

fn common_options_to_anyhow(e: CommonOptionsError) -> anyhow::Error {
    anyhow::anyhow!(e)
}

fn session_store_to_anyhow(e: SessionStoreError) -> anyhow::Error {
    anyhow::anyhow!(e)
}

fn session_read_to_anyhow(e: SessionReadError) -> anyhow::Error {
    anyhow::anyhow!(e)
}

fn parse_exec_log_args(args: &[String]) -> anyhow::Result<(PathBuf, Vec<String>)> {
    let cfg = AishConfig::load();
    let mut log_path = cfg.default_exec_log();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--log" {
            let path = args
                .get(i + 1)
                .ok_or_else(|| anyhow::anyhow!("--log requires a path"))?;
            log_path = PathBuf::from(path);
            i += 2;
            continue;
        }
        if args[i] == "--" {
            return Ok((log_path, args[i + 1..].to_vec()));
        }
        i += 1;
    }
    anyhow::bail!("missing -- before command");
}

fn print_usage() {
    eprintln!(
        "usage:\n  \
         aish exec [--format tsv|json|env] [--log PATH] -- <program> [args...]\n  \
         aish shell [--format tsv|json|env]\n  \
         aish session [--format tsv|json|env]"
    );
}
