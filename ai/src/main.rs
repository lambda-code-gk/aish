//! ai — aibe クライアント。

#![cfg(unix)]

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use clap::Parser;

use ai::adapters::outbound::toml_config::AiConfig;
use ai::adapters::outbound::{
    external_command_names, AibeUnixClient, FileLogTail, LocalHistoryStore, StdoutPresenter,
    YesExecCache,
};
use ai::application::{
    build_response_summary, build_summary, ensure_aibe_if_needed, list_history, next_history_id,
    plan_ask_launch, record_turn, HistoryRecordInput, HistoryReplayInput, TurnCancelGuard,
};
use ai::clap_cli::{AiCli, AiCommand, HistoryStatusArg, OutputFormatArg, TurnOptions};
use ai::domain::{
    resolve_log_tail_bytes, resolve_output_filter, resolve_shell_log_for_ask,
    validate_ask_arg_order, AskInput, ConfigToolsTokens, DiagnosticsReport, DryRunReport,
    HistoryIndexFilter, HistoryRecordKind, HistoryRecordStatus, OutputFormat, ShellLogChoice,
    ShellLogResolveError, ToolsResolveError,
};
use ai::ports::outbound::Presenter;
use ai::ports::outbound::{HistoryStore, LogReadError, ShellLogSource};
use aibe_client::{ensure_running, ping_detailed, AgentTurnProgressEvent, ShellExecApprovalPrompt};
use aibe_protocol::{ClientRequest, ClientResponse, ProtocolMessage, RequestContext};

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
    let normalized = AiCli::normalized_args_for_completion();
    validate_normalized_ask_args(&normalized)?;
    let cli = AiCli::parse_from(normalized);

    match cli.command {
        AiCommand::Complete { shell } => AiCli::run_complete(shell).map_err(|e| anyhow::anyhow!(e)),
        AiCommand::Ask {
            turn,
            file,
            message,
        } => run_ask(AskArgs {
            turn,
            file,
            message,
        }),
        AiCommand::Chat { turn } => run_chat(turn),
        AiCommand::Retry { turn, history_id } => run_retry(turn, history_id),
        AiCommand::Rerun { turn, history_id } => run_rerun(turn, history_id),
        AiCommand::History {
            quiet,
            format,
            limit,
            session,
            command,
            status,
        } => run_history(HistoryArgs {
            quiet,
            format,
            limit,
            session,
            command,
            status,
        }),
        AiCommand::Status {
            quiet,
            format,
            socket,
        } => run_diagnostic_command("status", quiet, format.into(), socket, false),
        AiCommand::Doctor {
            quiet,
            format,
            socket,
        } => run_diagnostic_command("doctor", quiet, format.into(), socket, true),
        AiCommand::Ping {
            quiet,
            format,
            socket,
        } => run_ping_command(quiet, format.into(), socket),
    }
}

#[derive(Debug)]
struct AskArgs {
    turn: TurnOptions,
    file: Option<PathBuf>,
    message: Vec<String>,
}

#[derive(Debug)]
struct HistoryArgs {
    quiet: bool,
    format: OutputFormatArg,
    limit: Option<usize>,
    session: Option<String>,
    command: Option<String>,
    status: Option<HistoryStatusArg>,
}

#[derive(Debug, Clone)]
struct ResolvedTurnSettings {
    quiet: bool,
    output_format: Option<OutputFormat>,
    preset_name: Option<String>,
    log_tail_bytes: usize,
    socket_path: PathBuf,
    session_id: Option<String>,
    shell_log_choice: ShellLogChoice,
    output_filter: Option<String>,
    llm_profile: Option<String>,
    ask_tools: ConfigToolsTokens,
    tools_cli: Option<String>,
    no_start: bool,
    verbose_tools: bool,
    progress: bool,
    timeout_secs: Option<u64>,
    yes_exec: bool,
}

fn run_ask(args: AskArgs) -> anyhow::Result<()> {
    let cfg = AiConfig::load();
    let message = resolve_ask_message(args.file.clone(), args.message)?;
    let settings = resolve_turn_settings(&cfg, &args.turn)?;
    if args.turn.dry_run {
        let report = DryRunReport {
            command: "ask".to_string(),
            message_source: message.source,
            message_length: message.content.len(),
            message_masked: "<masked>".to_string(),
            config_socket_path: cfg.socket_path.display().to_string(),
            ask_default_profile: cfg.ask_default_profile.clone(),
            ask_filter: cfg.ask_filter.clone(),
            ask_tools: cfg.ask_tools.0.clone(),
            socket_path: settings.socket_path.display().to_string(),
            aish_session_dir: std::env::var("AISH_SESSION_DIR").ok(),
            implicit_session_id: implicit_session_id_from_env(),
            ai_ask_log: std::env::var("AI_ASK_LOG").ok(),
            shell_log_choice: render_shell_log_choice(&settings.shell_log_choice),
            shell_log_path: render_shell_log_path(&settings.shell_log_choice),
            shell_log_error: None,
            dry_run: true,
            preset: settings.preset_name.clone(),
            log_tail_bytes: settings.log_tail_bytes,
        };
        let format = settings.output_format.unwrap_or(OutputFormat::Tsv);
        write_stdout(report.render(format))?;
        return Ok(());
    }
    execute_turn(&cfg, "ask", message, settings, None, None)
}

fn run_chat(turn: TurnOptions) -> anyhow::Result<()> {
    let cfg = AiConfig::load();
    let settings = resolve_turn_settings(&cfg, &turn)?;
    let mut line = String::new();
    loop {
        line.clear();
        eprint!("ai> ");
        use std::io::Write;
        std::io::stderr().flush().ok();
        let n = std::io::stdin().read_line(&mut line)?;
        if n == 0 {
            break;
        }
        let content = line.trim_end().to_string();
        if content.is_empty() || content == "/exit" {
            if content == "/exit" {
                break;
            }
            continue;
        }
        execute_turn(
            &cfg,
            "chat",
            ResolvedMessage {
                source: "chat".to_string(),
                content,
            },
            settings.clone(),
            None,
            None,
        )?;
    }
    Ok(())
}

fn run_retry(turn: TurnOptions, history_id: String) -> anyhow::Result<()> {
    let cfg = AiConfig::load();
    let store = LocalHistoryStore::new(cfg.history_dir.clone());
    let payload = store
        .load_payload(&history_id)
        .map_err(history_store_to_anyhow)?;
    let settings = resolve_turn_settings(&cfg, &turn)?;
    let message = ResolvedMessage {
        source: format!("history:{history_id}"),
        content: payload.user_message,
    };
    execute_turn(&cfg, "retry", message, settings, None, None)
}

fn run_rerun(turn: TurnOptions, history_id: String) -> anyhow::Result<()> {
    let cfg = AiConfig::load();
    let store = LocalHistoryStore::new(cfg.history_dir.clone());
    let payload = store
        .load_payload(&history_id)
        .map_err(history_store_to_anyhow)?;
    let message = ResolvedMessage {
        source: format!("history:{history_id}"),
        content: payload.user_message.clone(),
    };
    let mut merged_turn = turn;
    if merged_turn.preset.is_none() {
        merged_turn.preset = payload.preset.clone();
    }
    if merged_turn.session.is_none() {
        merged_turn.session = payload.session_id.clone();
    }
    if merged_turn.socket.is_none() {
        merged_turn.socket = Some(PathBuf::from(payload.socket_path.clone()));
    }
    if merged_turn.tools.is_none() && !payload.tools.is_empty() {
        merged_turn.tools = Some(payload.tools.join(","));
    }
    if merged_turn.profile.is_none() {
        merged_turn.profile = payload.llm_profile.clone();
    }
    if merged_turn.log_tail.is_none() {
        merged_turn.log_tail = Some(payload.log_tail_bytes);
    }
    merged_turn.no_log = true;
    merged_turn.log = None;
    merged_turn.session = None;
    execute_turn(
        &cfg,
        "rerun",
        message,
        resolve_turn_settings(&cfg, &merged_turn)?,
        payload.shell_log_tail.clone(),
        payload.client_cwd.map(PathBuf::from),
    )
}

fn run_history(args: HistoryArgs) -> anyhow::Result<()> {
    let cfg = AiConfig::load();
    let store = LocalHistoryStore::new(cfg.history_dir.clone());
    let status = match args.status {
        Some(HistoryStatusArg::Ok) => Some(HistoryRecordStatus::Ok),
        Some(HistoryStatusArg::Error) => Some(HistoryRecordStatus::Error),
        None => None,
    };
    let filter = HistoryIndexFilter {
        session_id: validate_explicit_session(args.session.as_deref())?,
        command: args.command,
        status,
        limit: args.limit.unwrap_or(20),
    };
    let entries = list_history(&store, filter).map_err(history_store_to_anyhow)?;
    let entry_count = entries.len();
    let format = OutputFormat::from(args.format);
    let stdout = match format {
        OutputFormat::Json => serde_json::to_string(&entries)?,
        OutputFormat::Tsv => entries
            .iter()
            .map(|entry| entry.render_tsv())
            .collect::<String>(),
        OutputFormat::Env => entries
            .iter()
            .map(|entry| entry.render_env())
            .collect::<String>(),
    };
    if !args.quiet {
        eprintln!("ai: history: {} record(s)", entry_count);
    }
    write_stdout(stdout)?;
    Ok(())
}

fn execute_turn(
    cfg: &AiConfig,
    command: &str,
    message: ResolvedMessage,
    settings: ResolvedTurnSettings,
    shell_log_override: Option<String>,
    client_cwd_override: Option<PathBuf>,
) -> anyhow::Result<()> {
    let shell_log_choice = settings.shell_log_choice.clone();
    let shell_log_tail_text = if let Some(text) = shell_log_override {
        Some(text)
    } else {
        match &shell_log_choice {
            ShellLogChoice::Path(path) => Some(
                FileLogTail::new(path.clone())
                    .tail_bytes(settings.log_tail_bytes)
                    .map_err(|e: LogReadError| anyhow::anyhow!(e))?,
            ),
            ShellLogChoice::None => None,
        }
    };

    if let ShellLogChoice::Path(ref path) = shell_log_choice {
        if !settings.quiet {
            eprintln!("ai: using shell log: {}", path.display());
        }
    }

    let plan = plan_ask_launch(
        &settings.ask_tools,
        settings.tools_cli.as_deref(),
        settings.socket_path.clone(),
        !settings.no_start,
    )
    .map_err(tools_resolve_to_anyhow)?;
    ensure_aibe_if_needed(&plan, |path| {
        ensure_running(path).map_err(|e| anyhow::anyhow!(e))
    })?;

    let presenter = Arc::new(StdoutPresenter::with_options(
        settings.output_filter.clone(),
        settings.output_format,
        settings.quiet,
    ));
    presenter.show_tools_startup(&plan.resolved_tools.startup);
    presenter.show_external_commands(&external_command_names());
    let tool_names: Vec<String> = plan
        .resolved_tools
        .allowlist
        .names()
        .iter()
        .map(|t| t.as_str().to_string())
        .collect();

    let ask_input = AskInput {
        user_message: message.content.clone(),
        shell_log_tail: shell_log_tail_text.clone(),
        client_cwd: client_cwd_override
            .clone()
            .or_else(|| std::env::current_dir().ok()),
        tools: plan.resolved_tools.allowlist.clone().into_names(),
        llm_profile: settings.llm_profile.clone(),
    };
    let request = ask_input.into_request()?;
    let turn_id = next_history_id();
    let client_request = request_from_ask(turn_id.clone(), request)?;

    let response = if settings.timeout_secs.is_some() || settings.progress || settings.yes_exec {
        run_agent_turn_async(
            plan.socket_path.clone(),
            client_request,
            presenter.clone(),
            cfg.history_dir.clone(),
            settings.session_id.clone(),
            settings.yes_exec,
            settings.progress,
            settings.timeout_secs,
        )?
    } else {
        run_agent_turn_sync(
            plan.socket_path.clone(),
            client_request,
            presenter.clone(),
            cfg.history_dir.clone(),
            settings.session_id.clone(),
            settings.yes_exec,
            settings.progress,
        )?
    };

    let response_error = match &response {
        ClientResponse::Error { message, .. } => Some(message.clone()),
        ClientResponse::Cancelled { reason, .. } => Some(
            reason
                .clone()
                .unwrap_or_else(|| "turn cancelled".to_string()),
        ),
        _ => None,
    };
    let streamed = settings.progress || settings.timeout_secs.is_some() || settings.yes_exec;

    let tool_calls = match &response {
        ClientResponse::AgentTurnResult { tool_calls, .. } => tool_calls.len(),
        _ => 0,
    };
    let assistant_message_len = match &response {
        ClientResponse::AgentTurnResult {
            assistant_message, ..
        } => assistant_message.content.len(),
        _ => 0,
    };
    let request_summary = build_summary(
        &message.content,
        shell_log_tail_text.as_deref(),
        &tool_names,
    );
    let response_summary =
        build_response_summary(assistant_message_len, tool_calls, response_error.as_deref());
    let status = if response_error.is_some() {
        HistoryRecordStatus::Error
    } else {
        HistoryRecordStatus::Ok
    };
    let history_id = next_history_id();
    let record_input = HistoryRecordInput {
        command: command.to_string(),
        session_id: settings.session_id.clone(),
        conversation_id: None,
        preset: settings.preset_name.clone(),
        profile: settings.llm_profile.clone(),
        socket_path: settings.socket_path.display().to_string(),
        request_kind: match command {
            "retry" => HistoryRecordKind::Retry,
            "rerun" => HistoryRecordKind::Rerun,
            _ => HistoryRecordKind::Ask,
        },
        request_summary,
        response_kind: if response_error.is_some() {
            HistoryRecordKind::Error
        } else {
            HistoryRecordKind::Ask
        },
        response_summary,
        status,
    };
    let replay_input = HistoryReplayInput {
        history_id: history_id.clone(),
        command: command.to_string(),
        user_message: message.content,
        shell_log_tail: shell_log_tail_text.clone(),
        client_cwd: client_cwd_override
            .or_else(|| std::env::current_dir().ok())
            .map(|p| p.display().to_string()),
        tools: tool_names,
        llm_profile: settings.llm_profile.clone(),
        preset: settings.preset_name.clone(),
        session_id: settings.session_id.clone(),
        socket_path: settings.socket_path.display().to_string(),
        log_tail_bytes: settings.log_tail_bytes,
    };
    let store = LocalHistoryStore::new(cfg.history_dir.clone());
    record_turn(&store, &record_input, &replay_input).map_err(history_store_to_anyhow)?;
    presenter.show_response(&response, settings.verbose_tools, streamed);

    if response_error.is_some() {
        anyhow::bail!("aibe returned an error response");
    }
    Ok(())
}

fn run_agent_turn_sync(
    socket_path: PathBuf,
    request: ClientRequest,
    presenter: Arc<StdoutPresenter>,
    history_dir: PathBuf,
    session_id: Option<String>,
    yes_exec: bool,
    progress: bool,
) -> anyhow::Result<ClientResponse> {
    run_agent_turn_core(
        socket_path,
        request,
        presenter,
        history_dir,
        session_id,
        yes_exec,
        progress,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_agent_turn_async(
    socket_path: PathBuf,
    request: ClientRequest,
    presenter: Arc<StdoutPresenter>,
    history_dir: PathBuf,
    session_id: Option<String>,
    yes_exec: bool,
    progress: bool,
    timeout_secs: Option<u64>,
) -> anyhow::Result<ClientResponse> {
    run_agent_turn_core(
        socket_path,
        request,
        presenter,
        history_dir,
        session_id,
        yes_exec,
        progress,
        timeout_secs,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_agent_turn_core(
    socket_path: PathBuf,
    request: ClientRequest,
    presenter: Arc<StdoutPresenter>,
    history_dir: PathBuf,
    session_id: Option<String>,
    yes_exec: bool,
    progress: bool,
    timeout_secs: Option<u64>,
) -> anyhow::Result<ClientResponse> {
    let turn_id = request_turn_id(&request)?;
    let worker_client = AibeUnixClient::new(socket_path.clone());
    let cancel_client = AibeUnixClient::new(socket_path);
    let cancel_guard = TurnCancelGuard::new().map_err(|e| anyhow::anyhow!("{e}"))?;
    let cancel_requested = Arc::clone(cancel_guard.flag());

    let (tx, rx) = mpsc::channel();
    let presenter_thread = Arc::clone(&presenter);
    let history_dir_thread = history_dir.clone();
    let turn_id_thread = turn_id.clone();
    thread::spawn(move || {
        let mut yes_exec_cache = if yes_exec {
            Some(YesExecCache::load(
                &history_dir_thread,
                session_id.as_deref(),
            ))
        } else {
            None
        };
        let response = match worker_client.agent_turn_request_stream(
            request,
            |event: AgentTurnProgressEvent| {
                if progress {
                    let phase = progress_phase_name(event.phase);
                    presenter_thread.show_progress(&phase, event.message.as_deref());
                }
            },
            |chunk| {
                presenter_thread.show_stream_chunk(&chunk);
            },
            |prompt: ShellExecApprovalPrompt| {
                if let Some(Ok(cache)) = yes_exec_cache.as_ref() {
                    if cache.should_auto_approve(&prompt) {
                        return true;
                    }
                }
                let approved = ai::adapters::outbound::prompt_shell_exec_approval(prompt.clone());
                if approved {
                    if let Some(Ok(cache)) = yes_exec_cache.as_mut() {
                        let _ = cache.remember(&prompt);
                    }
                }
                approved
            },
        ) {
            Ok(resp) => resp,
            Err(e) => ClientResponse::Error {
                id: turn_id_thread.clone(),
                code: aibe_protocol::ErrorCode::InternalError,
                message: e.to_string(),
            },
        };
        let _ = tx.send(response);
    });

    let timeout = timeout_secs.map(Duration::from_secs);
    let start = Instant::now();
    loop {
        if cancel_requested.load(Ordering::SeqCst) {
            let _ = cancel_client.cancel_turn(&turn_id);
        }
        if let Some(deadline) = timeout {
            if start.elapsed() >= deadline && !cancel_requested.load(Ordering::SeqCst) {
                cancel_requested.store(true, Ordering::SeqCst);
                let _ = cancel_client.cancel_turn(&turn_id);
            }
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(resp) => {
                if matches!(
                    resp,
                    ClientResponse::Progress { .. } | ClientResponse::AssistantStreaming { .. }
                ) {
                    continue;
                }
                return Ok(resp);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(anyhow::anyhow!("agent turn worker disconnected"));
            }
        }
    }
}

fn request_turn_id(request: &ClientRequest) -> anyhow::Result<String> {
    match request {
        ClientRequest::AgentTurn { id, .. }
        | ClientRequest::Ping { id }
        | ClientRequest::ShellExecApproval { id, .. }
        | ClientRequest::CancelTurn { id, .. } => Ok(id.clone()),
    }
}

fn request_from_ask(
    turn_id: String,
    request: ai::domain::AskRequest,
) -> anyhow::Result<ClientRequest> {
    let messages = vec![ProtocolMessage {
        role: "user".to_string(),
        content: request.user_message,
    }];
    let context = RequestContext {
        shell_log_tail: request.shell_log_tail,
        cwd: request.client_cwd.map(|p| p.display().to_string()),
    };
    Ok(ClientRequest::AgentTurn {
        id: turn_id,
        messages,
        tools: request
            .tools
            .into_iter()
            .map(|t| t.as_str().to_string())
            .collect(),
        context,
        llm_profile: request.llm_profile,
    })
}

fn progress_phase_name(phase: aibe_protocol::ProgressPhase) -> String {
    match phase {
        aibe_protocol::ProgressPhase::Thinking => "thinking".to_string(),
        aibe_protocol::ProgressPhase::ToolCall => "tool_call".to_string(),
        aibe_protocol::ProgressPhase::WaitingApproval => "waiting_approval".to_string(),
        aibe_protocol::ProgressPhase::Finalizing => "finalizing".to_string(),
        aibe_protocol::ProgressPhase::Cancelling => "cancelling".to_string(),
    }
}

fn resolve_turn_settings(
    cfg: &AiConfig,
    turn: &TurnOptions,
) -> anyhow::Result<ResolvedTurnSettings> {
    let preset = turn
        .preset
        .as_deref()
        .and_then(|name| cfg.presets.get(name));
    if let Some(name) = turn.preset.as_deref() {
        if preset.is_none() {
            anyhow::bail!("unknown preset: {name}");
        }
    }
    let socket_path = turn
        .socket
        .clone()
        .unwrap_or_else(|| cfg.socket_path.clone());
    let log_tail_bytes = resolve_log_tail_bytes(
        turn.log_tail,
        preset.and_then(|p| p.log_tail_bytes),
        cfg.log_tail_bytes,
    )?;
    let shell_log_choice = resolve_shell_log_for_ask(
        turn.no_log,
        turn.log.as_deref().map(Path::new),
        turn.session.as_deref(),
        std::env::var("AI_ASK_LOG").ok().as_deref(),
        std::env::var("AISH_SESSION_DIR")
            .ok()
            .as_deref()
            .map(Path::new),
    )
    .map_err(shell_log_resolve_to_anyhow)?;
    let output_filter = resolve_output_filter(
        std::env::var("AI_FILTER").ok(),
        preset
            .and_then(|p| p.filter.as_deref())
            .or(cfg.ask_filter.as_deref()),
    );
    let llm_profile = resolve_llm_profile_with_preset(
        turn.profile.as_deref(),
        preset.and_then(|p| p.profile.as_deref()),
        cfg.ask_default_profile.as_deref(),
    );
    Ok(ResolvedTurnSettings {
        quiet: turn.quiet || preset.and_then(|p| p.quiet).unwrap_or(false),
        output_format: turn.format.map(Into::into),
        preset_name: turn.preset.clone(),
        log_tail_bytes,
        socket_path,
        session_id: resolve_turn_session_id(turn.session.as_deref())?,
        shell_log_choice,
        output_filter,
        llm_profile,
        ask_tools: preset
            .and_then(|p| p.tools.clone())
            .unwrap_or_else(|| cfg.ask_tools.clone()),
        tools_cli: turn.tools.clone(),
        no_start: turn.no_start,
        verbose_tools: turn.verbose_tools,
        progress: turn.progress,
        timeout_secs: turn.timeout,
        yes_exec: turn.yes_exec,
    })
}

fn resolve_llm_profile_with_preset(
    cli: Option<&str>,
    preset: Option<&str>,
    config_default: Option<&str>,
) -> Option<String> {
    if let Some(p) = cli.filter(|s| !s.is_empty()) {
        return Some(p.to_string());
    }
    if let Some(p) = preset.filter(|s| !s.is_empty()) {
        return Some(p.to_string());
    }
    if let Ok(env) = std::env::var("AI_LLM_PROFILE") {
        if !env.is_empty() {
            return Some(env);
        }
    }
    config_default.filter(|s| !s.is_empty()).map(str::to_string)
}

fn run_diagnostic_command(
    command: &str,
    quiet: bool,
    format: OutputFormat,
    socket_override: Option<PathBuf>,
    is_doctor: bool,
) -> anyhow::Result<()> {
    let cfg = AiConfig::load();
    let socket_path = socket_override.unwrap_or(cfg.socket_path.clone());
    let ping = ping_detailed(&socket_path);
    let (socket_alive, socket_error) = match ping {
        Ok(alive) => (alive, None),
        Err(e) => (false, Some(e.to_string())),
    };
    let shell_log = resolve_shell_log_info();
    let report = DiagnosticsReport {
        command: command.to_string(),
        config_socket_path: cfg.socket_path.display().to_string(),
        ask_default_profile: cfg.ask_default_profile.clone(),
        ask_filter: cfg.ask_filter.clone(),
        ask_tools: cfg.ask_tools.0.clone(),
        socket_path: socket_path.display().to_string(),
        socket_alive,
        socket_error,
        aish_session_dir: std::env::var("AISH_SESSION_DIR").ok(),
        implicit_session_id: implicit_session_id_from_env(),
        ai_ask_log: std::env::var("AI_ASK_LOG").ok(),
        shell_log_choice: shell_log.choice,
        shell_log_path: shell_log.path,
        shell_log_error: shell_log.error,
        preset: None,
        log_tail_bytes: resolve_log_tail_bytes(None, None, cfg.log_tail_bytes)?,
    };

    if !quiet {
        eprintln!(
            "ai: {}: socket {} ({})",
            command,
            if report.socket_alive {
                "alive"
            } else {
                "unreachable"
            },
            report.socket_path
        );
        if is_doctor {
            eprintln!("ai: doctor: config, session, and shell-log state are shown below");
        }
    }
    write_stdout(report.render(format))?;
    Ok(())
}

fn run_ping_command(
    quiet: bool,
    format: OutputFormat,
    socket_override: Option<PathBuf>,
) -> anyhow::Result<()> {
    let cfg = AiConfig::load();
    let socket_path = socket_override.unwrap_or(cfg.socket_path.clone());
    let ping = ping_detailed(&socket_path);
    let (socket_alive, socket_error) = match ping {
        Ok(alive) => (alive, None),
        Err(e) => (false, Some(e.to_string())),
    };
    let shell_log = resolve_shell_log_info();
    let report = DiagnosticsReport {
        command: "ping".to_string(),
        config_socket_path: cfg.socket_path.display().to_string(),
        ask_default_profile: cfg.ask_default_profile.clone(),
        ask_filter: cfg.ask_filter.clone(),
        ask_tools: cfg.ask_tools.0.clone(),
        socket_path: socket_path.display().to_string(),
        socket_alive,
        socket_error,
        aish_session_dir: std::env::var("AISH_SESSION_DIR").ok(),
        implicit_session_id: implicit_session_id_from_env(),
        ai_ask_log: std::env::var("AI_ASK_LOG").ok(),
        shell_log_choice: shell_log.choice,
        shell_log_path: shell_log.path,
        shell_log_error: shell_log.error,
        preset: None,
        log_tail_bytes: resolve_log_tail_bytes(None, None, cfg.log_tail_bytes)?,
    };

    if !quiet {
        eprintln!(
            "ai: ping: {} ({})",
            if report.socket_alive {
                "pong"
            } else {
                "unreachable"
            },
            report.socket_path
        );
    }
    write_stdout(report.render(format))?;

    if report.socket_alive {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            report
                .socket_error
                .unwrap_or_else(|| "aibe socket is unreachable".to_string())
        ))
    }
}

fn resolve_ask_message(
    file: Option<PathBuf>,
    message_parts: Vec<String>,
) -> anyhow::Result<ResolvedMessage> {
    if file.is_some() && !message_parts.is_empty() {
        anyhow::bail!("--file cannot be combined with message text");
    }

    if let Some(path) = file {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;
        return Ok(ResolvedMessage {
            source: format!("file:{}", path.display()),
            content,
        });
    }

    if message_parts.iter().any(|part| part == "-") {
        if message_parts.len() != 1 {
            anyhow::bail!("`-` must be used alone to read stdin");
        }
        return Ok(ResolvedMessage {
            source: "stdin".to_string(),
            content: read_all_stdin()?,
        });
    }

    if !message_parts.is_empty() {
        return Ok(ResolvedMessage {
            source: "argv".to_string(),
            content: message_parts.join(" "),
        });
    }

    if !std::io::stdin().is_terminal() {
        return Ok(ResolvedMessage {
            source: "pipe".to_string(),
            content: read_all_stdin()?,
        });
    }

    anyhow::bail!("missing message");
}

#[derive(Debug)]
struct ResolvedMessage {
    source: String,
    content: String,
}

fn read_all_stdin() -> anyhow::Result<String> {
    use std::io::Read;

    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| anyhow::anyhow!(e))?;
    Ok(buf)
}

fn validate_normalized_ask_args(args: &[std::ffi::OsString]) -> anyhow::Result<()> {
    let Some(first) = args.get(1) else {
        return Ok(());
    };
    if first != "ask" {
        return Ok(());
    }

    let tail: Vec<String> = args
        .iter()
        .skip(2)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    validate_ask_arg_order(&tail).map_err(|e| anyhow::anyhow!(e))
}

fn resolve_shell_log_info() -> ShellLogInfo {
    let ai_ask_log = std::env::var("AI_ASK_LOG").ok();
    let aish_session_dir = std::env::var("AISH_SESSION_DIR").ok();
    let session_dir = aish_session_dir.as_deref().map(Path::new);
    match resolve_shell_log_for_ask(false, None, None, ai_ask_log.as_deref(), session_dir) {
        Ok(choice) => ShellLogInfo {
            choice: render_shell_log_choice(&choice),
            path: render_shell_log_path(&choice),
            error: None,
        },
        Err(e) => ShellLogInfo {
            choice: "error".to_string(),
            path: None,
            error: Some(e.to_string()),
        },
    }
}

struct ShellLogInfo {
    choice: String,
    path: Option<String>,
    error: Option<String>,
}

fn implicit_session_id_from_env() -> Option<String> {
    let session_dir = std::env::var("AISH_SESSION_DIR").ok()?;
    Path::new(&session_dir)
        .file_name()
        .and_then(|name| name.to_str())
        .map(|s| s.to_string())
}

fn resolve_turn_session_id(session_cli: Option<&str>) -> anyhow::Result<Option<String>> {
    let Some(id) = session_cli.filter(|s| !s.is_empty()) else {
        return Ok(implicit_session_id_from_env());
    };

    let session_dir = std::env::var("AISH_SESSION_DIR")
        .map_err(|_| anyhow::anyhow!("--session requires AISH_SESSION_DIR to be set"))?;
    let expected = Path::new(&session_dir)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid AISH_SESSION_DIR: {session_dir}"))?;
    if expected != id {
        anyhow::bail!("--session {id} does not match AISH_SESSION_DIR ({session_dir})");
    }
    Ok(Some(id.to_string()))
}

fn validate_explicit_session(session_cli: Option<&str>) -> anyhow::Result<Option<String>> {
    let Some(id) = session_cli.filter(|s| !s.is_empty()) else {
        return Ok(None);
    };

    let session_dir = std::env::var("AISH_SESSION_DIR")
        .map_err(|_| anyhow::anyhow!("--session requires AISH_SESSION_DIR to be set"))?;
    let expected = Path::new(&session_dir)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid AISH_SESSION_DIR: {session_dir}"))?;
    if expected != id {
        anyhow::bail!("--session {id} does not match AISH_SESSION_DIR ({session_dir})");
    }
    Ok(Some(id.to_string()))
}

fn render_shell_log_choice(choice: &ShellLogChoice) -> String {
    match choice {
        ShellLogChoice::None => "none".to_string(),
        ShellLogChoice::Path(path) => format!("path:{}", path.display()),
    }
}

fn render_shell_log_path(choice: &ShellLogChoice) -> Option<String> {
    match choice {
        ShellLogChoice::None => None,
        ShellLogChoice::Path(path) => Some(path.display().to_string()),
    }
}

fn write_stdout(s: String) -> anyhow::Result<()> {
    use std::io::Write;

    std::io::stdout()
        .write_all(s.as_bytes())
        .map_err(|e| anyhow::anyhow!(e))
}

fn tools_resolve_to_anyhow(e: ToolsResolveError) -> anyhow::Error {
    anyhow::anyhow!(e)
}

fn shell_log_resolve_to_anyhow(e: ShellLogResolveError) -> anyhow::Error {
    anyhow::anyhow!(e)
}

fn history_store_to_anyhow(e: ai::ports::outbound::HistoryStoreError) -> anyhow::Error {
    anyhow::anyhow!(e)
}

#[cfg(test)]
mod cli_tests {
    use clap::CommandFactory;

    use ai::clap_cli::AiCli;

    #[test]
    fn ask_rejects_options_after_message() {
        let err = crate::validate_ask_arg_order(&["hello".into(), "--log".into(), "/tmp/x".into()])
            .unwrap_err();
        assert_eq!(err.to_string(), "options must appear before message");
    }

    #[test]
    fn cli_includes_complete_subcommand() {
        let cmd = AiCli::command();
        assert!(cmd.find_subcommand("complete").is_some());
    }
}
