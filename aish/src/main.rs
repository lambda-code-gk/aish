//! aish — シェルコマンド実行と JSONL ログ記録。

use std::path::PathBuf;
use std::process::ExitCode;

use aish::adapters::outbound::toml_config::AishConfig;
use aish::adapters::outbound::{JsonlFileLog, ProcessShell, PtyShell};
use aish::application::{ExecuteAndRecord, RunShell};
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
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first().map(String::as_str) else {
        anyhow::bail!("usage: aish exec|shell [--log PATH] ...");
    };
    match cmd {
        "exec" => run_exec(&args[1..]),
        "shell" => run_shell(&args[1..]),
        _ => anyhow::bail!("usage: aish exec|shell [--log PATH] ..."),
    }
}

fn run_exec(rest: &[String]) -> anyhow::Result<u8> {
    let (log_path, cmd_args) = parse_log_args(rest, true)?;
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

fn run_shell(rest: &[String]) -> anyhow::Result<u8> {
    let cfg = AishConfig::load();
    let (log_path, _) = parse_log_args(rest, false)?;
    let shell = cfg.shell;

    let mut log = JsonlFileLog::new(log_path);
    let log_path_display = log.path().display().to_string();
    log.append(&LogEvent::command_start(&CommandSpec {
        program: "interactive_shell".to_string(),
        args: vec![shell.clone()],
    }))
    .map_err(|e| anyhow::anyhow!(e))?;
    let mut runner = PtyShell::new(&mut log);
    let mut app = RunShell::new(&mut runner);
    let code = app.run(&shell)?;
    log.append(&LogEvent::Exit { code: Some(code) })
        .map_err(|e| anyhow::anyhow!(e))?;

    eprintln!("aish: log written to {log_path_display}");
    Ok(code as u8)
}

fn parse_log_args(
    args: &[String],
    require_separator: bool,
) -> anyhow::Result<(PathBuf, Vec<String>)> {
    let cfg = AishConfig::load();
    let mut log_path = cfg.default_session_log();
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
    if require_separator {
        anyhow::bail!("missing -- before command");
    }
    Ok((log_path, vec![]))
}
