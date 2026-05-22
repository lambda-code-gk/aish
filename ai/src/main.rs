//! ai — aibe クライアント。

#![cfg(unix)]

use std::path::PathBuf;
use std::process::ExitCode;

use ai::adapters::outbound::toml_config::AiConfig;
use ai::adapters::outbound::{AibeUnixClient, FileLogTail, StdoutPresenter};
use ai::application::Ask;
use aibe::client;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ai: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        anyhow::bail!("usage: ai ask <message> [--log PATH] [--socket PATH] [--no-start]");
    }
    if args[0] != "ask" {
        anyhow::bail!("usage: ai ask <message> [--log PATH] [--socket PATH] [--no-start]");
    }

    let mut message_parts = Vec::new();
    let mut log_path = None;
    let mut socket_path = AiConfig::load().socket_path;
    let mut auto_start = true;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--log" => {
                log_path = Some(
                    args.get(i + 1)
                        .ok_or_else(|| anyhow::anyhow!("--log requires a path"))?
                        .clone(),
                );
                i += 2;
            }
            "--socket" => {
                socket_path = PathBuf::from(
                    args.get(i + 1)
                        .ok_or_else(|| anyhow::anyhow!("--socket requires a path"))?
                        .clone(),
                );
                i += 2;
            }
            "--no-start" => {
                auto_start = false;
                i += 1;
            }
            part => {
                message_parts.push(part.to_string());
                i += 1;
            }
        }
    }

    if message_parts.is_empty() {
        anyhow::bail!("missing message");
    }
    let message = message_parts.join(" ");

    if auto_start {
        client::ensure_running(&socket_path).map_err(|e| anyhow::anyhow!(e))?;
    }

    let client = AibeUnixClient::new(socket_path);
    let presenter = StdoutPresenter;

    if let Some(path) = log_path {
        let log = FileLogTail::new(PathBuf::from(path));
        let ask = Ask::new(&client, &presenter, Some(&log));
        ask.run(message)?;
    } else {
        let ask = Ask::new(&client, &presenter, None::<&FileLogTail>);
        ask.run(message)?;
    }

    Ok(())
}
