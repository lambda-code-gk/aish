//! ai — aibe クライアント。

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;

use ai::adapters::outbound::toml_config::AiConfig;
use ai::adapters::outbound::{AibeUnixClient, FileLogTail, StdoutPresenter};
use ai::application::{ensure_aibe_if_needed, plan_ask_launch, Ask, AskRunOptions};
use ai::clap_cli::{AiCli, AiCommand};
use ai::domain::{
    resolve_llm_profile, resolve_output_filter, resolve_shell_log_for_ask, validate_ask_arg_order,
    ShellLogChoice, ShellLogResolveError, ToolsResolveError,
};
use aibe_client::ensure_running;

fn main() -> ExitCode {
    if AiCli::try_complete_env() {
        return ExitCode::SUCCESS;
    }

    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ai: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<()> {
    let raw: Vec<String> = std::env::args().collect();
    if raw.get(1).map(String::as_str) == Some("ask") {
        validate_ask_arg_order(&raw[2..]).map_err(|e| anyhow::anyhow!(e))?;
    }

    let cli = AiCli::parse();
    match cli.command {
        AiCommand::Complete { shell } => AiCli::run_complete(shell).map_err(|e| anyhow::anyhow!(e)),
        AiCommand::Ask {
            log,
            session,
            no_log,
            socket,
            no_start,
            tools,
            profile,
            verbose_tools,
            message,
        } => run_ask(
            message,
            log,
            session,
            no_log,
            socket,
            no_start,
            tools,
            profile,
            verbose_tools,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_ask(
    message_parts: Vec<String>,
    log_path: Option<PathBuf>,
    session_id: Option<String>,
    no_log: bool,
    socket_path: Option<PathBuf>,
    auto_start: bool,
    tools_cli: Option<String>,
    profile_cli: Option<String>,
    verbose_tools: bool,
) -> anyhow::Result<()> {
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
    let output_filter =
        resolve_output_filter(std::env::var("AI_FILTER").ok(), cfg.ask_filter.as_deref());
    let presenter = StdoutPresenter::new(output_filter);
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

#[cfg(test)]
mod cli_tests {
    use clap::CommandFactory;

    use ai::clap_cli::AiCli;
    use ai::domain::{validate_ask_arg_order, AskArgOrderError};

    #[test]
    fn ask_rejects_options_after_message() {
        let err =
            validate_ask_arg_order(&["hello".into(), "--log".into(), "/tmp/x".into()]).unwrap_err();
        assert_eq!(err, AskArgOrderError);
    }

    #[test]
    fn cli_includes_complete_subcommand() {
        let cmd = AiCli::command();
        assert!(cmd.find_subcommand("complete").is_some());
    }
}
