//! aish — シェルコマンド実行と JSONL ログ記録。

use std::path::PathBuf;
use std::process::ExitCode;

use aish::adapters::outbound::{JsonlFileLog, ProcessShell};
use aish::application::ExecuteAndRecord;
use aish::domain::CommandSpec;

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
    if args.first().map(String::as_str) != Some("exec") {
        anyhow::bail!("usage: aish exec [--log PATH] -- <program> [args...]");
    }
    let rest = &args[1..];

    let (log_path, cmd_args) = parse_exec_args(rest)?;
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

fn parse_exec_args(args: &[String]) -> anyhow::Result<(PathBuf, Vec<String>)> {
    let mut log_path = default_log_path();
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

fn default_log_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let name = format!("session-{}.jsonl", std::process::id());
    PathBuf::from(home)
        .join(".local/share/aish/sessions")
        .join(name)
}
