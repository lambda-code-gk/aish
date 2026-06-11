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
    detect_terminal_size, external_command_names, load_shell_exec_approval, read_chat_line,
    resolve_shell_log_for_ask, AibeUnixClient, ChatReadLineResult, FileLogTail, LocalHistoryStore,
    StdoutPresenter, YesExecCache,
};
use ai::application::memory_cli::MemoryCliContext;
use ai::application::{
    build_response_summary, build_summary, ensure_aibe_if_needed, list_history, memory_cli,
    next_history_id, plan_ask_launch, record_turn, HistoryRecordInput, HistoryReplayInput,
    TurnCancelGuard,
};
use ai::clap_cli::{
    AiCli, AiCommand, GoalCommand, HistoryStatusArg, IdeaCommand, MemCommand, MemoryCliOptions,
    NowCommand, OutputFormatArg, TurnOptions,
};
use ai::domain::{
    resolve_console_hints, resolve_llm_profile, resolve_log_tail_bytes, resolve_output_filter,
    resolve_progress, validate_ask_arg_order, AskArgOrderError, AskInput, AskRequestError,
    ConfigToolsTokens, ConsoleHintReport, HistoryIndexFilter, HistoryMessage, HistoryPayload,
    HistoryRecordKind, HistoryRecordStatus, LogTailResolveError, OutputFormat, OutputFormatError,
    RequestContextInput, ShellLogChoice, ShellLogResolveError, ToolsResolveError,
};
use ai::domain::{DiagnosticsReport, DryRunReport, FilterMetadata};
use ai::ports::outbound::Presenter;
use ai::ports::outbound::{AgentError, MemoryClient};
use ai::ports::outbound::{HistoryStore, LogReadError, ShellLogSource};
use aibe_client::{ensure_running, ping_detailed, AgentTurnProgressEvent, ShellExecApprovalPrompt};
use aibe_protocol::{
    is_known_tool, ClientRequest, ClientResponse, ProtocolMessage, RouteKind, RoutePlan,
    RouteTurnCliOverrides, RouteTurnConversation, RouteTurnSession, GIT_DIFF, GIT_STATUS, GREP,
    LIST_DIR, READ_FILE, SHELL_EXEC,
};

fn main() -> ExitCode {
    if AiCli::try_complete_env() {
        return ExitCode::SUCCESS;
    }

    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("ai: {e}");
            exit_code_for_anyhow(&e)
        }
    }
}

fn run() -> anyhow::Result<ExitCode> {
    let normalized = AiCli::normalized_args_for_completion();
    validate_normalized_ask_args(&normalized)?;
    let cli = AiCli::parse_from(normalized);

    match cli.command {
        AiCommand::Complete { shell } => {
            AiCli::run_complete(shell).map_err(|e| anyhow::anyhow!(e))?;
            Ok(ExitCode::SUCCESS)
        }
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
        AiCommand::Goal { command } => run_goal(command),
        AiCommand::Now { command } => run_now(command),
        AiCommand::Idea { command } => run_idea(command),
        AiCommand::Mem { command } => run_mem(command),
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
    ai_session_id: String,
    shell_log_choice: ShellLogChoice,
    output_filter: Option<String>,
    output_filter_meta: FilterMetadata,
    llm_profile: Option<String>,
    ask_tools: ConfigToolsTokens,
    tools_cli: Option<String>,
    no_start: bool,
    verbose_tools: bool,
    progress: bool,
    progress_spinner: bool,
    timeout_secs: Option<u64>,
    yes_exec: bool,
    shell_exec_approval: Option<String>,
    console_hint: ConsoleHintReport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TurnCancelSource {
    Sigint,
    Timeout,
}

#[derive(Debug, Clone)]
struct TurnExecutionOutcome {
    response: ClientResponse,
    cancel_source: Option<TurnCancelSource>,
    streamed: bool,
}

fn run_ask(args: AskArgs) -> anyhow::Result<ExitCode> {
    let cfg = AiConfig::load();
    let message = resolve_ask_message(args.file.clone(), args.message)?;
    let base_settings = resolve_turn_settings(&cfg, &args.turn)?;
    if args.turn.dry_run {
        let report = build_dry_run_report(
            "ask",
            &message.source,
            message.content.len(),
            &cfg,
            &base_settings,
        );
        let format = base_settings.output_format.unwrap_or(OutputFormat::Tsv);
        write_stdout(report.render(format))?;
        return Ok(ExitCode::SUCCESS);
    }
    let smart = if std::io::stdin().is_terminal() {
        run_smart_route(
            &base_settings.socket_path,
            &message.content,
            &base_settings,
            &args.turn,
        )
    } else {
        SmartRouteOutcome::disabled()
    };
    let mut effective_turn = args.turn.clone();
    if let Some(ref plan) = smart.route_plan {
        effective_turn = apply_route_plan_advisory(effective_turn, plan, &cfg, base_settings.quiet);
    }
    if smart.route_fallback {
        effective_turn.tools = Some("none".to_string());
    }
    let settings = resolve_turn_settings(&cfg, &effective_turn)?;
    let conversation_id = smart.conversation_id;
    let route_plan_json = smart
        .route_plan
        .as_ref()
        .and_then(|p| serde_json::to_string(p).ok());
    let response = execute_turn(
        &cfg,
        "ask",
        message.clone(),
        settings,
        None,
        None,
        vec![ProtocolMessage {
            role: "user".to_string(),
            content: message.content,
        }],
        conversation_id,
        route_plan_json,
    )?;
    Ok(exit_code_for_response(
        &response.response,
        response.cancel_source,
    ))
}

fn run_chat(turn: TurnOptions) -> anyhow::Result<ExitCode> {
    let cfg = AiConfig::load();
    let settings = resolve_turn_settings(&cfg, &turn)?;
    if turn.dry_run {
        let report = build_dry_run_report("chat", "repl", 0, &cfg, &settings);
        let format = settings.output_format.unwrap_or(OutputFormat::Tsv);
        write_stdout(report.render(format))?;
        return Ok(ExitCode::SUCCESS);
    }
    let conversation_id = next_conversation_id();
    let mut transcript: Vec<ProtocolMessage> = Vec::new();
    loop {
        let content = match read_chat_line().map_err(|e| anyhow::anyhow!(e))? {
            ChatReadLineResult::Input(line) => line,
            ChatReadLineResult::Eof => break,
        };
        if content.is_empty() || content == "/exit" {
            if content == "/exit" {
                break;
            }
            continue;
        }
        let user_message = ResolvedMessage {
            source: "chat".to_string(),
            content: content.clone(),
        };
        let mut messages = transcript.clone();
        messages.push(ProtocolMessage {
            role: "user".to_string(),
            content: content.clone(),
        });
        let outcome = execute_turn(
            &cfg,
            "chat",
            user_message,
            settings.clone(),
            None,
            None,
            messages,
            Some(conversation_id.clone()),
            None,
        )?;
        let exit_code = exit_code_for_response(&outcome.response, outcome.cancel_source);
        match &outcome.response {
            ClientResponse::AgentTurnResult {
                assistant_message, ..
            } => {
                transcript.push(ProtocolMessage {
                    role: "user".to_string(),
                    content: content.clone(),
                });
                transcript.push(ProtocolMessage {
                    role: "assistant".to_string(),
                    content: assistant_message.content.clone(),
                });
            }
            ClientResponse::Error { .. } | ClientResponse::Cancelled { .. } => {
                return Ok(exit_code);
            }
            _ => {}
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn run_retry(turn: TurnOptions, history_id: String) -> anyhow::Result<ExitCode> {
    let cfg = AiConfig::load();
    let store = LocalHistoryStore::new(cfg.history_dir.clone());
    let payload = store
        .load_payload(&history_id)
        .map_err(history_store_to_anyhow)?;
    let settings = resolve_turn_settings(&cfg, &turn)?;
    let message = ResolvedMessage {
        source: format!("history:{history_id}"),
        content: payload.user_message.clone(),
    };
    let messages = replay_messages_from_payload(&payload);
    let response = execute_turn(
        &cfg,
        "retry",
        message.clone(),
        settings,
        None,
        None,
        messages,
        payload.conversation_id.clone(),
        payload.route_plan.clone(),
    )?;
    Ok(exit_code_for_response(
        &response.response,
        response.cancel_source,
    ))
}

fn run_rerun(turn: TurnOptions, history_id: String) -> anyhow::Result<ExitCode> {
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
    let messages = replay_messages_from_payload(&payload);
    let response = execute_turn(
        &cfg,
        "rerun",
        message.clone(),
        resolve_turn_settings(&cfg, &merged_turn)?,
        payload.shell_log_tail.clone(),
        payload.client_cwd.map(PathBuf::from),
        messages,
        payload.conversation_id.clone(),
        payload.route_plan.clone(),
    )?;
    Ok(exit_code_for_response(
        &response.response,
        response.cancel_source,
    ))
}

fn run_history(args: HistoryArgs) -> anyhow::Result<ExitCode> {
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
    Ok(ExitCode::SUCCESS)
}

#[allow(clippy::too_many_arguments)]
fn execute_turn(
    cfg: &AiConfig,
    command: &str,
    message: ResolvedMessage,
    settings: ResolvedTurnSettings,
    shell_log_override: Option<String>,
    client_cwd_override: Option<PathBuf>,
    messages: Vec<ProtocolMessage>,
    conversation_id: Option<String>,
    route_plan_json: Option<String>,
) -> anyhow::Result<TurnExecutionOutcome> {
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

    let progress_spinner = settings.progress_spinner;
    let presenter = Arc::new(StdoutPresenter::with_options(
        settings.output_filter.clone(),
        settings.output_format,
        settings.quiet,
        progress_spinner,
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
        ai_session_id: Some(settings.ai_session_id.clone()),
        conversation_id: conversation_id.clone(),
    };
    let mut request = ask_input.into_request()?;
    request.request_context = build_request_context(
        shell_log_tail_text.clone(),
        request.client_cwd.as_deref(),
        settings.ai_session_id.clone(),
        conversation_id.clone(),
        &settings.console_hint,
    );
    let turn_id = next_history_id();
    let request_messages = history_messages_from_protocol(&messages);
    let client_request = request_from_messages(turn_id.clone(), request, messages)?;

    let yes_exec_effective =
        settings.yes_exec && matches!(settings.shell_exec_approval.as_deref(), Some("ask"));
    let _progress_guard = presenter.progress_guard();
    let response = if settings.timeout_secs.is_some() || settings.progress || yes_exec_effective {
        run_agent_turn_async(
            plan.socket_path.clone(),
            client_request,
            presenter.clone(),
            cfg.history_dir.clone(),
            settings.session_id.clone(),
            yes_exec_effective,
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
            yes_exec_effective,
            settings.progress,
        )?
    };

    let TurnExecutionOutcome {
        response,
        cancel_source,
        streamed,
    } = response;
    let streamed = streamed || settings.progress || settings.timeout_secs.is_some();
    let response_error = match &response {
        ClientResponse::Error { message, .. } => Some(message.clone()),
        ClientResponse::Cancelled { reason, .. } => Some(
            reason
                .clone()
                .unwrap_or_else(|| "turn cancelled".to_string()),
        ),
        _ => None,
    };

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
    let route_plan_redacted = route_plan_json;
    let history_id = next_history_id();
    let record_input = HistoryRecordInput {
        command: command.to_string(),
        session_id: settings.session_id.clone(),
        conversation_id: conversation_id.clone(),
        ai_session_id: Some(settings.ai_session_id.clone()),
        preset: settings.preset_name.clone(),
        profile: settings.llm_profile.clone(),
        shell_exec_approval: settings.shell_exec_approval.clone(),
        route_plan: route_plan_redacted.clone(),
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
        conversation_id,
        ai_session_id: Some(settings.ai_session_id.clone()),
        shell_exec_approval: settings.shell_exec_approval.clone(),
        route_plan: route_plan_redacted,
        socket_path: settings.socket_path.display().to_string(),
        log_tail_bytes: settings.log_tail_bytes,
        request_messages,
    };
    let store = LocalHistoryStore::new(cfg.history_dir.clone());
    record_turn(
        &store,
        &record_input,
        &replay_input,
        cfg.history_max_entries,
    )
    .map_err(history_store_to_anyhow)?;
    presenter.show_response(&response, settings.verbose_tools, streamed);

    Ok(TurnExecutionOutcome {
        response,
        cancel_source,
        streamed,
    })
}

fn run_agent_turn_sync(
    socket_path: PathBuf,
    request: ClientRequest,
    presenter: Arc<StdoutPresenter>,
    history_dir: PathBuf,
    session_id: Option<String>,
    yes_exec: bool,
    progress: bool,
) -> anyhow::Result<TurnExecutionOutcome> {
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
) -> anyhow::Result<TurnExecutionOutcome> {
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
) -> anyhow::Result<TurnExecutionOutcome> {
    let turn_id = request_turn_id(&request)?;
    let worker_client = AibeUnixClient::new(socket_path.clone());
    let cancel_client = AibeUnixClient::new(socket_path);
    let cancel_guard = TurnCancelGuard::new().map_err(|e| anyhow::anyhow!("{e}"))?;
    let cancel_requested = Arc::clone(cancel_guard.flag());
    let streamed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut cancel_source: Option<TurnCancelSource> = None;

    let (tx, rx) = mpsc::channel();
    let presenter_thread = Arc::clone(&presenter);
    let history_dir_thread = history_dir.clone();
    let turn_id_thread = turn_id.clone();
    let streamed_thread = Arc::clone(&streamed);
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
                if !chunk.is_empty() {
                    if !presenter_thread.is_quiet() {
                        streamed_thread.store(true, Ordering::SeqCst);
                    }
                    presenter_thread.show_stream_chunk(&chunk);
                }
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
            if cancel_source.is_none() {
                cancel_source = Some(TurnCancelSource::Sigint);
            }
            let _ = cancel_client.cancel_turn(&turn_id);
        }
        if let Some(deadline) = timeout {
            if start.elapsed() >= deadline && !cancel_requested.load(Ordering::SeqCst) {
                cancel_requested.store(true, Ordering::SeqCst);
                cancel_source = Some(TurnCancelSource::Timeout);
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
                return Ok(TurnExecutionOutcome {
                    response: resp,
                    cancel_source,
                    streamed: streamed.load(Ordering::SeqCst),
                });
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
        | ClientRequest::RouteTurn { id, .. }
        | ClientRequest::ShellExecApproval { id, .. }
        | ClientRequest::CancelTurn { id, .. } => Ok(id.clone()),
        ClientRequest::MemoryApply(body) => Ok(body.id.clone()),
        ClientRequest::MemoryQuery(body) => Ok(body.id.clone()),
    }
}

fn cli_console_hint_explicit(turn: &TurnOptions) -> Option<bool> {
    if turn.no_console_hint {
        Some(false)
    } else if turn.console_hint {
        Some(true)
    } else {
        None
    }
}

fn cli_progress_explicit(turn: &TurnOptions) -> Option<bool> {
    if turn.no_progress {
        Some(false)
    } else if turn.progress {
        Some(true)
    } else {
        None
    }
}

fn build_request_context(
    shell_log_tail: Option<String>,
    client_cwd: Option<&Path>,
    ai_session_id: String,
    conversation_id: Option<String>,
    console_hint: &ConsoleHintReport,
) -> RequestContextInput {
    let terminal_size = if console_hint.effective {
        detect_terminal_size()
    } else {
        None
    };
    RequestContextInput {
        shell_log_tail,
        cwd: client_cwd.map(|p| p.display().to_string()),
        ai_session_id: Some(ai_session_id),
        conversation_id,
        ..Default::default()
    }
    .with_console_system_instruction(terminal_size, console_hint.effective)
}

fn request_from_messages(
    turn_id: String,
    request: ai::domain::AskRequest,
    messages: Vec<ProtocolMessage>,
) -> anyhow::Result<ClientRequest> {
    let context = request.request_context.into_wire();
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

fn exit_code_for_response(
    response: &ClientResponse,
    cancel_source: Option<TurnCancelSource>,
) -> ExitCode {
    match response {
        ClientResponse::AgentTurnResult { .. } => ExitCode::SUCCESS,
        ClientResponse::Cancelled { .. } => match cancel_source {
            Some(TurnCancelSource::Sigint) => ExitCode::from(130),
            _ => ExitCode::from(3),
        },
        ClientResponse::Error { code, .. } => match code {
            aibe_protocol::ErrorCode::InvalidRequest => ExitCode::from(2),
            aibe_protocol::ErrorCode::ProviderError => ExitCode::from(4),
            aibe_protocol::ErrorCode::ToolError
            | aibe_protocol::ErrorCode::ToolTimeout
            | aibe_protocol::ErrorCode::ToolNotAllowed => ExitCode::from(5),
            aibe_protocol::ErrorCode::MaxToolRounds => ExitCode::SUCCESS,
            aibe_protocol::ErrorCode::InternalError => ExitCode::from(3),
        },
        ClientResponse::Pong { .. }
        | ClientResponse::Progress { .. }
        | ClientResponse::AssistantStreaming { .. }
        | ClientResponse::RouteTurnResult { .. }
        | ClientResponse::ShellExecApprovalPrompt { .. }
        | ClientResponse::MemoryApplyResult { .. }
        | ClientResponse::MemoryQueryResult { .. } => ExitCode::SUCCESS,
    }
}

fn exit_code_for_anyhow(err: &anyhow::Error) -> ExitCode {
    if err.downcast_ref::<AskRequestError>().is_some()
        || err.downcast_ref::<AskArgOrderError>().is_some()
        || err.downcast_ref::<ToolsResolveError>().is_some()
        || err.downcast_ref::<ShellLogResolveError>().is_some()
        || err.downcast_ref::<LogTailResolveError>().is_some()
        || err.downcast_ref::<OutputFormatError>().is_some()
    {
        return ExitCode::from(2);
    }

    let s = err.to_string();
    if s.contains("unknown preset")
        || s.contains("missing message")
        || s.contains("cannot be combined")
        || s.contains("invalid message role")
        || s.contains("client cwd is unavailable")
        || s.contains("unknown tool")
    {
        return ExitCode::from(2);
    }
    if s.contains("connect to aibe")
        || s.contains("deserialize response")
        || s.contains("invalid JSON")
        || s.contains("agent turn worker disconnected")
        || s.contains("shell_log")
        || s.contains("log tail")
        || s.contains("timeout")
    {
        return ExitCode::from(3);
    }
    ExitCode::FAILURE
}

fn next_conversation_id() -> String {
    format!("conv-{}", next_history_id())
}

#[derive(Debug, Clone)]
struct SmartRouteOutcome {
    conversation_id: Option<String>,
    route_plan: Option<RoutePlan>,
    route_fallback: bool,
}

impl SmartRouteOutcome {
    fn disabled() -> Self {
        Self {
            conversation_id: None,
            route_plan: None,
            route_fallback: false,
        }
    }
}

fn run_smart_route(
    socket_path: &Path,
    query: &str,
    settings: &ResolvedTurnSettings,
    turn: &TurnOptions,
) -> SmartRouteOutcome {
    let request = build_route_turn_request(query, settings, turn);
    let client = AibeUnixClient::new(socket_path);
    let plan = match try_route_turn(&client, request.clone()) {
        Ok(plan) => Some(plan),
        Err(first) => match try_route_turn(&client, request) {
            Ok(plan) => Some(plan),
            Err(second) => {
                if !settings.quiet {
                    eprintln!("ai: route_turn failed; falling back to text-only one-shot");
                    eprintln!("ai: route_turn error: {second}");
                    if first != second {
                        eprintln!("ai: route_turn first attempt: {first}");
                    }
                }
                None
            }
        },
    };
    if let Some(ref plan) = plan {
        maybe_log_route_escalation(settings.quiet, plan);
        SmartRouteOutcome {
            conversation_id: Some(plan.conversation_id.clone()),
            route_plan: Some(plan.clone()),
            route_fallback: false,
        }
    } else {
        SmartRouteOutcome {
            conversation_id: None,
            route_plan: None,
            route_fallback: true,
        }
    }
}

fn try_route_turn(client: &AibeUnixClient, request: ClientRequest) -> Result<RoutePlan, String> {
    match client.route_turn(request) {
        Ok(ClientResponse::RouteTurnResult { plan, .. }) => Ok(plan),
        Ok(other) => Err(format!("unexpected response: {other:?}")),
        Err(e) => Err(e.to_string()),
    }
}

fn build_route_turn_request(
    query: &str,
    settings: &ResolvedTurnSettings,
    turn: &TurnOptions,
) -> ClientRequest {
    let cwd = std::env::current_dir()
        .ok()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "/".to_string());
    ClientRequest::RouteTurn {
        id: next_history_id(),
        query: query.to_string(),
        cwd,
        session: RouteTurnSession {
            ai_session_id: settings.ai_session_id.clone(),
            aish_session_dir: std::env::var("AISH_SESSION_DIR").ok(),
            tty: true,
        },
        conversation: RouteTurnConversation {
            conversation_id: None,
            recent_summary: None,
            new_conversation: turn.new,
        },
        cli_overrides: RouteTurnCliOverrides {
            preset: turn.preset.clone(),
            tools: turn.tools.as_ref().map(|s| {
                s.split(',')
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                    .map(str::to_string)
                    .collect()
            }),
            log_tail_bytes: turn.log_tail.map(|n| n as u64),
            yes_exec: turn.yes_exec,
        },
    }
}

fn apply_route_plan_advisory(
    mut turn: TurnOptions,
    plan: &RoutePlan,
    cfg: &AiConfig,
    quiet: bool,
) -> TurnOptions {
    if turn.preset.is_none() {
        if let Some(name) = plan.recommended_preset.as_deref().filter(|s| !s.is_empty()) {
            if cfg.presets.contains_key(name) {
                turn.preset = Some(name.to_string());
            } else if !quiet {
                eprintln!("ai: smart entry: ignored unknown preset: {name}");
            }
        }
    }
    if turn.tools.is_none() {
        match sanitize_recommended_tools(plan.recommended_tools.as_deref()) {
            Some(tools) => turn.tools = Some(tools.join(",")),
            None if plan
                .recommended_tools
                .as_ref()
                .is_some_and(|t| !t.is_empty())
                && !quiet =>
            {
                eprintln!("ai: smart entry: ignored unknown suggested tools");
            }
            None => {}
        }
    }
    if turn.log_tail.is_none() {
        if let Some(bytes) = plan.log_tail_bytes {
            turn.log_tail = Some(bytes as usize);
        }
    }
    turn
}

fn sanitize_recommended_tools(raw: Option<&[String]>) -> Option<Vec<String>> {
    let raw = raw.filter(|tools| !tools.is_empty())?;
    let mut out = Vec::new();
    for name in raw {
        let mapped = map_route_tool_alias(name);
        if is_known_tool(&mapped) && !out.iter().any(|existing| existing == &mapped) {
            out.push(mapped);
        }
    }
    (!out.is_empty()).then_some(out)
}

fn map_route_tool_alias(raw: &str) -> String {
    let norm = raw.trim().to_ascii_lowercase().replace('-', "_");
    match norm.as_str() {
        "view_file" | "viewfile" | "read" | "cat" | "cat_file" => READ_FILE.to_string(),
        "list_files" | "listdir" | "ls" | "dir" => LIST_DIR.to_string(),
        "search" | "find" | "rg" => GREP.to_string(),
        "git" | "status" => GIT_STATUS.to_string(),
        "diff" => GIT_DIFF.to_string(),
        "shell" | "exec" | "run" => SHELL_EXEC.to_string(),
        other => other.to_string(),
    }
}

fn maybe_log_route_escalation(quiet: bool, plan: &RoutePlan) {
    if quiet {
        return;
    }
    if plan.new_conversation {
        eprintln!(
            "ai: smart entry: new conversation ({})",
            plan.conversation_id
        );
    } else if matches!(plan.route_kind, RouteKind::Continue | RouteKind::Chat) {
        eprintln!(
            "ai: smart entry: continuing conversation ({})",
            plan.conversation_id
        );
    }
    if plan.log_tail_escalation {
        eprintln!("ai: smart entry: log tail escalation suggested");
    }
    if plan.require_shell_approval {
        eprintln!("ai: smart entry: shell approval may be required");
    }
    if let Some(tools) = sanitize_recommended_tools(plan.recommended_tools.as_deref()) {
        eprintln!("ai: smart entry: tools suggested: {}", tools.join(","));
    }
}

fn resolve_ai_session_id() -> String {
    use std::sync::OnceLock;

    static SESSION_ID: OnceLock<String> = OnceLock::new();
    SESSION_ID
        .get_or_init(|| {
            if let Ok(id) = std::env::var("AI_SESSION_ID") {
                if !id.is_empty() {
                    return id;
                }
            }
            format!(
                "ai-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            )
        })
        .clone()
}

fn build_dry_run_report(
    command: &str,
    message_source: &str,
    message_length: usize,
    cfg: &AiConfig,
    settings: &ResolvedTurnSettings,
) -> DryRunReport {
    DryRunReport {
        command: command.to_string(),
        message_source: message_source.to_string(),
        message_length,
        message_masked: "<masked>".to_string(),
        config_socket_path: cfg.socket_path.display().to_string(),
        ask_default_profile: cfg.ask_default_profile.clone(),
        ask_filter: settings.output_filter_meta.clone(),
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
        console_hint: settings.console_hint.clone(),
    }
}

fn history_messages_from_protocol(messages: &[ProtocolMessage]) -> Vec<HistoryMessage> {
    messages
        .iter()
        .map(|message| HistoryMessage {
            role: message.role.clone(),
            content: message.content.clone(),
        })
        .collect()
}

fn protocol_messages_from_history(messages: &[HistoryMessage]) -> Vec<ProtocolMessage> {
    messages
        .iter()
        .map(|message| ProtocolMessage {
            role: message.role.clone(),
            content: message.content.clone(),
        })
        .collect()
}

fn replay_messages_from_payload(payload: &HistoryPayload) -> Vec<ProtocolMessage> {
    if !payload.request_messages.is_empty() {
        return protocol_messages_from_history(&payload.request_messages);
    }
    vec![ProtocolMessage {
        role: "user".to_string(),
        content: payload.user_message.clone(),
    }]
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
    let output_filter_meta = resolve_filter_metadata(
        std::env::var("AI_FILTER").ok(),
        preset.and_then(|p| p.filter.as_deref()),
        cfg.ask_filter.as_deref(),
    );
    let llm_profile = resolve_llm_profile_with_preset(
        turn.profile.as_deref(),
        preset.and_then(|p| p.profile.as_deref()),
        cfg.ask_default_profile.as_deref(),
    );
    let aibe_shell_exec = load_shell_exec_approval();
    let shell_exec_approval = preset
        .and_then(|p| p.shell_exec_approval.clone())
        .filter(|s| !s.is_empty())
        .or(aibe_shell_exec.mode.clone());
    let output_format = turn.format.map(Into::into);
    let console_hint = resolve_console_hints(
        cli_console_hint_explicit(turn),
        preset.and_then(|p| p.console_hints),
        cfg.ask_console_hints,
        detect_terminal_size().is_some(),
        output_format,
    );
    let stderr_tty = std::io::stderr().is_terminal();
    let progress = resolve_progress(
        cli_progress_explicit(turn),
        preset.and_then(|p| p.progress),
        cfg.ask_progress,
        stderr_tty,
    );
    let quiet = turn.quiet || preset.and_then(|p| p.quiet).unwrap_or(false);
    let progress_spinner = progress && !quiet && stderr_tty;
    Ok(ResolvedTurnSettings {
        quiet,
        output_format,
        preset_name: turn.preset.clone(),
        log_tail_bytes,
        socket_path,
        session_id: resolve_turn_session_id(turn.session.as_deref())?,
        ai_session_id: resolve_ai_session_id(),
        shell_log_choice,
        output_filter,
        output_filter_meta,
        llm_profile,
        ask_tools: preset
            .and_then(|p| p.tools.clone())
            .unwrap_or_else(|| cfg.ask_tools.clone()),
        tools_cli: turn.tools.clone(),
        no_start: turn.no_start,
        verbose_tools: turn.verbose_tools,
        progress,
        progress_spinner,
        timeout_secs: turn.timeout,
        yes_exec: turn.yes_exec,
        shell_exec_approval,
        console_hint,
    })
}

fn resolve_filter_metadata(
    env: Option<String>,
    preset: Option<&str>,
    config: Option<&str>,
) -> FilterMetadata {
    let (enabled, source) = if env.as_ref().is_some_and(|s| !s.is_empty()) {
        (true, "env".to_string())
    } else if preset.is_some_and(|s| !s.is_empty()) {
        (true, "preset".to_string())
    } else if config.is_some_and(|s| !s.is_empty()) {
        (true, "config".to_string())
    } else {
        (false, "none".to_string())
    };
    FilterMetadata {
        enabled,
        source,
        masked: true,
    }
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
    let env_profile = std::env::var("AI_LLM_PROFILE").ok();
    resolve_llm_profile(None, env_profile.as_deref(), config_default)
}

fn join_words(parts: Vec<String>) -> String {
    parts.join(" ")
}

fn prepare_memory_context(options: &MemoryCliOptions) -> anyhow::Result<MemoryCliContext> {
    let cfg = AiConfig::load();
    let socket_path = options.socket.clone().unwrap_or(cfg.socket_path);
    if !options.no_start {
        ensure_running(&socket_path).map_err(|e| anyhow::anyhow!(e))?;
    }
    let cwd = std::env::current_dir()?;
    Ok(MemoryCliContext {
        socket_path,
        session_id: resolve_ai_session_id(),
        cwd,
        format: options.format.into(),
    })
}

fn run_memory_command(
    options: MemoryCliOptions,
    action: impl FnOnce(&dyn MemoryClient, &MemoryCliContext) -> Result<String, AgentError>,
) -> anyhow::Result<ExitCode> {
    let ctx = prepare_memory_context(&options)?;
    let client = AibeUnixClient::new(&ctx.socket_path);
    let out = action(&client, &ctx).map_err(|e| anyhow::anyhow!(e))?;
    println!("{out}");
    Ok(ExitCode::SUCCESS)
}

fn run_goal(command: GoalCommand) -> anyhow::Result<ExitCode> {
    match command {
        GoalCommand::Set { text, options } => run_memory_command(options, |client, ctx| {
            memory_cli::run_goal_set(client, ctx, &join_words(text))
        }),
        GoalCommand::Show { options } => run_memory_command(options, memory_cli::run_goal_show),
        GoalCommand::Clear { options } => run_memory_command(options, memory_cli::run_goal_clear),
    }
}

fn run_now(command: NowCommand) -> anyhow::Result<ExitCode> {
    match command {
        NowCommand::Set { text, options } => run_memory_command(options, |client, ctx| {
            memory_cli::run_now_set(client, ctx, &join_words(text))
        }),
        NowCommand::Show { options } => run_memory_command(options, memory_cli::run_now_show),
        NowCommand::Clear { options } => run_memory_command(options, memory_cli::run_now_clear),
    }
}

fn run_idea(command: IdeaCommand) -> anyhow::Result<ExitCode> {
    match command {
        IdeaCommand::Add { text, options } => run_memory_command(options, |client, ctx| {
            memory_cli::run_idea_add(client, ctx, &join_words(text))
        }),
        IdeaCommand::List { options } => run_memory_command(options, memory_cli::run_idea_list),
        IdeaCommand::Clear { options } => run_memory_command(options, memory_cli::run_idea_clear),
    }
}

fn run_mem(command: MemCommand) -> anyhow::Result<ExitCode> {
    match command {
        MemCommand::Add {
            kind,
            text,
            options,
        } => run_memory_command(options, |client, ctx| {
            memory_cli::run_mem_add(client, ctx, &kind, &join_words(text))
        }),
        MemCommand::List { kind, options } => run_memory_command(options, |client, ctx| {
            memory_cli::run_mem_list(client, ctx, kind.as_deref())
        }),
        MemCommand::Show { query, options } => run_memory_command(options, |client, ctx| {
            memory_cli::run_mem_show(client, ctx, query.as_deref())
        }),
        MemCommand::Clear { kind, options } => run_memory_command(options, |client, ctx| {
            memory_cli::run_mem_clear(client, ctx, &kind)
        }),
    }
}

fn run_diagnostic_command(
    command: &str,
    quiet: bool,
    format: OutputFormat,
    socket_override: Option<PathBuf>,
    is_doctor: bool,
) -> anyhow::Result<ExitCode> {
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
        ask_filter: resolve_filter_metadata(
            std::env::var("AI_FILTER").ok(),
            None,
            cfg.ask_filter.as_deref(),
        ),
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
    Ok(ExitCode::SUCCESS)
}

fn run_ping_command(
    quiet: bool,
    format: OutputFormat,
    socket_override: Option<PathBuf>,
) -> anyhow::Result<ExitCode> {
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
        ask_filter: resolve_filter_metadata(
            std::env::var("AI_FILTER").ok(),
            None,
            cfg.ask_filter.as_deref(),
        ),
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
        Ok(ExitCode::SUCCESS)
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

#[derive(Debug, Clone)]
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
    use std::path::PathBuf;

    use ai::adapters::outbound::toml_config::{AiConfig, AiPresetConfig};
    use ai::clap_cli::{AiCli, TurnOptions};
    use ai::domain::{
        resolve_console_hints, resolve_progress, ConfigToolsTokens, FilterMetadata, ShellLogChoice,
    };
    use ai::domain::{HistoryMessage, HistoryPayload};
    use aibe_client::default_socket_path;
    use aibe_protocol::{ClientRequest, ClientResponse, ErrorCode, RouteKind, RoutePlan};

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

    #[test]
    fn exit_codes_map_to_response_shape() {
        assert_eq!(
            crate::exit_code_for_response(
                &ClientResponse::Cancelled {
                    id: "id".into(),
                    turn_id: "id".into(),
                    reason: Some("cancelled".into()),
                },
                Some(crate::TurnCancelSource::Sigint),
            ),
            std::process::ExitCode::from(130)
        );
        assert_eq!(
            crate::exit_code_for_response(
                &ClientResponse::Error {
                    id: "id".into(),
                    code: ErrorCode::InvalidRequest,
                    message: "bad".into(),
                },
                None,
            ),
            std::process::ExitCode::from(2)
        );
        assert_eq!(
            crate::exit_code_for_anyhow(&anyhow::anyhow!("unknown preset: fast")),
            std::process::ExitCode::from(2)
        );
        assert_eq!(
            crate::exit_code_for_response(
                &ClientResponse::Error {
                    id: "id".into(),
                    code: ErrorCode::ProviderError,
                    message: "bad".into(),
                },
                None,
            ),
            std::process::ExitCode::from(4)
        );
        assert_eq!(
            crate::exit_code_for_response(
                &ClientResponse::Error {
                    id: "id".into(),
                    code: ErrorCode::ToolError,
                    message: "bad".into(),
                },
                None,
            ),
            std::process::ExitCode::from(5)
        );
    }

    #[test]
    fn replay_messages_prefers_saved_transcript() {
        let payload = HistoryPayload {
            history_id: "id".into(),
            command: "chat".into(),
            user_message: "turn2".into(),
            request_messages: vec![
                HistoryMessage {
                    role: "user".into(),
                    content: "turn1".into(),
                },
                HistoryMessage {
                    role: "assistant".into(),
                    content: "reply1".into(),
                },
                HistoryMessage {
                    role: "user".into(),
                    content: "turn2".into(),
                },
            ],
            shell_log_tail: None,
            client_cwd: None,
            tools: vec![],
            llm_profile: None,
            preset: None,
            session_id: None,
            ai_session_id: None,
            conversation_id: None,
            shell_exec_approval: None,
            route_plan: None,
            socket_path: "/tmp/s".into(),
            log_tail_bytes: 1,
        };
        let messages = crate::replay_messages_from_payload(&payload);
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].content, "turn1");
        assert_eq!(messages[2].content, "turn2");
    }

    #[test]
    fn replay_messages_falls_back_to_user_message() {
        let payload = HistoryPayload {
            history_id: "id".into(),
            command: "ask".into(),
            user_message: "hello".into(),
            request_messages: vec![],
            shell_log_tail: None,
            client_cwd: None,
            tools: vec![],
            llm_profile: None,
            preset: None,
            session_id: None,
            ai_session_id: None,
            conversation_id: None,
            shell_exec_approval: None,
            route_plan: None,
            socket_path: "/tmp/s".into(),
            log_tail_bytes: 1,
        };
        let messages = crate::replay_messages_from_payload(&payload);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "hello");
    }

    #[test]
    fn build_route_turn_request_sets_new_conversation_and_session_id() {
        let settings = crate::ResolvedTurnSettings {
            quiet: false,
            output_format: None,
            preset_name: Some("fast".into()),
            log_tail_bytes: 64,
            socket_path: PathBuf::from("/tmp/aibe.sock"),
            session_id: None,
            ai_session_id: "session-123".into(),
            shell_log_choice: ShellLogChoice::None,
            output_filter: None,
            output_filter_meta: FilterMetadata {
                enabled: false,
                source: "none".into(),
                masked: false,
            },
            llm_profile: None,
            ask_tools: ConfigToolsTokens::default(),
            tools_cli: None,
            no_start: false,
            verbose_tools: false,
            progress: false,
            progress_spinner: false,
            timeout_secs: None,
            yes_exec: true,
            shell_exec_approval: None,
            console_hint: resolve_console_hints(None, None, None, true, None),
        };
        let turn = crate::TurnOptions {
            new: true,
            preset: Some("fast".into()),
            tools: Some("read_file,shell_exec".into()),
            log_tail: Some(128),
            yes_exec: true,
            ..Default::default()
        };

        let request = crate::build_route_turn_request("hello", &settings, &turn);
        let ClientRequest::RouteTurn {
            query,
            session,
            conversation,
            cli_overrides,
            ..
        } = request
        else {
            panic!("expected route_turn");
        };
        assert_eq!(query, "hello");
        assert_eq!(session.ai_session_id, "session-123");
        assert!(session.tty);
        assert!(conversation.new_conversation);
        assert_eq!(cli_overrides.preset.as_deref(), Some("fast"));
        assert_eq!(cli_overrides.log_tail_bytes, Some(128));
        assert!(cli_overrides.yes_exec);
        assert_eq!(
            cli_overrides.tools.as_ref().map(Vec::as_slice),
            Some(&["read_file".to_string(), "shell_exec".to_string()][..])
        );
    }

    #[test]
    fn turn_options_parse_progress_flags() {
        use clap::Parser;

        #[derive(Parser)]
        #[command(name = "test")]
        struct Cli {
            #[command(flatten)]
            turn: TurnOptions,
        }

        let turn = Cli::try_parse_from(["test", "--progress"])
            .expect("parse")
            .turn;
        assert!(turn.progress);
        assert!(!turn.no_progress);

        let turn = Cli::try_parse_from(["test", "--no-progress"])
            .expect("parse")
            .turn;
        assert!(turn.no_progress);
        assert!(!turn.progress);

        assert!(Cli::try_parse_from(["test", "--progress", "--no-progress"]).is_err());
    }

    #[test]
    fn resolve_progress_tty_default() {
        assert!(resolve_progress(None, None, None, true));
        assert!(!resolve_progress(None, None, None, false));
        assert!(!resolve_progress(Some(false), None, None, true));
    }

    #[test]
    fn resolve_turn_settings_progress_spinner_not_blocked_by_format() {
        use std::collections::HashMap;
        use std::io::IsTerminal;

        use crate::{resolve_turn_settings, OutputFormat};
        use ai::clap_cli::OutputFormatArg;

        let cfg = AiConfig {
            socket_path: default_socket_path(),
            ask_tools: ConfigToolsTokens::default(),
            ask_default_profile: None,
            ask_filter: None,
            ask_console_hints: None,
            ask_progress: None,
            history_dir: PathBuf::from("/tmp/ai-history-test"),
            history_max_entries: 500,
            log_tail_bytes: None,
            presets: HashMap::new(),
        };
        let turn = TurnOptions {
            format: Some(OutputFormatArg::Json),
            progress: true,
            ..Default::default()
        };
        let settings = resolve_turn_settings(&cfg, &turn).expect("resolve");
        assert_eq!(settings.output_format, Some(OutputFormat::Json));
        assert!(settings.progress);
        if std::io::stderr().is_terminal() {
            assert!(settings.progress_spinner);
        }
    }

    #[test]
    fn turn_options_parse_console_hint_flags() {
        use clap::Parser;

        #[derive(Parser)]
        #[command(name = "test")]
        struct Cli {
            #[command(flatten)]
            turn: TurnOptions,
        }

        let cli = Cli::try_parse_from(["test", "--console-hint"]).expect("parse");
        let turn = cli.turn;
        assert!(turn.console_hint);
        assert!(!turn.no_console_hint);

        let turn = Cli::try_parse_from(["test", "-H"]).expect("parse").turn;
        assert!(turn.console_hint);

        let turn = Cli::try_parse_from(["test", "--no-console-hint"])
            .expect("parse")
            .turn;
        assert!(turn.no_console_hint);

        let turn = Cli::try_parse_from(["test", "-N"]).expect("parse").turn;
        assert!(turn.no_console_hint);

        assert!(Cli::try_parse_from(["test", "-H", "--no-console-hint"]).is_err());
        assert!(Cli::try_parse_from(["test", "-H", "-N"]).is_err());
    }

    #[test]
    fn apply_route_plan_ignores_unknown_preset_and_maps_tools() {
        use std::collections::HashMap;

        let cfg = AiConfig {
            socket_path: default_socket_path(),
            ask_tools: ConfigToolsTokens::default(),
            ask_default_profile: None,
            ask_filter: None,
            ask_console_hints: None,
            ask_progress: None,
            history_dir: PathBuf::from("/tmp/ai-history-test"),
            history_max_entries: 500,
            log_tail_bytes: None,
            presets: HashMap::from([("fast".into(), AiPresetConfig::default())]),
        };
        let plan = RoutePlan {
            conversation_id: "conv-1".into(),
            new_conversation: false,
            route_kind: RouteKind::ToolAssisted,
            recommended_preset: Some("files".into()),
            recommended_tools: Some(vec!["view_file".into()]),
            log_tail_bytes: None,
            require_shell_approval: false,
            log_tail_escalation: false,
            route_reason: "read readme".into(),
            confidence: None,
        };
        let turn = crate::apply_route_plan_advisory(TurnOptions::default(), &plan, &cfg, true);
        assert!(turn.preset.is_none());
        assert_eq!(turn.tools.as_deref(), Some("read_file"));
    }
}
