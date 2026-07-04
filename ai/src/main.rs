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

use ai::adapters::inbound::reedline_prompt::acquire_prompt_via_reedline;
use ai::adapters::outbound::toml_config::{AiConfig, SmartPreprocessorConfig};
use ai::adapters::outbound::{
    acquire_prompt_via_external_editor, create_prompt_temp_file, detect_terminal_size,
    external_command_names, finalize_preprocessor_observation, load_bundled_preprocessor_model,
    load_preprocessor_model, load_replay_events, load_shell_exec_approval, read_chat_line,
    resolve_editor_command_from_env, resolve_session_error_summary, resolve_shell_log_for_ask,
    resolve_suggestion_cache_path, smart_preprocessor_trace_enabled, AibeUnixClient,
    AishHumanShellLauncher, ChatReadLineResult, FileHandoffCandidatePublisher, FileLogTail,
    FileSuggestedCommandRecallStore, FilesystemHandoffStore, LocalHistoryStore, LocalRouteMetrics,
    PreprocessorObservationDraft, ProcessEnvironmentObserver, ShellExecRenderOptions,
    StdoutPresenter, SystemHandoffRuntime, YesExecCache,
};
use ai::application::memory_cli_context::MemoryCliContext;
use ai::application::memory_cli_pack::{load_command_policy, MemoryCliPack};
use ai::application::memory_space::{format_resolution, resolve_memory_space_id};
use ai::application::{
    assistant_content_from_response, build_response_summary, build_summary, classify_from_raw_args,
    current_time_ms, ensure_aibe_if_needed, evaluate_preprocessor, execute_feature_actions_mvp,
    list_history, memory_cli, next_history_id, parse_request_human_action,
    persist_suggested_commands, plan_ask_launch, plan_interactive_prompt_route,
    recall_next_command, recall_prev_command, record_turn, resolve_recall_gating,
    CollaborativeExecutionContext, CollaborativeShellEnvironment, CollaborativeShellExecPolicy,
    HistoryRecordInput, HistoryReplayInput, InteractivePromptRoute, ParentShellExecRequest,
    PreprocessorRunInput, PreprocessorRunOutcome, ReadCollaborativeStatus, RecallGatingInput,
    RecallTurnContext, ShellLogMode, SideAgentDispatch, SideAgentInvocation, SideTurn,
    StartOrResumeSideAgent, TurnCancelGuard, HANDOFF_ENV_KEYS,
};
use ai::clap_cli::{
    AiCli, AiCommand, ContextCommand, GoalCommand, HistoryStatusArg, IdeaCommand, MemCommand,
    MemoryCliOptions, NowCommand, OutputFormatArg, RecallCommand, SmartCommand, TurnOptions,
    WorkCliOptions, WorkCommand,
};
use ai::domain::client_tools::replay_show::replay_client_tool_callback;
use ai::domain::smart_preprocessor::{
    build_local_route_context_summary, local_output_style_system_hint, LocalRouteDecision,
    LocalToolHint, PreprocessConfig, RouteMetadataInput, SmartIntentClass, SmartPreprocessMode,
};
use ai::domain::{
    resolve_console_hints, resolve_llm_profile, resolve_log_tail_bytes, resolve_output_filter,
    resolve_progress, resolve_tools, validate_ask_arg_order, AskArgOrderError, AskInput,
    AskInvocationSource, AskRequestError, ConfigToolsTokens, ConsoleHintReport, HistoryIndexFilter,
    HistoryMessage, HistoryPayload, HistoryRecordKind, HistoryRecordStatus, LogTailResolveError,
    OutputFormat, OutputFormatError, PromptAcquisitionResult, RequestContextInput,
    ShellExecSessionState, ShellExecTier, ShellLogChoice, ShellLogResolveError, ToolsResolveError,
};
use ai::domain::{DiagnosticsReport, DryRunReport, FilterMetadata};
use ai::ports::outbound::Presenter;
use ai::ports::outbound::{AgentError, MemoryClient, NoopParentToolBarrier};
use ai::ports::outbound::{HistoryStore, LogReadError, ShellLogSource};
use aibe_client::{
    ensure_running, ping_detailed, AgentTurnProgressEvent, ShellExecApprovalDecision,
    ShellExecApprovalPrompt,
};
use aibe_protocol::{
    ClientRequest, ClientResponse, ProtocolMessage, RouteKind, RoutePlan, RouteTurnCliOverrides,
    RouteTurnConversation, RouteTurnPreprocessorHints, RouteTurnSession, WorkEntryKindDto,
    WorkOperationDto,
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
    let raw_args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let ask_invocation = classify_from_raw_args(&raw_args);
    let normalized = AiCli::normalized_args_for_completion();
    validate_normalized_ask_args(&normalized)?;
    let cli = AiCli::parse_from(normalized);
    let collaborative = cli.collaborative;
    let standalone = cli.standalone;
    if standalone {
        for key in HANDOFF_ENV_KEYS {
            std::env::remove_var(key);
        }
    }

    match cli.command {
        AiCommand::Complete { shell } => {
            AiCli::run_complete(shell).map_err(|e| anyhow::anyhow!(e))?;
            Ok(ExitCode::SUCCESS)
        }
        AiCommand::Ask {
            turn,
            file,
            message,
        } => {
            let side = if standalone {
                None
            } else {
                resolve_side_dispatch(
                    collaborative,
                    matches!(ask_invocation, AskInvocationSource::BareRoot),
                    message.join(" "),
                )?
            };
            // 検証後は構造化済み side context だけを保持し、aibe や子プロセスへ
            // token を含む human-shell env を継承させない。
            if side.is_some() {
                for key in HANDOFF_ENV_KEYS {
                    std::env::remove_var(key);
                }
            }
            run_ask(AskArgs {
                turn,
                file,
                message,
                invocation: ask_invocation,
                collaborative: collaborative && side.is_none(),
                side,
            })
        }
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
        AiCommand::Smart { command } => run_smart_command(command),
        AiCommand::Goal { command } => run_goal(command),
        AiCommand::Now { command } => run_now(command),
        AiCommand::Idea { command } => run_idea(command),
        AiCommand::Mem { command } => run_mem(command),
        AiCommand::Context { command } => run_context(command),
        AiCommand::Work { command, options } => run_work(command, options),
        AiCommand::Recall { command } => run_recall_command(command),
    }
}

#[derive(Debug)]
struct AskArgs {
    turn: TurnOptions,
    file: Option<PathBuf>,
    message: Vec<String>,
    invocation: AskInvocationSource,
    collaborative: bool,
    side: Option<SideLaunch>,
}

#[derive(Debug, Clone)]
enum SideLaunch {
    PromptThenStart(CollaborativeShellEnvironment),
    Run(SideTurn),
}

fn resolve_side_dispatch(
    collaborative: bool,
    bare: bool,
    note: String,
) -> anyhow::Result<Option<SideLaunch>> {
    let values = HANDOFF_ENV_KEYS
        .into_iter()
        .filter_map(|key| {
            std::env::var(key)
                .ok()
                .map(|value| (key.to_string(), value))
        })
        .collect();
    let Some(env) = CollaborativeShellEnvironment::from_map(&values)? else {
        return Ok(None);
    };
    let store = FilesystemHandoffStore::new(FilesystemHandoffStore::default_root());
    let observer = ProcessEnvironmentObserver;
    let runtime = SystemHandoffRuntime;
    let service = StartOrResumeSideAgent::new(&store, &observer, &runtime);
    let invocation = SideAgentInvocation {
        standalone: false,
        collaborative_requested: collaborative,
        bare,
        user_note: (!note.is_empty()).then_some(note),
        client_id: format!("ai-{}", std::process::id()),
        process_id: std::process::id(),
        tty: std::env::var("TTY").ok(),
        cwd: std::env::current_dir()?,
    };
    match service.dispatch(Some(env.clone()), &invocation)? {
        SideAgentDispatch::PromptForInput { .. } => Ok(Some(SideLaunch::PromptThenStart(env))),
        SideAgentDispatch::Run(turn) => Ok(Some(SideLaunch::Run(turn))),
        SideAgentDispatch::Standalone | SideAgentDispatch::Normal => Ok(None),
    }
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
    shell_log_mode: ShellLogMode,
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
    silent_exec: bool,
    shell_exec_approval: Option<String>,
    console_hint: ConsoleHintReport,
    trace_route: bool,
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

fn auto_approve_shell_exec_decision(
    prompt: &ShellExecApprovalPrompt,
    tier: ShellExecTier,
    origin: aibe_protocol::ShellExecApprovalOrigin,
    session: &mut ShellExecSessionState,
    silent: bool,
) -> ShellExecApprovalDecision {
    ai::adapters::outbound::emit_auto_approved_shell_exec(prompt, tier, origin, silent);
    session.allow_session_shell();
    ShellExecApprovalDecision {
        approved: true,
        approval_origin: origin,
        handoff_result: None,
    }
}

fn run_ask(args: AskArgs) -> anyhow::Result<ExitCode> {
    let cfg = AiConfig::load();
    let mut message = match &args.side {
        Some(SideLaunch::Run(turn)) if turn.control_returned.is_some() => ResolvedMessage {
            source: "collaborative_human_control_returned".into(),
            content: serde_json::to_string(turn.control_returned.as_ref().expect("checked"))?,
        },
        _ => match resolve_ask_message(args.file.clone(), args.message, args.invocation)? {
            ResolveAskMessageOutcome::Ready(message) => message,
            ResolveAskMessageOutcome::Cancelled { message } => {
                eprintln!("{message}");
                return Ok(ExitCode::SUCCESS);
            }
            ResolveAskMessageOutcome::EditorFailed { exit_code } => {
                let code = exit_code.unwrap_or(-1);
                eprintln!("AISH: editor exited with status {code}; cancelled.");
                return Ok(ExitCode::from(1u8));
            }
        },
    };
    let mut side_turn = match args.side {
        Some(SideLaunch::Run(turn)) => Some(turn),
        Some(SideLaunch::PromptThenStart(env)) => {
            let store = FilesystemHandoffStore::new(FilesystemHandoffStore::default_root());
            let observer = ProcessEnvironmentObserver;
            let runtime = SystemHandoffRuntime;
            let service = StartOrResumeSideAgent::new(&store, &observer, &runtime);
            let invocation = SideAgentInvocation {
                standalone: false,
                collaborative_requested: false,
                bare: false,
                user_note: Some(message.content.clone()),
                client_id: format!("ai-{}", std::process::id()),
                process_id: std::process::id(),
                tty: std::env::var("TTY").ok(),
                cwd: std::env::current_dir()?,
            };
            match service.dispatch(Some(env), &invocation)? {
                SideAgentDispatch::Run(turn) => Some(turn),
                _ => {
                    return Err(anyhow::anyhow!(
                        "side agent did not start after prompt input"
                    ))
                }
            }
        }
        None => None,
    };
    if let Some(turn) = side_turn
        .as_ref()
        .filter(|turn| turn.control_returned.is_some())
    {
        message.content = serde_json::to_string(turn.control_returned.as_ref().expect("checked"))?;
    }
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
        run_smart_route_with_preprocessor(
            &cfg,
            "ask",
            &base_settings.socket_path,
            &message.content,
            &base_settings,
            &args.turn,
            RouteTurnHints::default(),
            resolve_route_metadata_from_history(&cfg, &base_settings.ai_session_id),
            resolve_history_id(&cfg, &base_settings.ai_session_id),
        )
    } else {
        SmartRouteOutcome::disabled()
    };
    let prep = apply_smart_route_and_features(
        &cfg,
        &message.content,
        args.turn.clone(),
        &base_settings,
        smart,
    );
    let settings = resolve_turn_settings(&cfg, &prep.effective_turn)?;
    let response = match execute_turn(
        &cfg,
        "ask",
        message.clone(),
        settings,
        None,
        None,
        prep.agent_messages,
        prep.feature_summaries,
        side_turn
            .as_ref()
            .map(|side| side.conversation_id.clone())
            .or(prep.conversation_id),
        prep.route_plan_json,
        prep.route_fallback,
        prep.observation_draft,
        Arc::new(std::sync::Mutex::new(ShellExecSessionState::default())),
        args.collaborative,
        side_turn.as_ref(),
    ) {
        Ok(response) => response,
        Err(error) => {
            if let Some(side) = side_turn.as_ref() {
                let store = FilesystemHandoffStore::new(FilesystemHandoffStore::default_root());
                let observer = ProcessEnvironmentObserver;
                let runtime = SystemHandoffRuntime;
                let service = StartOrResumeSideAgent::new(&store, &observer, &runtime);
                service.finish_side_turn(&side.handoff_id, &format!("side run failed: {error}"))?;
            }
            return Err(error);
        }
    };
    if let Some(side) = side_turn.take() {
        let store = FilesystemHandoffStore::new(FilesystemHandoffStore::default_root());
        let observer = ProcessEnvironmentObserver;
        let runtime = SystemHandoffRuntime;
        let service = StartOrResumeSideAgent::new(&store, &observer, &runtime);
        let summary = match &response.response {
            ClientResponse::AgentTurnResult {
                assistant_message, ..
            } => assistant_message.content.as_str(),
            ClientResponse::Error { message, .. } => message.as_str(),
            _ => "side turn finished",
        };
        if let Some(request) = parse_request_human_action(summary) {
            service.request_human_action(&side.handoff_id, request)?;
        } else {
            service.finish_side_turn(&side.handoff_id, summary)?;
        }
    }
    Ok(exit_code_for_response(
        &response.response,
        response.cancel_source,
    ))
}

fn run_chat(turn: TurnOptions) -> anyhow::Result<ExitCode> {
    let cfg = AiConfig::load();
    let settings = resolve_turn_settings(&cfg, &turn)?;
    let shell_exec_state = Arc::new(std::sync::Mutex::new(ShellExecSessionState::default()));
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
            Vec::new(),
            Some(conversation_id.clone()),
            None,
            false,
            None,
            Arc::clone(&shell_exec_state),
            false,
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
    let message = ResolvedMessage {
        source: format!("history:{history_id}"),
        content: payload.user_message.clone(),
    };
    let base_settings = resolve_turn_settings(&cfg, &turn)?;
    let (
        settings,
        messages,
        feature_summaries,
        conversation_id,
        route_plan_json,
        route_fallback,
        observation_draft,
    ) = if should_reapply_smart_features(&payload) {
        let smart = run_smart_route_with_preprocessor(
            &cfg,
            "retry",
            &base_settings.socket_path,
            &message.content,
            &base_settings,
            &turn,
            route_turn_hints_from_payload(&payload),
            route_metadata_from_payload(Some(&payload)),
            Some(payload.history_id.clone()),
        );
        let prep = apply_smart_route_and_features(
            &cfg,
            &message.content,
            turn.clone(),
            &base_settings,
            smart,
        );
        (
            resolve_turn_settings(&cfg, &prep.effective_turn)?,
            prep.agent_messages,
            prep.feature_summaries,
            prep.conversation_id,
            prep.route_plan_json,
            prep.route_fallback,
            prep.observation_draft,
        )
    } else {
        (
            base_settings,
            replay_messages_from_payload(&payload),
            Vec::new(),
            payload.conversation_id.clone(),
            payload.route_plan.clone(),
            payload.route_fallback,
            None,
        )
    };
    let response = execute_turn(
        &cfg,
        "retry",
        message.clone(),
        settings,
        None,
        None,
        messages,
        feature_summaries,
        conversation_id,
        route_plan_json,
        route_fallback,
        observation_draft,
        Arc::new(std::sync::Mutex::new(ShellExecSessionState::default())),
        false,
        None,
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
    let base_settings = resolve_turn_settings(&cfg, &merged_turn)?;
    let (
        settings,
        messages,
        feature_summaries,
        conversation_id,
        route_plan_json,
        route_fallback,
        observation_draft,
    ) = if should_reapply_smart_features(&payload) {
        let smart = run_smart_route_with_preprocessor(
            &cfg,
            "rerun",
            &base_settings.socket_path,
            &message.content,
            &base_settings,
            &merged_turn,
            route_turn_hints_from_payload(&payload),
            route_metadata_from_payload(Some(&payload)),
            Some(payload.history_id.clone()),
        );
        let prep = apply_smart_route_and_features(
            &cfg,
            &message.content,
            merged_turn.clone(),
            &base_settings,
            smart,
        );
        (
            resolve_turn_settings(&cfg, &prep.effective_turn)?,
            prep.agent_messages,
            prep.feature_summaries,
            prep.conversation_id,
            prep.route_plan_json,
            prep.route_fallback,
            prep.observation_draft,
        )
    } else {
        (
            base_settings,
            replay_messages_from_payload(&payload),
            Vec::new(),
            payload.conversation_id.clone(),
            payload.route_plan.clone(),
            payload.route_fallback,
            None,
        )
    };
    let response = execute_turn(
        &cfg,
        "rerun",
        message.clone(),
        settings,
        payload.shell_log_tail.clone(),
        payload.client_cwd.map(PathBuf::from),
        messages,
        feature_summaries,
        conversation_id,
        route_plan_json,
        route_fallback,
        observation_draft,
        Arc::new(std::sync::Mutex::new(ShellExecSessionState::default())),
        false,
        None,
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

fn run_smart_command(command: SmartCommand) -> anyhow::Result<ExitCode> {
    match command {
        SmartCommand::Stats {
            format,
            path,
            limit,
            since_hours,
            session,
        } => {
            let path = resolve_smart_observation_path(path);
            let read = ai::adapters::outbound::read_smart_observation_log(&path, limit)?;
            let records = ai::domain::filter_observations(
                read.records,
                session.as_deref(),
                since_cutoff_ms(since_hours),
            );
            let stats = ai::domain::SmartObservationStats::from_records(
                &records,
                read.total_records,
                read.invalid_lines,
            );
            write_stdout(stats.render(format.into())?)?;
        }
        SmartCommand::Recent {
            format,
            path,
            limit,
            session,
        } => {
            let path = resolve_smart_observation_path(path);
            let read = ai::adapters::outbound::read_smart_observation_log(&path, limit)?;
            let records = ai::domain::filter_observations(read.records, session.as_deref(), None);
            write_stdout(ai::domain::render_recent(&records, format.into())?)?;
        }
        SmartCommand::Report {
            path,
            limit,
            since_hours,
            session,
            include_recent,
        } => {
            let path = resolve_smart_observation_path(path);
            let read = ai::adapters::outbound::read_smart_observation_log(&path, limit)?;
            let records = ai::domain::filter_observations(
                read.records,
                session.as_deref(),
                since_cutoff_ms(since_hours),
            );
            let stats = ai::domain::SmartObservationStats::from_records(
                &records,
                read.total_records,
                read.invalid_lines,
            );
            let path_display = path.display().to_string();
            write_stdout(ai::domain::render_markdown_report(
                &stats,
                &records,
                ai::domain::SmartReportOptions {
                    observation_path: &path_display,
                    limit,
                    since_hours,
                    session_filter: session.as_deref(),
                    include_recent,
                },
            ))?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn resolve_smart_observation_path(path: Option<PathBuf>) -> PathBuf {
    ai::adapters::outbound::expand_observation_path(
        path.unwrap_or_else(ai::adapters::outbound::default_observation_path),
    )
}

fn since_cutoff_ms(since_hours: Option<u64>) -> Option<u64> {
    let age_ms = since_hours?.saturating_mul(60 * 60 * 1000);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_millis().min(u64::MAX as u128) as u64
        });
    Some(now_ms.saturating_sub(age_ms))
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
    feature_history_summaries: Vec<ProtocolMessage>,
    conversation_id: Option<String>,
    route_plan_json: Option<String>,
    route_fallback: bool,
    observation_draft: Option<PreprocessorObservationDraft>,
    shell_exec_state: Arc<std::sync::Mutex<ShellExecSessionState>>,
    collaborative: bool,
    side_turn: Option<&SideTurn>,
) -> anyhow::Result<TurnExecutionOutcome> {
    let shell_log_choice = settings.shell_log_choice.clone();
    let replay_ctx = ai::application::replay_manifest::build_turn_replay_context(
        settings.shell_log_mode,
        &shell_log_choice,
        shell_log_override,
        || match &shell_log_choice {
            ShellLogChoice::Path(path) => FileLogTail::new(path.clone())
                .tail_bytes(settings.log_tail_bytes)
                .map(Some)
                .map_err(|e: LogReadError| e.to_string()),
            ShellLogChoice::None => Ok(None),
        },
        |path| load_replay_events(path).map_err(|e| e.to_string()),
    )
    .map_err(|e| anyhow::anyhow!(e))?;
    if let Some(msg) = replay_ctx.manifest_fallback.as_ref() {
        if !settings.quiet {
            eprintln!("ai: replay manifest unavailable ({msg}); falling back to shell_log_tail");
        }
    }
    let shell_log_tail_text = replay_ctx.shell_log_tail;
    let replay_events = replay_ctx.replay_events;
    let replay_manifest_block = replay_ctx.replay_manifest_block;
    let client_tools = replay_ctx.client_tools;

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
    let presenter = Arc::new(
        StdoutPresenter::with_options(
            settings.output_filter.clone(),
            settings.output_format,
            settings.quiet,
            progress_spinner,
        )
        .with_shell_exec_render(ShellExecRenderOptions {
            silent: settings.silent_exec,
            show_always_mode_summary: matches!(
                settings.shell_exec_approval.as_deref(),
                Some("always")
            ),
        }),
    );
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
        client_tools,
        replay_events,
        replay_manifest_block: replay_manifest_block.clone(),
        llm_profile: settings.llm_profile.clone(),
        ai_session_id: Some(settings.ai_session_id.clone()),
        conversation_id: conversation_id.clone(),
    };
    let mut request = ask_input.into_request()?;
    let replay_events = request.replay_events.clone();
    let memory_space_id =
        resolve_turn_memory_space_id(cfg, request.client_cwd.as_deref(), &settings.ai_session_id);
    request.request_context = build_request_context(
        shell_log_tail_text.clone(),
        request.client_cwd.as_deref(),
        settings.ai_session_id.clone(),
        conversation_id.clone(),
        &settings.console_hint,
        memory_space_id,
        replay_manifest_block.clone(),
    );
    if let Some(side) = side_turn {
        request.request_context.conversation_id = Some(side.conversation_id.clone());
        request.request_context.system_instruction =
            Some(match request.request_context.system_instruction.take() {
                Some(existing) => format!("{existing}\n\n{}", side.system_instruction),
                None => side.system_instruction.clone(),
            });
    }
    let turn_id = next_history_id();
    let request_messages = history_messages_from_protocol(&messages);
    let feature_summaries = history_messages_from_protocol(&feature_history_summaries);
    let mut client_request = request_from_messages(turn_id.clone(), request, messages)?;
    if let ClientRequest::AgentTurn { context, .. } = &mut client_request {
        context.collaborative_handoff = collaborative;
    }

    let yes_exec_effective =
        settings.yes_exec && matches!(settings.shell_exec_approval.as_deref(), Some("ask"));
    let _progress_guard = presenter.progress_guard();
    let agent_turn_started = Instant::now();
    let response = if settings.timeout_secs.is_some() || settings.progress || yes_exec_effective {
        run_agent_turn_async(
            plan.socket_path.clone(),
            client_request,
            replay_events.clone(),
            presenter.clone(),
            cfg.history_dir.clone(),
            settings.session_id.clone(),
            yes_exec_effective,
            settings.progress,
            settings.timeout_secs,
            settings.silent_exec || settings.quiet,
            Arc::clone(&shell_exec_state),
            collaborative,
        )?
    } else {
        run_agent_turn_sync(
            plan.socket_path.clone(),
            client_request,
            replay_events,
            presenter.clone(),
            cfg.history_dir.clone(),
            settings.session_id.clone(),
            yes_exec_effective,
            settings.progress,
            settings.silent_exec || settings.quiet,
            shell_exec_state,
            collaborative,
        )?
    };

    let TurnExecutionOutcome {
        response,
        cancel_source,
        streamed,
    } = response;
    let agent_turn_latency_ms = agent_turn_started.elapsed().as_millis() as u64;
    if let Some(draft) = observation_draft {
        finalize_preprocessor_observation(
            draft,
            agent_turn_latency_ms,
            smart_preprocessor_trace_enabled(settings.trace_route),
        );
    }
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
        conversation_id: conversation_id.clone(),
        ai_session_id: Some(settings.ai_session_id.clone()),
        shell_exec_approval: settings.shell_exec_approval.clone(),
        route_plan: route_plan_redacted,
        route_fallback,
        socket_path: settings.socket_path.display().to_string(),
        log_tail_bytes: settings.log_tail_bytes,
        request_messages,
        feature_summaries,
    };
    let store = LocalHistoryStore::new(cfg.history_dir.clone());
    record_turn(
        &store,
        &record_input,
        &replay_input,
        cfg.history_max_entries,
    )
    .map_err(history_store_to_anyhow)?;
    maybe_persist_suggested_commands(cfg, &settings, &turn_id, &conversation_id, &response)?;
    presenter.show_response(&response, settings.verbose_tools, streamed);

    Ok(TurnExecutionOutcome {
        response,
        cancel_source,
        streamed,
    })
}

fn maybe_persist_suggested_commands(
    cfg: &AiConfig,
    settings: &ResolvedTurnSettings,
    turn_id: &str,
    conversation_id: &Option<String>,
    response: &ClientResponse,
) -> anyhow::Result<()> {
    let Some(content) = assistant_content_from_response(response) else {
        return Ok(());
    };
    let gating = resolve_recall_gating(RecallGatingInput {
        config_enabled: cfg.suggested_command_recall,
        config_hint: cfg.suggested_command_recall_hint,
        max_items: cfg.suggested_command_recall_max_items,
        quiet: settings.quiet,
        output_format: settings.output_format,
        stdin_tty: std::io::stdin().is_terminal(),
        stdout_tty: std::io::stdout().is_terminal(),
        stderr_tty: std::io::stderr().is_terminal(),
    });
    let cache_path = resolve_suggestion_cache_path(&settings.ai_session_id);
    let store = FileSuggestedCommandRecallStore::new(cache_path);
    let ctx = RecallTurnContext {
        gating,
        ai_session_id: settings.ai_session_id.clone(),
        conversation_id: conversation_id.clone(),
        turn_id: turn_id.to_string(),
        captured_at: recall_timestamp_from_ms(current_time_ms()),
        shell: detect_interactive_shell_name(),
    };
    let outcome =
        persist_suggested_commands(&store, &ctx, content).map_err(|e| anyhow::anyhow!(e))?;
    if let Some(hint) = outcome.hint {
        eprintln!("{hint}");
    }
    Ok(())
}

fn run_recall_command(command: RecallCommand) -> anyhow::Result<ExitCode> {
    let cache_path = std::env::var("AI_SUGGESTION_CACHE")
        .ok()
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| resolve_suggestion_cache_path(&resolve_ai_session_id()));
    let store = FileSuggestedCommandRecallStore::new(cache_path);
    let cmd = match command {
        RecallCommand::Next => recall_next_command(&store),
        RecallCommand::Prev => recall_prev_command(&store),
    }
    .map_err(|e| anyhow::anyhow!(e))?;
    if let Some(text) = cmd {
        print!("{text}");
    }
    Ok(ExitCode::SUCCESS)
}

fn detect_interactive_shell_name() -> String {
    std::env::var("SHELL")
        .ok()
        .and_then(|shell| shell.rsplit('/').next().map(str::to_ascii_lowercase))
        .filter(|name| name == "bash" || name == "zsh")
        .unwrap_or_else(|| "bash".to_string())
}

fn recall_timestamp_from_ms(ms: u64) -> String {
    let secs = ms / 1000;
    format!("{secs}")
}

#[allow(clippy::too_many_arguments)]
fn run_agent_turn_sync(
    socket_path: PathBuf,
    request: ClientRequest,
    replay_events: Vec<aish_replay::LogEvent>,
    presenter: Arc<StdoutPresenter>,
    history_dir: PathBuf,
    session_id: Option<String>,
    yes_exec: bool,
    progress: bool,
    silent_shell_exec: bool,
    shell_exec_state: Arc<std::sync::Mutex<ShellExecSessionState>>,
    collaborative: bool,
) -> anyhow::Result<TurnExecutionOutcome> {
    run_agent_turn_core(
        socket_path,
        request,
        replay_events,
        presenter,
        history_dir,
        session_id,
        yes_exec,
        progress,
        None,
        silent_shell_exec,
        shell_exec_state,
        collaborative,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_agent_turn_async(
    socket_path: PathBuf,
    request: ClientRequest,
    replay_events: Vec<aish_replay::LogEvent>,
    presenter: Arc<StdoutPresenter>,
    history_dir: PathBuf,
    session_id: Option<String>,
    yes_exec: bool,
    progress: bool,
    timeout_secs: Option<u64>,
    silent_shell_exec: bool,
    shell_exec_state: Arc<std::sync::Mutex<ShellExecSessionState>>,
    collaborative: bool,
) -> anyhow::Result<TurnExecutionOutcome> {
    run_agent_turn_core(
        socket_path,
        request,
        replay_events,
        presenter,
        history_dir,
        session_id,
        yes_exec,
        progress,
        timeout_secs,
        silent_shell_exec,
        shell_exec_state,
        collaborative,
    )
}

fn should_use_client_tool_stream(request: &ClientRequest) -> bool {
    matches!(
        request,
        ClientRequest::AgentTurn { client_tools, .. } if !client_tools.is_empty()
    )
}

#[allow(clippy::too_many_arguments)]
fn run_agent_turn_core(
    socket_path: PathBuf,
    request: ClientRequest,
    replay_events: Vec<aish_replay::LogEvent>,
    presenter: Arc<StdoutPresenter>,
    history_dir: PathBuf,
    session_id: Option<String>,
    yes_exec: bool,
    progress: bool,
    timeout_secs: Option<u64>,
    silent_shell_exec: bool,
    shell_exec_state: Arc<std::sync::Mutex<ShellExecSessionState>>,
    collaborative: bool,
) -> anyhow::Result<TurnExecutionOutcome> {
    let turn_id = request_turn_id(&request)?;
    let collaborative_meta = match &request {
        ClientRequest::AgentTurn {
            messages, context, ..
        } => Some((
            context.cwd.clone().map(PathBuf::from),
            context.ai_session_id.clone(),
            context.conversation_id.clone(),
            messages
                .iter()
                .find(|m| m.role == "user")
                .map(|m| m.content.clone()),
        )),
        _ => None,
    };
    let worker_client = AibeUnixClient::new(socket_path.clone());
    let cancel_client = AibeUnixClient::new(socket_path);
    let cancel_guard = TurnCancelGuard::new().map_err(|e| anyhow::anyhow!("{e}"))?;
    let cancel_requested = Arc::clone(cancel_guard.flag());
    let streamed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut cancel_source: Option<TurnCancelSource> = None;

    let use_client_tools = should_use_client_tool_stream(&request);
    let (tx, rx) = mpsc::channel();
    let presenter_thread = Arc::clone(&presenter);
    let history_dir_thread = history_dir.clone();
    let turn_id_thread = turn_id.clone();
    let streamed_thread = Arc::clone(&streamed);
    let shell_exec_state_thread = Arc::clone(&shell_exec_state);
    let silent_shell_exec_thread = silent_shell_exec;
    let aibe_shell_exec = load_shell_exec_approval();
    let auto_patterns = ai::domain::parse_shell_exec_auto_approve_patterns(
        aibe_shell_exec.auto_approve_patterns.read_only,
        aibe_shell_exec.auto_approve_patterns.mutating,
    );
    thread::spawn(move || {
        let handoff_store = FilesystemHandoffStore::new(FilesystemHandoffStore::default_root());
        let human_shell_launcher = AishHumanShellLauncher::default();
        let environment_observer = ProcessEnvironmentObserver;
        let parent_tool_barrier = NoopParentToolBarrier;
        let handoff_runtime = SystemHandoffRuntime;
        let candidate_store = FileSuggestedCommandRecallStore::new(resolve_suggestion_cache_path(
            collaborative_meta
                .as_ref()
                .and_then(|m| m.1.as_deref())
                .unwrap_or("default"),
        ));
        let candidate_publisher = FileHandoffCandidatePublisher::new(
            candidate_store,
            collaborative_meta
                .as_ref()
                .and_then(|m| m.1.clone())
                .unwrap_or_else(|| "default".into()),
        );
        let collaborative_policy = CollaborativeShellExecPolicy::new(
            if collaborative {
                CollaborativeExecutionContext::parent_enabled()
            } else {
                CollaborativeExecutionContext::disabled()
            },
            &handoff_store,
            &human_shell_launcher,
            &environment_observer,
            &parent_tool_barrier,
            &candidate_publisher,
            &handoff_runtime,
        );
        let mut yes_exec_cache = if yes_exec {
            Some(YesExecCache::load(
                &history_dir_thread,
                session_id.as_deref(),
            ))
        } else {
            None
        };
        let response = {
            let shell_exec_handler =
                |prompt: ShellExecApprovalPrompt| -> ShellExecApprovalDecision {
                    if collaborative {
                        let (cwd, ai_session_id, conversation_id, parent_summary) =
                            collaborative_meta.clone().unwrap_or_default();
                        let request = ParentShellExecRequest {
                            parent_task_id: ai_session_id
                                .clone()
                                .unwrap_or_else(|| turn_id_thread.clone()),
                            parent_conversation_id: conversation_id
                                .unwrap_or_else(|| turn_id_thread.clone()),
                            parent_run_id: turn_id_thread.clone(),
                            parent_goal_id: None,
                            parent_goal: parent_summary
                                .clone()
                                .unwrap_or_else(|| "Complete the parent task".into()),
                            parent_request_summary: parent_summary
                                .clone()
                                .unwrap_or_else(|| "Parent requested shell work".into()),
                            conversation_snapshot: parent_summary.clone().unwrap_or_default(),
                            conversation_summary: parent_summary.unwrap_or_default(),
                            command: prompt.command.clone(),
                            args: prompt.args.clone(),
                            cwd: cwd.unwrap_or_else(|| PathBuf::from(".")),
                            tool_call_id: prompt.tool_call_id.clone(),
                            shell_log_start: 0,
                        };
                        return match collaborative_policy.intercept(request) {
                            Ok(handoff_result) => ShellExecApprovalDecision {
                                approved: true,
                                approval_origin:
                                    aibe_protocol::ShellExecApprovalOrigin::CollaborativeHandoff,
                                handoff_result: Some(handoff_result),
                            },
                            Err(error) => {
                                eprintln!("ai: collaborative handoff failed: {error}");
                                ShellExecApprovalDecision {
                                    approved: false,
                                    approval_origin:
                                        aibe_protocol::ShellExecApprovalOrigin::CollaborativeHandoff,
                                    handoff_result: None,
                                }
                            }
                        };
                    }
                    let tier = ai::domain::classify_shell_exec_tier(&prompt.command, &prompt.args);
                    let invocation =
                        ai::domain::canonical_shell_exec_invocation(&prompt.command, &prompt.args);
                    let session_allowed = {
                        let session = shell_exec_state_thread.lock().expect("shell exec session");
                        session.session_shell_allowed()
                    };

                    {
                        let mut session =
                            shell_exec_state_thread.lock().expect("shell exec session");
                        if session_allowed && tier != ShellExecTier::Destructive {
                            if let Some(Ok(cache)) = yes_exec_cache.as_ref() {
                                if let Some(scope) =
                                    cache.should_auto_approve(&prompt.command, &prompt.args, tier)
                                {
                                    let origin = match scope {
                                    ai::domain::ShellExecRememberScope::ExactInvocation => {
                                        aibe_protocol::ShellExecApprovalOrigin::SessionCacheExactInvocation
                                    }
                                    ai::domain::ShellExecRememberScope::CommandName => {
                                        aibe_protocol::ShellExecApprovalOrigin::SessionCacheCommandName
                                    }
                                };
                                    return auto_approve_shell_exec_decision(
                                        &prompt,
                                        tier,
                                        origin,
                                        &mut session,
                                        silent_shell_exec_thread,
                                    );
                                }
                            }
                            let exact_key =
                                ai::domain::exact_shell_exec_key(&prompt.command, &prompt.args);
                            if session.has_exact(&exact_key) {
                                return auto_approve_shell_exec_decision(
                                &prompt,
                                tier,
                                aibe_protocol::ShellExecApprovalOrigin::SessionCacheExactInvocation,
                                &mut session,
                                silent_shell_exec_thread,
                            );
                            }
                            let command_key =
                                ai::domain::command_shell_exec_key(&prompt.command, tier);
                            if session.has_command(&command_key) {
                                return auto_approve_shell_exec_decision(
                                    &prompt,
                                    tier,
                                    aibe_protocol::ShellExecApprovalOrigin::SessionCacheCommandName,
                                    &mut session,
                                    silent_shell_exec_thread,
                                );
                            }
                            if let Some(patterns) = auto_patterns.as_ref() {
                                if let Some((_, origin)) =
                                    ai::domain::match_shell_exec_auto_approve_pattern(
                                        &invocation,
                                        tier,
                                        patterns,
                                    )
                                {
                                    return auto_approve_shell_exec_decision(
                                        &prompt,
                                        tier,
                                        origin,
                                        &mut session,
                                        silent_shell_exec_thread,
                                    );
                                }
                            }
                            if tier == ShellExecTier::ReadOnly {
                                return auto_approve_shell_exec_decision(
                                    &prompt,
                                    tier,
                                    aibe_protocol::ShellExecApprovalOrigin::SessionAllowed,
                                    &mut session,
                                    silent_shell_exec_thread,
                                );
                            }
                        }
                    }

                    let decision = ai::adapters::outbound::prompt_shell_exec_approval(
                        prompt.clone(),
                        tier,
                        session_allowed,
                    );
                    if decision.approved {
                        let mut session =
                            shell_exec_state_thread.lock().expect("shell exec session");
                        session.allow_session_shell();
                        if let Some(scope) = decision.remember_scope {
                            if tier != ShellExecTier::Destructive {
                                match scope {
                                    ai::domain::ShellExecRememberScope::ExactInvocation => {
                                        session.remember_exact(ai::domain::exact_shell_exec_key(
                                            &prompt.command,
                                            &prompt.args,
                                        ));
                                    }
                                    ai::domain::ShellExecRememberScope::CommandName => {
                                        session.remember_command(
                                            ai::domain::command_shell_exec_key(
                                                &prompt.command,
                                                tier,
                                            ),
                                        );
                                    }
                                }
                                if let Some(Ok(cache)) = yes_exec_cache.as_mut() {
                                    let _ =
                                        cache.remember(&prompt.command, &prompt.args, tier, scope);
                                }
                            }
                        }
                    }
                    ShellExecApprovalDecision {
                        approved: decision.approved,
                        approval_origin: decision.approval_origin,
                        handoff_result: None,
                    }
                };
            let result = if use_client_tools {
                worker_client.agent_turn_request_stream_with_client_tools(
                    request,
                    |event: AgentTurnProgressEvent| {
                        if progress {
                            let phase = progress_phase_name(event.phase);
                            presenter_thread.show_progress(&phase, event.message.as_deref());
                        }
                    },
                    |chunk| {
                        if !chunk.is_empty() {
                            if presenter_thread.assistant_stream_stdout_enabled() {
                                streamed_thread.store(true, Ordering::SeqCst);
                            }
                            presenter_thread.show_stream_chunk(&chunk);
                        }
                    },
                    replay_client_tool_callback(replay_events),
                    shell_exec_handler,
                )
            } else {
                worker_client.agent_turn_request_stream(
                    request,
                    |event: AgentTurnProgressEvent| {
                        if progress {
                            let phase = progress_phase_name(event.phase);
                            presenter_thread.show_progress(&phase, event.message.as_deref());
                        }
                    },
                    |chunk| {
                        if !chunk.is_empty() {
                            if presenter_thread.assistant_stream_stdout_enabled() {
                                streamed_thread.store(true, Ordering::SeqCst);
                            }
                            presenter_thread.show_stream_chunk(&chunk);
                        }
                    },
                    shell_exec_handler,
                )
            };
            match result {
                Ok(resp) => resp,
                Err(e) => ClientResponse::Error {
                    id: turn_id_thread.clone(),
                    code: aibe_protocol::ErrorCode::InternalError,
                    message: e.to_string(),
                },
            }
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
        | ClientRequest::ToolApproval { id, .. }
        | ClientRequest::CancelTurn { id, .. } => Ok(id.clone()),
        ClientRequest::MemoryApply(body) => Ok(body.id.clone()),
        ClientRequest::MemoryQuery(body) => Ok(body.id.clone()),
        ClientRequest::MemoryKindList(body) => Ok(body.id.clone()),
        ClientRequest::MemoryRecipeRun(body) => Ok(body.id.clone()),
        ClientRequest::MemorySubscribe(body) => Ok(body.id.clone()),
        ClientRequest::WorkApply(body) => Ok(body.id.clone()),
        ClientRequest::WorkQuery(body) => Ok(body.id.clone()),
        ClientRequest::ClientToolResult(body) => Ok(body.id.clone()),
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

/// turn 用の `memory_space_id` を解決する（best-effort。失敗時は aibe 側の解決に任せる）。
#[cfg(not(feature = "memory"))]
fn resolve_turn_memory_space_id(
    _cfg: &AiConfig,
    _client_cwd: Option<&Path>,
    _ai_session_id: &str,
) -> Option<String> {
    None
}

#[cfg(feature = "memory")]
fn resolve_turn_memory_space_id(
    cfg: &AiConfig,
    client_cwd: Option<&Path>,
    ai_session_id: &str,
) -> Option<String> {
    if !cfg.memory_enabled {
        return None;
    }
    let canonical_cwd = client_cwd.and_then(|p| p.canonicalize().ok());
    let project_key = canonical_cwd.as_deref().and_then(|p| {
        ai::adapters::outbound::project_key::canonical_project_key_from_cwd(p)
            .ok()
            .flatten()
    });
    let env_context = std::env::var("AIBE_CONTEXT_ID").ok();
    resolve_memory_space_id(
        ai_session_id,
        project_key.as_deref(),
        cfg.context_current.as_deref(),
        env_context.as_deref(),
    )
    .ok()
    .map(|resolution| resolution.memory_space_id)
}

fn build_feature_memory_context(
    cfg: &AiConfig,
    ai_session_id: &str,
    quiet: bool,
) -> Option<aibe_protocol::MemoryContext> {
    if !cfg.memory_enabled {
        return None;
    }
    let cwd = std::env::current_dir().ok()?;
    let canonical_cwd = match cwd.canonicalize() {
        Ok(path) => path,
        Err(e) => {
            if !quiet {
                eprintln!("ai: smart feature plan: failed to canonicalize cwd: {e}");
            }
            return None;
        }
    };
    let project_key =
        ai::adapters::outbound::project_key::canonical_project_key_from_cwd(&canonical_cwd)
            .ok()
            .flatten();
    let env_context = std::env::var("AIBE_CONTEXT_ID").ok();
    match ai::application::memory_space::build_memory_context(
        ai_session_id,
        &canonical_cwd,
        project_key.as_deref(),
        cfg.context_current.as_deref(),
        env_context.as_deref(),
    ) {
        Ok(context) => Some(context),
        Err(e) => {
            if !quiet {
                eprintln!("ai: smart feature plan: failed to build memory context: {e}");
            }
            None
        }
    }
}

fn build_request_context(
    shell_log_tail: Option<String>,
    client_cwd: Option<&Path>,
    ai_session_id: String,
    conversation_id: Option<String>,
    console_hint: &ConsoleHintReport,
    memory_space_id: Option<String>,
    replay_manifest_block: Option<String>,
) -> RequestContextInput {
    let terminal_size = if console_hint.effective {
        detect_terminal_size()
    } else {
        None
    };
    let mut ctx = RequestContextInput {
        shell_log_tail,
        cwd: client_cwd.map(|p| p.display().to_string()),
        ai_session_id: Some(ai_session_id),
        conversation_id,
        memory_space_id,
        ..Default::default()
    }
    .with_console_system_instruction(terminal_size, console_hint.effective);
    if let Some(block) = replay_manifest_block {
        ctx.system_instruction = Some(match ctx.system_instruction.take() {
            Some(existing) if !existing.is_empty() => format!("{existing}\n\n{block}"),
            _ => block,
        });
    }
    ctx
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
        client_tools: request.client_tools,
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
        | ClientResponse::ToolApprovalPrompt { .. }
        | ClientResponse::ClientToolCallRequested { .. }
        | ClientResponse::MemoryApplyResult { .. }
        | ClientResponse::MemoryQueryResult { .. }
        | ClientResponse::MemoryKindListResult { .. }
        | ClientResponse::MemoryRecipeRunResult { .. }
        | ClientResponse::MemorySubscribeResult { .. }
        | ClientResponse::MemoryChanged { .. }
        | ClientResponse::WorkApplyResult(_)
        | ClientResponse::WorkQueryResult(_) => ExitCode::SUCCESS,
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
    local_route: Option<LocalRouteDecision>,
    observation_draft: Option<PreprocessorObservationDraft>,
}

impl SmartRouteOutcome {
    fn disabled() -> Self {
        Self {
            conversation_id: None,
            route_plan: None,
            route_fallback: false,
            local_route: None,
            observation_draft: None,
        }
    }
}

#[derive(Debug)]
struct SmartFeaturePrep {
    effective_turn: TurnOptions,
    agent_messages: Vec<ProtocolMessage>,
    feature_summaries: Vec<ProtocolMessage>,
    conversation_id: Option<String>,
    route_plan_json: Option<String>,
    route_fallback: bool,
    observation_draft: Option<PreprocessorObservationDraft>,
}

fn should_reapply_smart_features(payload: &HistoryPayload) -> bool {
    std::io::stdin().is_terminal() && payload_eligible_for_smart_rerun(payload)
}

fn payload_eligible_for_smart_rerun(payload: &HistoryPayload) -> bool {
    payload.command == "ask"
}

fn apply_smart_route_and_features(
    cfg: &AiConfig,
    message_content: &str,
    mut turn: TurnOptions,
    base_settings: &ResolvedTurnSettings,
    smart: SmartRouteOutcome,
) -> SmartFeaturePrep {
    let mut feature_extra_messages: Vec<ProtocolMessage> = Vec::new();
    let mut feature_history_summaries: Vec<ProtocolMessage> = Vec::new();
    if let Some(ref plan) = smart.route_plan {
        if !plan.feature_actions.is_empty() {
            let memory_client = AibeUnixClient::new(&base_settings.socket_path);
            let feature_memory_context = build_feature_memory_context(
                cfg,
                &base_settings.ai_session_id,
                base_settings.quiet,
            );
            let outcome = execute_feature_actions_mvp(
                &plan.feature_actions,
                message_content,
                feature_memory_context,
                &base_settings.ai_session_id,
                turn,
                &memory_client,
                base_settings.quiet,
            );
            turn = outcome.turn;
            feature_extra_messages = outcome.extra_messages;
            feature_history_summaries = outcome.history_summaries;
        }
        turn = apply_route_plan_advisory(turn, plan, cfg, base_settings.quiet);
    } else if let Some(ref local) = smart.local_route {
        turn = apply_local_route_tools(&turn, &local.enabled_tools);
        if let Some(summary) = build_local_route_context_summary(&local.context_needs) {
            feature_extra_messages.push(ProtocolMessage {
                role: "system".to_string(),
                content: summary,
            });
        }
        if turn.format.is_none() {
            if let Some(hint) = local_output_style_system_hint(local.output_style) {
                feature_extra_messages.push(ProtocolMessage {
                    role: "system".to_string(),
                    content: hint.to_string(),
                });
            }
        }
    }
    if smart.route_fallback {
        turn.tools = Some("none".to_string());
    }
    let mut agent_messages = feature_extra_messages;
    agent_messages.push(ProtocolMessage {
        role: "user".to_string(),
        content: message_content.to_string(),
    });
    let route_plan_json = smart
        .route_plan
        .as_ref()
        .and_then(|p| serde_json::to_string(p).ok());
    SmartFeaturePrep {
        effective_turn: turn,
        agent_messages,
        feature_summaries: feature_history_summaries,
        conversation_id: smart.conversation_id,
        route_plan_json,
        route_fallback: smart.route_fallback,
        observation_draft: smart.observation_draft,
    }
}

fn turn_has_cli_overrides(turn: &TurnOptions) -> bool {
    turn.preset.is_some()
        || turn.tools.is_some()
        || turn.log_tail.is_some()
        || turn.yes_exec
        || turn.new
}

fn build_preprocess_config(
    cfg: &SmartPreprocessorConfig,
    validated_model: Option<&ai::adapters::outbound::ValidatedPreprocessorModel>,
) -> Option<PreprocessConfig> {
    if !cfg.enabled {
        return None;
    }
    let mode = SmartPreprocessMode::parse(&cfg.mode_raw)?;
    if mode == SmartPreprocessMode::Off {
        return None;
    }
    Some(PreprocessConfig {
        mode,
        route_turn_threshold_bps: (cfg.route_turn_threshold * 10000.0) as u16,
        assist_threshold_bps: (cfg.assist_threshold * 10000.0) as u16,
        max_evidence_bytes: cfg.max_evidence_bytes,
        feature_hash_buckets: cfg.feature_hash_buckets,
        feature_hash_seed: cfg.feature_hash_seed,
        allow_shortcuts: cfg
            .allow_shortcuts
            .iter()
            .filter_map(|s| SmartIntentClass::parse_shortcut(s))
            .filter(|intent| matches!(intent, SmartIntentClass::SimpleChat))
            .collect(),
        model: validated_model.map(|m| m.model.clone()),
    })
}

fn preprocessor_model_load_fallback_reason(model_load_error: Option<&String>) -> Option<String> {
    model_load_error.map(|_| "model_load_failed".to_string())
}

fn preprocessor_route_turn_fallback_reason(model_load_error: Option<&String>) -> Option<String> {
    if model_load_error.is_some() {
        Some("route_turn_failed;model_load_failed".to_string())
    } else {
        Some("route_turn_failed".to_string())
    }
}

#[allow(clippy::too_many_arguments)]
fn build_preprocessor_observation_draft(
    sp_cfg: &SmartPreprocessorConfig,
    decision: &ai::domain::smart_preprocessor::SmartPreprocessDecision,
    ai_session_id: &str,
    conversation_id: Option<String>,
    history_id: Option<String>,
    decision_path: &str,
    route_turn_used: bool,
    fallback_reason: Option<String>,
    local_route: LocalRouteMetrics,
    local_route_decision: Option<LocalRouteDecision>,
) -> PreprocessorObservationDraft {
    PreprocessorObservationDraft {
        observation_path: sp_cfg.observation_path.clone(),
        max_observation_bytes: sp_cfg.max_observation_bytes,
        decision: decision.clone(),
        ai_session_id: ai_session_id.to_string(),
        conversation_id,
        history_id,
        decision_path: decision_path.to_string(),
        route_turn_used,
        fallback_reason,
        local_route,
        local_route_decision,
    }
}

fn resolve_recent_local_history_summary(cfg: &AiConfig, ai_session_id: &str) -> Option<String> {
    let store = LocalHistoryStore::new(cfg.history_dir.clone());
    let entries = store.list().ok()?;
    let entry = entries
        .iter()
        .find(|entry| entry.ai_session_id.as_deref() == Some(ai_session_id))?;
    let summary = format!(
        "request: {}; response: {}",
        entry.request_summary.detail, entry.response_summary.detail
    );
    Some(ai::domain::smart_preprocessor::redact_for_evidence(
        &summary, 240,
    ))
}

fn resolve_history_id(cfg: &AiConfig, ai_session_id: &str) -> Option<String> {
    let store = LocalHistoryStore::new(cfg.history_dir.clone());
    let entries = store.list().ok()?;
    entries
        .iter()
        .find(|entry| entry.ai_session_id.as_deref() == Some(ai_session_id))
        .map(|entry| entry.history_id.clone())
}

fn resolve_route_metadata_from_history(cfg: &AiConfig, ai_session_id: &str) -> RouteMetadataInput {
    let store = LocalHistoryStore::new(cfg.history_dir.clone());
    let Ok(entries) = store.list() else {
        return RouteMetadataInput::default();
    };
    let Some(entry) = entries
        .iter()
        .find(|entry| entry.ai_session_id.as_deref() == Some(ai_session_id))
    else {
        return RouteMetadataInput::default();
    };
    let Ok(payload) = store.load_payload(&entry.history_id) else {
        return RouteMetadataInput::default();
    };
    route_metadata_from_payload(Some(&payload))
}

fn route_metadata_from_payload(payload: Option<&HistoryPayload>) -> RouteMetadataInput {
    let Some(payload) = payload else {
        return RouteMetadataInput::default();
    };
    let mut meta = RouteMetadataInput {
        prior_route_fallback: payload.route_fallback,
        prior_required_approval: false,
        prior_route_kind: None,
    };
    if let Some(ref json) = payload.route_plan {
        if let Ok(plan) = serde_json::from_str::<RoutePlan>(json) {
            meta.prior_route_kind = Some(format!("{:?}", plan.route_kind).to_ascii_lowercase());
            meta.prior_required_approval = plan.require_shell_approval;
        }
    }
    meta
}

fn cli_tool_allowlist_for_local_route(
    settings: &ResolvedTurnSettings,
    turn: &TurnOptions,
) -> Vec<String> {
    if let Some(ref tools) = turn.tools {
        return tools
            .split(',')
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .map(str::to_string)
            .collect();
    }
    if let Ok(resolved) = resolve_tools(settings.tools_cli.as_deref(), &settings.ask_tools) {
        return resolved
            .allowlist
            .into_names()
            .into_iter()
            .map(|name| name.as_str().to_string())
            .collect();
    }
    Vec::new()
}

fn apply_local_route_tools(turn: &TurnOptions, enabled_tools: &[LocalToolHint]) -> TurnOptions {
    let mut turn = turn.clone();
    if turn.tools.is_some() {
        return turn;
    }
    if enabled_tools.is_empty() {
        return turn;
    }
    let names: Vec<String> = enabled_tools
        .iter()
        .map(|tool| tool.runtime_tool_name().to_string())
        .collect();
    turn.tools = Some(names.join(","));
    turn
}

#[allow(clippy::too_many_arguments)]
fn run_preprocessor_pipeline(
    cfg: &AiConfig,
    settings: &ResolvedTurnSettings,
    ai_session_id: &str,
    command: &str,
    query: &str,
    turn: &TurnOptions,
    hints: &RouteTurnHints,
    route_metadata: RouteMetadataInput,
    history_id: Option<String>,
) -> Option<PreprocessorRunOutcome> {
    let sp_cfg = cfg.smart_preprocessor.as_ref()?;
    let (validated_model, model_load_error) = match sp_cfg.model_path.as_ref() {
        Some(path) => match load_preprocessor_model(
            path,
            sp_cfg.feature_hash_buckets,
            sp_cfg.feature_hash_seed,
        ) {
            Ok(model) => (Some(model), None),
            Err(err) => (None, Some(err)),
        },
        None => match load_bundled_preprocessor_model(
            sp_cfg.feature_hash_buckets,
            sp_cfg.feature_hash_seed,
        ) {
            Ok(model) => (Some(model), None),
            Err(err) => (None, Some(err)),
        },
    };
    let pre_cfg = build_preprocess_config(sp_cfg, validated_model.as_ref())?;
    let aish_session_dir = std::env::var("AISH_SESSION_DIR").ok().map(PathBuf::from);
    let local_history_summary = resolve_recent_local_history_summary(cfg, ai_session_id);
    let session_error = resolve_session_error_summary(aish_session_dir.as_deref());
    let outcome = evaluate_preprocessor(
        PreprocessorRunInput {
            query: query.to_string(),
            command: command.to_string(),
            tty: std::io::stdin().is_terminal(),
            new_conversation: hints.conversation_id.is_none() && turn.new,
            conversation_id: hints.conversation_id.clone(),
            memory_enabled: cfg.memory_enabled,
            history_tail_summary: local_history_summary,
            session_error_summary: session_error,
            cli_overrides: turn_has_cli_overrides(turn),
            route_metadata,
            history_id: history_id.clone(),
            model_load_error: model_load_error.clone(),
        },
        &pre_cfg,
        &cli_tool_allowlist_for_local_route(settings, turn),
    );
    Some(outcome)
}

#[allow(clippy::too_many_arguments)]
fn run_smart_route_with_preprocessor(
    cfg: &AiConfig,
    command: &str,
    socket_path: &Path,
    query: &str,
    settings: &ResolvedTurnSettings,
    turn: &TurnOptions,
    mut hints: RouteTurnHints,
    route_metadata: RouteMetadataInput,
    history_id: Option<String>,
) -> SmartRouteOutcome {
    let preprocess_started = Instant::now();
    let preprocessor = run_preprocessor_pipeline(
        cfg,
        settings,
        &settings.ai_session_id,
        command,
        query,
        turn,
        &hints,
        route_metadata,
        history_id.clone(),
    );
    let local_decision_latency_ms = preprocess_started.elapsed().as_millis() as u64;
    if let Some(ref outcome) = preprocessor {
        if outcome.decision.inject_hints {
            let rh = &outcome.decision.route_turn_hints;
            hints.recent_summary = rh.recent_summary.clone();
            hints.preprocessor_hints = preprocessor_wire_hints(rh);
        }
        let observation_history_id = outcome.history_id.clone().or_else(|| history_id.clone());
        let model_fallback =
            preprocessor_model_load_fallback_reason(outcome.model_load_error.as_ref());
        if outcome.use_local_route {
            let local = outcome.local_route.as_ref().expect("local route");
            let conversation_id = hints
                .conversation_id
                .clone()
                .or_else(|| Some(next_conversation_id()));
            let observation_draft = cfg.smart_preprocessor.as_ref().map(|sp_cfg| {
                build_preprocessor_observation_draft(
                    sp_cfg,
                    &outcome.decision,
                    &settings.ai_session_id,
                    conversation_id.clone(),
                    observation_history_id.clone(),
                    "local_route",
                    false,
                    model_fallback.clone(),
                    LocalRouteMetrics {
                        local_route_kind: Some(local.route_kind.as_str().to_string()),
                        local_route_used: true,
                        route_turn_skipped_count: 1,
                        route_turn_fallback_count: 0,
                        local_route_latency_ms: local_decision_latency_ms,
                        route_turn_latency_ms: 0,
                        estimated_tokens_saved: local.estimated_tokens_saved,
                    },
                    outcome.local_route.clone(),
                )
            });
            return SmartRouteOutcome {
                conversation_id,
                route_plan: None,
                route_fallback: false,
                local_route: outcome.local_route.clone(),
                observation_draft,
            };
        }
        if outcome.short_circuit {
            let conversation_id = hints
                .conversation_id
                .clone()
                .or_else(|| Some(next_conversation_id()));
            let observation_draft = cfg.smart_preprocessor.as_ref().map(|sp_cfg| {
                build_preprocessor_observation_draft(
                    sp_cfg,
                    &outcome.decision,
                    &settings.ai_session_id,
                    conversation_id.clone(),
                    observation_history_id.clone(),
                    "gate_short_circuit",
                    false,
                    model_fallback.clone(),
                    LocalRouteMetrics::default(),
                    None,
                )
            });
            return SmartRouteOutcome {
                conversation_id,
                route_plan: None,
                route_fallback: false,
                local_route: None,
                observation_draft,
            };
        }
    }
    let conversation_id_hint = hints.conversation_id.clone();
    let route_turn_started = Instant::now();
    let smart = run_smart_route(socket_path, query, settings, turn, hints);
    let route_turn_latency_ms = route_turn_started.elapsed().as_millis() as u64;
    let observation_draft = if let (Some(sp_cfg), Some(outcome)) =
        (cfg.smart_preprocessor.as_ref(), preprocessor.as_ref())
    {
        let observation_history_id = outcome.history_id.clone().or_else(|| history_id.clone());
        let model_fallback =
            preprocessor_model_load_fallback_reason(outcome.model_load_error.as_ref());
        let local_route_fallback = outcome
            .local_route
            .as_ref()
            .is_some_and(|local| local.fallback_required);
        let (decision_path, fallback_reason) = if smart.route_fallback {
            (
                "text_only_fallback",
                preprocessor_route_turn_fallback_reason(outcome.model_load_error.as_ref()),
            )
        } else {
            let path = match outcome.decision.mode {
                SmartPreprocessMode::Shadow => "shadow",
                SmartPreprocessMode::Assist => "assist",
                SmartPreprocessMode::Gate if local_route_fallback => "local_route_fallback",
                SmartPreprocessMode::Gate => "route_turn",
                SmartPreprocessMode::Off => "route_turn",
            };
            let reason = if local_route_fallback {
                outcome
                    .local_route
                    .as_ref()
                    .and_then(|local| local.fallback_reason.clone())
                    .or(model_fallback)
            } else {
                model_fallback
            };
            (path, reason)
        };
        Some(build_preprocessor_observation_draft(
            sp_cfg,
            &outcome.decision,
            &settings.ai_session_id,
            smart.conversation_id.clone().or(conversation_id_hint),
            observation_history_id,
            decision_path,
            !smart.route_fallback,
            fallback_reason,
            LocalRouteMetrics {
                local_route_kind: outcome
                    .local_route
                    .as_ref()
                    .map(|local| local.route_kind.as_str().to_string()),
                local_route_used: false,
                route_turn_skipped_count: 0,
                route_turn_fallback_count: u8::from(local_route_fallback),
                local_route_latency_ms: local_decision_latency_ms,
                route_turn_latency_ms,
                estimated_tokens_saved: 0,
            },
            outcome.local_route.clone(),
        ))
    } else {
        None
    };
    SmartRouteOutcome {
        conversation_id: smart.conversation_id,
        route_plan: smart.route_plan,
        route_fallback: smart.route_fallback,
        local_route: smart.local_route,
        observation_draft,
    }
}

fn run_smart_route(
    socket_path: &Path,
    query: &str,
    settings: &ResolvedTurnSettings,
    turn: &TurnOptions,
    hints: RouteTurnHints,
) -> SmartRouteOutcome {
    let request = build_route_turn_request(query, settings, turn, hints);
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
            local_route: None,
            observation_draft: None,
        }
    } else {
        SmartRouteOutcome {
            conversation_id: None,
            route_plan: None,
            route_fallback: true,
            local_route: None,
            observation_draft: None,
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

#[derive(Debug, Clone, Default)]
struct RouteTurnHints {
    conversation_id: Option<String>,
    recent_summary: Option<String>,
    preprocessor_hints: Option<RouteTurnPreprocessorHints>,
}

fn preprocessor_wire_hints(
    hints: &ai::domain::smart_preprocessor::SmartRouteTurnHints,
) -> Option<RouteTurnPreprocessorHints> {
    if hints.context_needs.is_empty()
        && hints.tool_hints.is_empty()
        && hints.failure_kind.is_none()
        && hints.preprocessor_intent.is_none()
        && hints.preprocessor_reason_codes.is_empty()
        && hints.confidence_bps.is_none()
        && hints.confidence_gate.is_none()
        && hints.safety_requires_approval.is_none()
    {
        return None;
    }
    Some(RouteTurnPreprocessorHints {
        context_needs: hints.context_needs.clone(),
        tool_hints: hints.tool_hints.clone(),
        failure_kind: hints.failure_kind.clone(),
        preprocessor_intent: hints.preprocessor_intent.clone(),
        preprocessor_reason_codes: hints.preprocessor_reason_codes.clone(),
        confidence_bps: hints.confidence_bps,
        confidence_gate: hints.confidence_gate.clone(),
        safety_requires_approval: hints.safety_requires_approval,
    })
}

fn route_turn_hints_from_payload(payload: &HistoryPayload) -> RouteTurnHints {
    RouteTurnHints {
        conversation_id: payload.conversation_id.clone(),
        recent_summary: None,
        preprocessor_hints: None,
    }
}

fn build_route_turn_request(
    query: &str,
    settings: &ResolvedTurnSettings,
    turn: &TurnOptions,
    hints: RouteTurnHints,
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
            conversation_id: hints.conversation_id.clone(),
            recent_summary: hints.recent_summary.clone(),
            new_conversation: hints.conversation_id.is_none() && turn.new,
            preprocessor_hints: hints.preprocessor_hints.clone(),
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
    let out = aibe_protocol::sanitize_readonly_advisory_tools(raw);
    (!out.is_empty()).then_some(out)
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
        aibe_protocol::ProgressPhase::Preparing => "preparing".to_string(),
        aibe_protocol::ProgressPhase::Routing => "routing".to_string(),
        aibe_protocol::ProgressPhase::Thinking => "thinking".to_string(),
        aibe_protocol::ProgressPhase::Generating => "generating".to_string(),
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
    let shell_log_mode = ShellLogMode::parse(cfg.shell_log_mode.as_deref());
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
        shell_log_mode,
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
        silent_exec: turn.silent_exec,
        shell_exec_approval,
        console_hint,
        trace_route: turn.trace_route,
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

fn require_memory_enabled() -> anyhow::Result<AiConfig> {
    #[cfg(not(feature = "memory"))]
    {
        return Err(anyhow::anyhow!(
            ai::application::memory_stub::MEMORY_FEATURE_DISABLED_MESSAGE
        ));
    }
    #[cfg(feature = "memory")]
    {
        let cfg = AiConfig::load();
        cfg.ensure_memory_enabled().map_err(anyhow::Error::msg)?;
        Ok(cfg)
    }
}

fn prepare_memory_context(
    options: &MemoryCliOptions,
    cfg: &AiConfig,
) -> anyhow::Result<MemoryCliContext> {
    let socket_path = options.socket.clone().unwrap_or(cfg.socket_path.clone());
    if !options.no_start {
        ensure_running(&socket_path).map_err(|e| anyhow::anyhow!(e))?;
    }
    let cwd = std::env::current_dir()?;
    let canonical_cwd = cwd
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("failed to canonicalize cwd: {e}"))?;
    let project_key =
        ai::adapters::outbound::project_key::canonical_project_key_from_cwd(&canonical_cwd)
            .map_err(anyhow::Error::msg)?;
    let session_id = resolve_ai_session_id();
    let env_context = std::env::var("AIBE_CONTEXT_ID").ok();
    let memory_context = ai::application::memory_space::build_memory_context(
        &session_id,
        &canonical_cwd,
        project_key.as_deref(),
        cfg.context_current.as_deref(),
        env_context.as_deref(),
    )
    .map_err(anyhow::Error::msg)?;
    Ok(MemoryCliContext {
        socket_path,
        session_id,
        memory_context,
        cwd,
        #[cfg(feature = "memory")]
        format: ai::application::memory_cli_context::to_plugin_format(options.format.into()),
        #[cfg(not(feature = "memory"))]
        format: options.format.into(),
    })
}

fn run_context(command: ContextCommand) -> anyhow::Result<ExitCode> {
    let cfg = require_memory_enabled()?;
    let cwd = std::env::current_dir()?;
    let canonical_cwd = cwd
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("failed to canonicalize cwd: {e}"))?;
    let project_key =
        ai::adapters::outbound::project_key::canonical_project_key_from_cwd(&canonical_cwd)
            .map_err(anyhow::Error::msg)?;
    let session_id = resolve_ai_session_id();
    match command {
        ContextCommand::Current => {
            let env_context = std::env::var("AIBE_CONTEXT_ID").ok();
            let resolution = resolve_memory_space_id(
                &session_id,
                project_key.as_deref(),
                cfg.context_current.as_deref(),
                env_context.as_deref(),
            )
            .map_err(anyhow::Error::msg)?;
            println!("{}", format_resolution(&resolution));
            println!("session_id (provenance): {session_id}");
        }
        ContextCommand::Use { name } => {
            AiConfig::save_context_current(&name).map_err(anyhow::Error::msg)?;
            println!("context set to: {name}");
            println!("note: AIBE_CONTEXT_ID environment variable overrides config");
        }
        ContextCommand::New { name } => {
            AiConfig::save_context_current(&name).map_err(anyhow::Error::msg)?;
            println!("context created and set to: {name}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn run_work(command: Option<WorkCommand>, options: WorkCliOptions) -> anyhow::Result<ExitCode> {
    let cfg = require_memory_enabled()?;
    let memory_options = MemoryCliOptions {
        socket: options.socket,
        format: OutputFormatArg::Tsv,
        no_start: options.no_start,
    };
    let ctx = prepare_memory_context(&memory_options, &cfg)?;
    let client = AibeUnixClient::new(&ctx.socket_path);
    let output = match command {
        None => ai::application::work_cli::run_work_query(
            &client,
            &ctx,
            ai::domain::WorkView::Dashboard,
        ),
        Some(WorkCommand::Status) => {
            ai::application::work_cli::run_work_query(&client, &ctx, ai::domain::WorkView::Status)
        }
        Some(WorkCommand::List) => {
            ai::application::work_cli::run_work_query(&client, &ctx, ai::domain::WorkView::List)
        }
        Some(command) => {
            ai::application::work_cli::run_work_apply(&client, &ctx, work_operation(command))
        }
    }
    .map_err(|error| anyhow::anyhow!(error))?;
    println!("{output}");
    Ok(ExitCode::SUCCESS)
}

fn work_operation(command: WorkCommand) -> WorkOperationDto {
    match command {
        WorkCommand::Start { goal } => WorkOperationDto::Start {
            goal: join_words(goal),
        },
        WorkCommand::Switch { work_id } => WorkOperationDto::Switch { work_id },
        WorkCommand::Push { goal } => WorkOperationDto::Push {
            goal: join_words(goal),
        },
        WorkCommand::Pop => WorkOperationDto::Pop,
        WorkCommand::Defer { text } => WorkOperationDto::Defer {
            text: join_words(text),
        },
        WorkCommand::Idea { text } => WorkOperationDto::AddEntry {
            kind: WorkEntryKindDto::Idea,
            text: join_words(text),
        },
        WorkCommand::Note { text } => WorkOperationDto::AddEntry {
            kind: WorkEntryKindDto::Note,
            text: join_words(text),
        },
        WorkCommand::Decide { text } => WorkOperationDto::AddEntry {
            kind: WorkEntryKindDto::Decision,
            text: join_words(text),
        },
        WorkCommand::Focus { text } => WorkOperationDto::Focus {
            text: join_words(text),
        },
        WorkCommand::Finish => WorkOperationDto::Finish,
        WorkCommand::Status | WorkCommand::List => unreachable!("query command handled separately"),
    }
}

fn run_memory_command(
    options: MemoryCliOptions,
    action: impl FnOnce(&MemoryCliPack<'_>) -> Result<String, AgentError>,
) -> anyhow::Result<ExitCode> {
    let cfg = require_memory_enabled()?;
    let ctx = prepare_memory_context(&options, &cfg)?;
    let client = AibeUnixClient::new(&ctx.socket_path);
    let policy = load_command_policy(&client, &ctx).map_err(|e| anyhow::anyhow!(e))?;
    let pack = MemoryCliPack::new(&client, &ctx, &policy);
    let out = action(&pack).map_err(|e| anyhow::anyhow!(e))?;
    println!("{out}");
    Ok(ExitCode::SUCCESS)
}

fn run_memory_command_without_policy(
    options: MemoryCliOptions,
    action: impl FnOnce(&dyn MemoryClient, &MemoryCliContext) -> Result<String, AgentError>,
) -> anyhow::Result<ExitCode> {
    let cfg = require_memory_enabled()?;
    let ctx = prepare_memory_context(&options, &cfg)?;
    let client = AibeUnixClient::new(&ctx.socket_path);
    let out = action(&client, &ctx).map_err(|e| anyhow::anyhow!(e))?;
    println!("{out}");
    Ok(ExitCode::SUCCESS)
}

fn run_goal(command: GoalCommand) -> anyhow::Result<ExitCode> {
    match command {
        GoalCommand::Set { text, options } => run_memory_command(options, |pack| {
            let joined = join_words(text);
            memory_cli::run_dedicated_set(pack, "goal", &joined, &format!("goal set: {joined}"))
        }),
        GoalCommand::Show { options } => {
            run_memory_command(options, |pack| memory_cli::run_dedicated_show(pack, "goal"))
        }
        GoalCommand::Clear { options } => run_memory_command(options, |pack| {
            memory_cli::run_dedicated_clear(pack, "goal", "goal cleared")
        }),
    }
}

fn run_now(command: NowCommand) -> anyhow::Result<ExitCode> {
    match command {
        NowCommand::Set { text, options } => run_memory_command(options, |pack| {
            let joined = join_words(text);
            memory_cli::run_dedicated_set(pack, "now", &joined, &format!("now set: {joined}"))
        }),
        NowCommand::Show { options } => {
            run_memory_command(options, |pack| memory_cli::run_dedicated_show(pack, "now"))
        }
        NowCommand::Clear { options } => run_memory_command(options, |pack| {
            memory_cli::run_dedicated_clear(pack, "now", "now cleared")
        }),
    }
}

fn run_idea(command: IdeaCommand) -> anyhow::Result<ExitCode> {
    match command {
        IdeaCommand::Add { text, options } => run_memory_command(options, |pack| {
            let joined = join_words(text);
            memory_cli::run_dedicated_set(pack, "idea", &joined, &format!("idea added: {joined}"))
        }),
        IdeaCommand::List { options } => {
            run_memory_command(options, |pack| memory_cli::run_dedicated_list(pack, "idea"))
        }
        IdeaCommand::Clear { options } => run_memory_command(options, |pack| {
            memory_cli::run_dedicated_clear(pack, "idea", "idea cleared")
        }),
    }
}

fn run_mem(command: MemCommand) -> anyhow::Result<ExitCode> {
    match command {
        MemCommand::Add {
            kind,
            text,
            options,
        } => run_memory_command(options, |pack| {
            memory_cli::run_mem_add(pack, &kind, &join_words(text))
        }),
        MemCommand::List { kind, options } => {
            run_memory_command_without_policy(options, |client, ctx| {
                memory_cli::run_mem_list(client, ctx, kind.as_deref())
            })
        }
        MemCommand::Show { query, options } => {
            run_memory_command_without_policy(options, |client, ctx| {
                memory_cli::run_mem_show(client, ctx, query.as_deref())
            })
        }
        MemCommand::Clear { kind, options } => {
            run_memory_command(options, |pack| memory_cli::run_mem_clear(pack, &kind))
        }
        MemCommand::Kinds { options } => run_memory_command(options, |pack| {
            memory_cli::run_mem_kinds(pack.policy, pack.ctx.format)
        }),
        MemCommand::Run {
            recipe,
            apply,
            instruction,
            options,
        } => run_memory_command_without_policy(options, |client, ctx| {
            memory_cli::run_mem_recipe(client, ctx, &recipe, apply, instruction.as_deref(), || {
                if apply {
                    ai::adapters::outbound::prompt_memory_recipe_apply()
                } else {
                    false
                }
            })
        }),
    }
}

fn read_collaborative_status() -> anyhow::Result<Vec<ai::domain::CollaborativeHandoffReport>> {
    let store = FilesystemHandoffStore::new(FilesystemHandoffStore::default_root());
    ReadCollaborativeStatus::new(&store)
        .read()
        .map_err(Into::into)
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
    let collaborative_handoff = read_collaborative_status()?;
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
        collaborative_handoff,
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
        collaborative_handoff: Vec::new(),
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
    invocation: AskInvocationSource,
) -> anyhow::Result<ResolveAskMessageOutcome> {
    if file.is_some() && !message_parts.is_empty() {
        anyhow::bail!("--file cannot be combined with message text");
    }

    if let Some(path) = file {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;
        return Ok(finalize_message_content(
            format!("file:{}", path.display()),
            content,
        ));
    }

    if message_parts.iter().any(|part| part == "-") {
        if message_parts.len() != 1 {
            anyhow::bail!("`-` must be used alone to read stdin");
        }
        return Ok(finalize_message_content(
            "stdin".to_string(),
            read_all_stdin()?,
        ));
    }

    if !message_parts.is_empty() {
        return Ok(finalize_message_content(
            "argv".to_string(),
            message_parts.join(" "),
        ));
    }

    if !std::io::stdin().is_terminal() {
        return Ok(finalize_message_content(
            "pipe".to_string(),
            read_all_stdin()?,
        ));
    }

    if let Some(route) = plan_interactive_prompt_route(
        invocation,
        std::io::stdin().is_terminal(),
        resolve_editor_command_from_env(),
    ) {
        let result = match route {
            InteractivePromptRoute::ExternalEditor(command) => {
                let (_file, path) = create_prompt_temp_file(None)?;
                acquire_prompt_via_external_editor(&command, &path)
            }
            InteractivePromptRoute::BuiltinEditor => acquire_prompt_via_reedline()?,
        };
        return Ok(prompt_acquisition_to_outcome(result));
    }

    anyhow::bail!("missing message");
}

fn finalize_message_content(source: String, content: String) -> ResolveAskMessageOutcome {
    if ai::domain::is_substantive_prompt(&content) {
        ResolveAskMessageOutcome::Ready(ResolvedMessage { source, content })
    } else {
        ResolveAskMessageOutcome::Cancelled {
            message: "AISH: prompt is empty; cancelled.".to_string(),
        }
    }
}

#[derive(Debug)]
enum ResolveAskMessageOutcome {
    Ready(ResolvedMessage),
    Cancelled { message: String },
    EditorFailed { exit_code: Option<i32> },
}

fn prompt_acquisition_to_outcome(result: PromptAcquisitionResult) -> ResolveAskMessageOutcome {
    match result {
        PromptAcquisitionResult::Submitted { content } => {
            ResolveAskMessageOutcome::Ready(ResolvedMessage {
                source: "prompt".to_string(),
                content,
            })
        }
        PromptAcquisitionResult::Empty => ResolveAskMessageOutcome::Cancelled {
            message: "AISH: prompt is empty; cancelled.".to_string(),
        },
        PromptAcquisitionResult::Cancelled => ResolveAskMessageOutcome::Cancelled {
            message: "AISH: cancelled.".to_string(),
        },
        PromptAcquisitionResult::EditorFailed { exit_code } => {
            ResolveAskMessageOutcome::EditorFailed { exit_code }
        }
    }
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

    use crate::ResolvedTurnSettings;
    use ai::adapters::outbound::toml_config::{AiConfig, AiPresetConfig};
    use ai::adapters::outbound::LocalHistoryStore;
    use ai::application::ShellLogMode;
    use ai::clap_cli::{AiCli, TurnOptions};

    use ai::domain::{
        resolve_console_hints, resolve_progress, ConfigToolsTokens, FilterMetadata, ShellLogChoice,
    };
    use ai::domain::{
        HistoryMessage, HistoryPayload, HistoryRecordKind, HistoryRecordStatus, HistorySummary,
    };
    use ai::ports::outbound::HistoryStore;
    use aibe_client::default_socket_path;
    use aibe_protocol::{
        ClientProvidedToolSpec, ClientRequest, ClientResponse, ErrorCode, RouteKind, RoutePlan,
        ToolRiskClass,
    };

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
    fn request_with_client_tools_uses_client_tool_stream() {
        let request = ClientRequest::AgentTurn {
            id: "turn".into(),
            messages: vec![],
            tools: vec![],
            client_tools: vec![ClientProvidedToolSpec {
                name: "aish.replay_show".into(),
                description: "Show replay output".into(),
                parameters: serde_json::json!({"type": "object"}),
                risk_class: ToolRiskClass::ReadOnly,
                max_output_bytes: 1024,
            }],
            context: Default::default(),
            llm_profile: None,
        };

        assert!(crate::should_use_client_tool_stream(&request));
    }

    #[test]
    fn request_without_client_tools_uses_normal_stream() {
        let request = ClientRequest::AgentTurn {
            id: "turn".into(),
            messages: vec![],
            tools: vec![],
            client_tools: vec![],
            context: Default::default(),
            llm_profile: None,
        };

        assert!(!crate::should_use_client_tool_stream(&request));
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
            feature_summaries: vec![],
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
            route_fallback: false,
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
            feature_summaries: vec![],
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
            route_fallback: false,
            socket_path: "/tmp/s".into(),
            log_tail_bytes: 1,
        };
        let messages = crate::replay_messages_from_payload(&payload);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "hello");
    }

    #[test]
    fn payload_eligible_for_smart_rerun_only_ask() {
        let ask = HistoryPayload {
            history_id: "id".into(),
            command: "ask".into(),
            user_message: "q".into(),
            request_messages: vec![],
            feature_summaries: vec![],
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
            route_fallback: false,
            socket_path: "/tmp/s".into(),
            log_tail_bytes: 1,
        };
        let chat = HistoryPayload {
            command: "chat".into(),
            ..ask.clone()
        };
        assert!(crate::payload_eligible_for_smart_rerun(&ask));
        assert!(!crate::payload_eligible_for_smart_rerun(&chat));
    }

    #[test]
    fn route_metadata_from_payload_restores_prior_fallback() {
        let payload = HistoryPayload {
            history_id: "id".into(),
            command: "ask".into(),
            user_message: "hello".into(),
            request_messages: vec![],
            feature_summaries: vec![],
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
            route_fallback: true,
            socket_path: "/tmp/s".into(),
            log_tail_bytes: 1,
        };
        let meta = crate::route_metadata_from_payload(Some(&payload));
        assert!(meta.prior_route_fallback);
    }

    #[test]
    fn build_route_turn_request_pins_conversation_for_retry() {
        let settings = crate::ResolvedTurnSettings {
            quiet: true,
            output_format: None,
            preset_name: None,
            log_tail_bytes: 64,
            socket_path: PathBuf::from("/tmp/aibe.sock"),
            session_id: None,
            ai_session_id: "session-retry".into(),
            shell_log_choice: ShellLogChoice::None,
            shell_log_mode: ShellLogMode::Hybrid,
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
            yes_exec: false,
            silent_exec: false,
            shell_exec_approval: None,
            console_hint: resolve_console_hints(None, None, None, true, None),
            trace_route: false,
        };
        let turn = crate::TurnOptions::default();
        let hints = crate::RouteTurnHints {
            conversation_id: Some("conv-original".into()),
            recent_summary: None,
            preprocessor_hints: None,
        };
        let request = crate::build_route_turn_request("retry me", &settings, &turn, hints);
        let ClientRequest::RouteTurn { conversation, .. } = request else {
            panic!("expected route_turn");
        };
        assert_eq!(
            conversation.conversation_id.as_deref(),
            Some("conv-original")
        );
        assert!(!conversation.new_conversation);
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
            shell_log_mode: ShellLogMode::Hybrid,
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
            silent_exec: false,
            shell_exec_approval: None,
            console_hint: resolve_console_hints(None, None, None, true, None),
            trace_route: false,
        };
        let turn = crate::TurnOptions {
            new: true,
            preset: Some("fast".into()),
            tools: Some("read_file,shell_exec".into()),
            log_tail: Some(128),
            yes_exec: true,
            ..Default::default()
        };

        let request = crate::build_route_turn_request(
            "hello",
            &settings,
            &turn,
            crate::RouteTurnHints::default(),
        );
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
            cli_overrides.tools.as_deref(),
            Some(&["read_file".to_string(), "shell_exec".to_string()][..])
        );
    }

    #[test]
    fn resolve_recent_local_history_summary_uses_matching_session() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = LocalHistoryStore::new(dir.path().to_path_buf());
        let entry = ai::domain::HistoryIndexEntry {
            history_id: "h1".into(),
            created_at_ms: 1,
            command: "ask".into(),
            session_id: Some("sess".into()),
            ai_session_id: Some("ai-session".into()),
            conversation_id: Some("conv".into()),
            preset: None,
            profile: None,
            shell_exec_approval: None,
            route_plan: None,
            socket_path: "/tmp/sock".into(),
            request_kind: HistoryRecordKind::Ask,
            request_summary: HistorySummary::new("request=1"),
            response_kind: HistoryRecordKind::Ask,
            response_summary: HistorySummary::new("response=1"),
            status: HistoryRecordStatus::Ok,
        };
        let payload = HistoryPayload {
            history_id: "h1".into(),
            command: "ask".into(),
            user_message: "hello".into(),
            request_messages: vec![],
            feature_summaries: vec![],
            shell_log_tail: None,
            client_cwd: None,
            tools: vec![],
            llm_profile: None,
            preset: None,
            session_id: Some("sess".into()),
            ai_session_id: Some("ai-session".into()),
            conversation_id: Some("conv".into()),
            shell_exec_approval: None,
            route_plan: None,
            route_fallback: false,
            socket_path: "/tmp/sock".into(),
            log_tail_bytes: 1,
        };
        store.append(&entry, &payload).expect("append");

        let cfg = ai::adapters::outbound::toml_config::AiConfig {
            socket_path: default_socket_path(),
            context_current: None,
            memory_enabled: true,
            ask_tools: ConfigToolsTokens::default(),
            ask_default_profile: None,
            ask_filter: None,
            ask_console_hints: None,
            ask_progress: None,
            shell_log_mode: None,
            history_dir: dir.path().to_path_buf(),
            history_max_entries: 1,
            log_tail_bytes: None,
            suggested_command_recall: true,
            suggested_command_recall_hint: true,
            suggested_command_recall_max_items: 8,
            presets: std::collections::HashMap::new(),
            smart_preprocessor: None,
        };
        let summary =
            crate::resolve_recent_local_history_summary(&cfg, "ai-session").expect("summary");
        assert!(summary.contains("request: request=1"));
        assert!(summary.contains("response: response=1"));
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
            context_current: None,
            memory_enabled: true,
            ask_tools: ConfigToolsTokens::default(),
            ask_default_profile: None,
            ask_filter: None,
            ask_console_hints: None,
            ask_progress: None,
            shell_log_mode: None,
            history_dir: PathBuf::from("/tmp/ai-history-test"),
            history_max_entries: 500,
            log_tail_bytes: None,
            suggested_command_recall: true,
            suggested_command_recall_hint: true,
            suggested_command_recall_max_items: 8,
            presets: HashMap::new(),
            smart_preprocessor: None,
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
            context_current: None,
            memory_enabled: true,
            ask_tools: ConfigToolsTokens::default(),
            ask_default_profile: None,
            ask_filter: None,
            ask_console_hints: None,
            ask_progress: None,
            shell_log_mode: None,
            history_dir: PathBuf::from("/tmp/ai-history-test"),
            history_max_entries: 500,
            log_tail_bytes: None,
            suggested_command_recall: true,
            suggested_command_recall_hint: true,
            suggested_command_recall_max_items: 8,
            presets: HashMap::from([("fast".into(), AiPresetConfig::default())]),
            smart_preprocessor: None,
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
            feature_actions: vec![],
            confidence: None,
        };
        let turn = crate::apply_route_plan_advisory(TurnOptions::default(), &plan, &cfg, true);
        assert!(turn.preset.is_none());
        assert_eq!(turn.tools.as_deref(), Some("read_file"));
    }

    #[test]
    fn local_route_enabled_tools_are_clamped_to_cli_allowlist() {
        use ai::domain::smart_preprocessor::{
            clamp_local_tools_to_allowlist, project_safe_local_tools, LocalToolHint, SmartToolHint,
        };

        let projected = project_safe_local_tools(&[
            SmartToolHint::GitStatus,
            SmartToolHint::GitDiff,
            SmartToolHint::Grep,
        ]);
        let clamped = clamp_local_tools_to_allowlist(projected, &["git_status".into()]);
        assert_eq!(clamped, vec![LocalToolHint::GitStatus]);

        let turn = TurnOptions {
            tools: Some("git_status".into()),
            ..Default::default()
        };
        let applied =
            crate::apply_local_route_tools(&turn, &[LocalToolHint::GitDiff, LocalToolHint::Grep]);
        assert_eq!(applied.tools.as_deref(), Some("git_status"));
    }

    #[test]
    fn apply_local_route_wires_context_and_output_style_messages() {
        use std::collections::HashMap;

        use ai::domain::smart_preprocessor::{
            build_local_route_context_summary, local_output_style_system_hint, LocalOutputStyle,
            LocalRouteDecision, LocalRouteKind, LocalToolHint, SmartContextNeed, SmartIntentClass,
        };

        let local = LocalRouteDecision {
            route_kind: LocalRouteKind::VcsInspect,
            enabled_tools: vec![LocalToolHint::GitDiff],
            context_needs: vec![SmartContextNeed::VcsDiff],
            output_style: LocalOutputStyle::Concise,
            fallback_required: false,
            fallback_reason: None,
            source_intent: SmartIntentClass::Inspect,
            confidence_bps: 9000,
            estimated_tokens_saved: 800,
        };
        assert!(build_local_route_context_summary(&local.context_needs).is_some());
        assert!(local_output_style_system_hint(local.output_style).is_some());

        let cfg = AiConfig {
            socket_path: default_socket_path(),
            context_current: None,
            memory_enabled: true,
            ask_tools: ConfigToolsTokens::default(),
            ask_default_profile: None,
            ask_filter: None,
            ask_console_hints: None,
            ask_progress: None,
            shell_log_mode: None,
            history_dir: PathBuf::from("/tmp/ai-history-test"),
            history_max_entries: 500,
            log_tail_bytes: None,
            suggested_command_recall: true,
            suggested_command_recall_hint: true,
            suggested_command_recall_max_items: 8,
            presets: HashMap::new(),
            smart_preprocessor: None,
        };
        let settings = crate::ResolvedTurnSettings {
            quiet: true,
            output_format: None,
            preset_name: None,
            log_tail_bytes: 1,
            socket_path: default_socket_path(),
            session_id: None,
            ai_session_id: "sess".into(),
            shell_log_choice: ShellLogChoice::None,
            shell_log_mode: ShellLogMode::Hybrid,
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
            yes_exec: false,
            silent_exec: false,
            shell_exec_approval: None,
            console_hint: resolve_console_hints(None, None, None, true, None),
            trace_route: false,
        };
        let prep = crate::apply_smart_route_and_features(
            &cfg,
            "git diff を見て",
            TurnOptions::default(),
            &settings,
            crate::SmartRouteOutcome {
                conversation_id: Some("conv-1".into()),
                route_plan: None,
                route_fallback: false,
                local_route: Some(local),
                observation_draft: None,
            },
        );
        assert_eq!(prep.agent_messages.len(), 3);
        assert!(prep
            .agent_messages
            .iter()
            .any(|msg| msg.role == "system" && msg.content.contains("local_context_needs")));
        assert!(prep
            .agent_messages
            .iter()
            .any(|msg| msg.role == "system" && msg.content.contains("concise")));
    }

    #[test]
    fn work_context_is_not_added_to_client_system_instruction() {
        use ai::domain::smart_preprocessor::{
            LocalOutputStyle, LocalRouteDecision, LocalRouteKind, LocalToolHint, SmartContextNeed,
            SmartIntentClass,
        };

        let local = LocalRouteDecision {
            route_kind: LocalRouteKind::VcsInspect,
            enabled_tools: vec![LocalToolHint::GitDiff],
            context_needs: vec![SmartContextNeed::VcsDiff],
            output_style: LocalOutputStyle::Concise,
            fallback_required: false,
            fallback_reason: None,
            source_intent: SmartIntentClass::Inspect,
            confidence_bps: 9000,
            estimated_tokens_saved: 800,
        };
        let cfg = AiConfig {
            socket_path: default_socket_path(),
            context_current: None,
            memory_enabled: true,
            ask_tools: ConfigToolsTokens::default(),
            ask_default_profile: None,
            ask_filter: None,
            ask_console_hints: None,
            ask_progress: None,
            shell_log_mode: None,
            history_dir: PathBuf::from("/tmp/ai-history-test"),
            history_max_entries: 500,
            log_tail_bytes: None,
            suggested_command_recall: true,
            suggested_command_recall_hint: true,
            suggested_command_recall_max_items: 8,
            presets: std::collections::HashMap::new(),
            smart_preprocessor: None,
        };
        let settings = ResolvedTurnSettings {
            quiet: true,
            output_format: None,
            preset_name: None,
            log_tail_bytes: 1,
            socket_path: default_socket_path(),
            session_id: None,
            ai_session_id: "sess".into(),
            shell_log_choice: ShellLogChoice::None,
            shell_log_mode: ShellLogMode::Hybrid,
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
            yes_exec: false,
            silent_exec: false,
            shell_exec_approval: None,
            console_hint: resolve_console_hints(None, None, None, true, None),
            trace_route: false,
        };
        let prep = crate::apply_smart_route_and_features(
            &cfg,
            "git diff を見て",
            TurnOptions::default(),
            &settings,
            crate::SmartRouteOutcome {
                conversation_id: Some("conv-1".into()),
                route_plan: None,
                route_fallback: false,
                local_route: Some(local),
                observation_draft: None,
            },
        );
        assert_eq!(prep.agent_messages.len(), 3);
        assert!(prep
            .agent_messages
            .iter()
            .any(|msg| msg.role == "system" && msg.content.contains("local_context_needs")));
        assert!(prep
            .agent_messages
            .iter()
            .any(|msg| msg.role == "system" && msg.content.contains("concise")));
        assert!(prep
            .agent_messages
            .iter()
            .all(|msg| !msg.content.contains("[active work]")));
    }
}
