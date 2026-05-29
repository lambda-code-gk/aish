//! ai — aibe クライアント。

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ai::adapters::outbound::toml_config::AiConfig;
use ai::adapters::outbound::{AibeUnixClient, FileLogTail, StdoutPresenter};
use ai::application::{ensure_aibe_if_needed, plan_ask_launch, Ask, AskRunOptions};
use ai::domain::{
    resolve_llm_profile, resolve_shell_log_for_ask, ShellLogChoice, ShellLogResolveError,
    ToolsResolveError,
};
use aibe_client::ensure_running;

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
        print_usage();
        anyhow::bail!("missing subcommand");
    }
    if args[0] != "ask" {
        print_usage();
        anyhow::bail!("unknown subcommand");
    }

    let mut message_parts = Vec::new();
    let mut log_path = None;
    let mut session_id = None;
    let mut no_log = false;
    let mut socket_path = None;
    let mut auto_start = true;
    let mut tools_cli = None;
    let mut profile_cli = None;
    let mut verbose_tools = false;

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
            "--session" => {
                session_id = Some(
                    args.get(i + 1)
                        .ok_or_else(|| anyhow::anyhow!("--session requires a session id"))?
                        .clone(),
                );
                i += 2;
            }
            "--no-log" => {
                no_log = true;
                i += 1;
            }
            "--socket" => {
                socket_path = Some(PathBuf::from(
                    args.get(i + 1)
                        .ok_or_else(|| anyhow::anyhow!("--socket requires a path"))?
                        .clone(),
                ));
                i += 2;
            }
            "--no-start" => {
                auto_start = false;
                i += 1;
            }
            "--tools" => {
                tools_cli = Some(
                    args.get(i + 1)
                        .ok_or_else(|| anyhow::anyhow!("--tools requires a list"))?
                        .clone(),
                );
                i += 2;
            }
            "--profile" => {
                profile_cli = Some(
                    args.get(i + 1)
                        .ok_or_else(|| anyhow::anyhow!("--profile requires a name"))?
                        .clone(),
                );
                i += 2;
            }
            "--verbose-tools" => {
                verbose_tools = true;
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

    let log_choice = resolve_shell_log_for_ask(
        no_log,
        log_path.as_deref().map(Path::new),
        session_id.as_deref(),
        std::env::var("AI_ASK_LOG").ok().as_deref(),
        std::env::var("AISH_SESSION_DIR")
            .ok()
            .as_deref()
            .map(Path::new),
    )
    .map_err(shell_log_resolve_to_anyhow)?;

    if let ShellLogChoice::Path(ref path) = log_choice {
        eprintln!("ai: using shell log: {}", path.display());
    }

    let cfg = AiConfig::load();
    let socket_path = socket_path.unwrap_or(cfg.socket_path);
    let plan = plan_ask_launch(
        &cfg.ask_tools,
        tools_cli.as_deref(),
        socket_path,
        auto_start,
    )
    .map_err(tools_resolve_to_anyhow)?;

    ensure_aibe_if_needed(&plan, |path| {
        ensure_running(path).map_err(|e| anyhow::anyhow!(e))
    })?;

    let client = AibeUnixClient::new(plan.socket_path);
    let presenter = StdoutPresenter;
    let llm_profile =
        resolve_llm_profile(profile_cli.as_deref(), cfg.ask_default_profile.as_deref());

    let options = AskRunOptions {
        resolved_tools: plan.resolved_tools,
        verbose_tools,
        llm_profile,
    };

    match log_choice {
        ShellLogChoice::Path(path) => {
            let log = FileLogTail::new(path);
            let ask = Ask::new(&client, &presenter, Some(&log));
            ask.run(message, options)?;
        }
        ShellLogChoice::None => {
            let ask = Ask::new(&client, &presenter, None::<&FileLogTail>);
            ask.run(message, options)?;
        }
    }

    Ok(())
}

fn tools_resolve_to_anyhow(e: ToolsResolveError) -> anyhow::Error {
    anyhow::anyhow!(e)
}

fn shell_log_resolve_to_anyhow(e: ShellLogResolveError) -> anyhow::Error {
    anyhow::anyhow!(e)
}

fn print_usage() {
    eprintln!(
        "usage: ai ask <message> [--log PATH] [--session ID] [--no-log] [--socket PATH] [--no-start] [--tools LIST] [--profile NAME] [--verbose-tools]"
    );
}
