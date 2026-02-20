//! 配線: 標準アダプタで UseCase を組み立てる

use std::path::PathBuf;
use std::sync::Arc;

use common::adapter::{
    FileJsonLog, NoopLog, StdClock, StdEnvResolver, StdFileSystem, StdProcess,
};
use common::event_hub::EventHubHandle;
use common::part_id::StdIdGenerator;
use common::ports::outbound::{EnvResolver, FileSystem, Log, Process};
use common::tool::EchoTool;

use crate::adapter::{
    CliContinuePrompt, CliToolApproval, CompositeLifecycleHooks, DeterministicCompactionStrategy,
    FileAgentStateStorage, GetMemoryContentTool, GrepTool, LeakscanPrepareSession,
    ManifestReviewedSessionStorage, ManifestTailCompactionViewStrategy, NoContinuePrompt,
    NoopInterruptChecker, NonInteractiveToolApproval, PartSessionStorage, PassThroughReducer,
    ReadFileTool, ReplaceFileTool, ReviewedTailViewStrategy, SelfImproveHandler, SigintChecker,
    StdCommandAllowRulesLoader, StdContextMessageBuilder, StdEventSinkFactory, StdLlmCompletion,
    StdLlmEventStreamFactory, StdProfileLister, StdResolveMemoryDir,     StdResolveModeConfig, StdResolveProfileAndModel, StdResolveSystemInstruction, StdTaskRunner,
    ShellTool, TailWindowReducer, WriteFileTool,
    HistoryGetTool, HistorySearchTool, QueueShellSuggestionTool, SaveMemoryTool, SearchMemoryTool,
};
use crate::adapter::lifecycle::LifecycleHandler;
use crate::domain::{ContextBudget, Query};
use crate::ports::outbound::{
    AgentStateLoader, AgentStateSaver, ContextMessageBuilder, LifecycleHooks, LlmCompletion,
    PrepareSessionForSensitiveCheck, ResolveModeConfig, ResolveSystemInstruction, RunQuery,
    SessionHistoryLoader, SessionResponseSaver, TaskRunner,
};
use crate::usecase::app::{AiDeps, AiUseCase, ModelDeps, ObsDeps, PolicyDeps, SessionDeps, SystemDeps, ToolingDeps};
use crate::usecase::task::TaskUseCase;

/// Arc<AiUseCase> を RunQuery として渡すための薄いラッパ
struct AiRunQuery(Arc<AiUseCase>);
impl RunQuery for AiRunQuery {
    fn run_query(
        &self,
        session_dir: Option<common::domain::SessionDir>,
        provider: Option<common::domain::ProviderName>,
        model: Option<common::domain::ModelName>,
        query: Option<&Query>,
        system_instruction: Option<&str>,
        max_turns_override: Option<usize>,
        tool_allowlist: Option<&[String]>,
        event_hub: Option<EventHubHandle>,
    ) -> Result<i32, common::error::Error> {
        self.0.run_query(
            session_dir,
            provider,
            model,
            query,
            system_instruction,
            max_turns_override,
            tool_allowlist,
            event_hub,
        )
    }
}

/// 配線で組み立てた use case 群（main の Command ディスパッチで利用）
pub struct App {
    pub env_resolver: Arc<dyn EnvResolver>,
    pub fs: Arc<dyn FileSystem>,
    pub task_use_case: TaskUseCase,
    pub run_query: Arc<dyn RunQuery>,
    pub resolve_system_instruction: Arc<dyn ResolveSystemInstruction>,
    pub resolve_mode_config: Arc<dyn ResolveModeConfig>,
    /// 構造化ログ（ファイルへ JSONL）。エラー時のコンソール表示とは別。
    pub logger: Arc<dyn Log>,
    /// テスト用に露出（Query 実行・session/history の単体テストで利用）
    #[cfg_attr(not(test), allow(dead_code))]
    pub ai_use_case: Arc<AiUseCase>,
}


/// AISH_CONTEXT_STRATEGY / AISH_CONTEXT_MAX_MESSAGES / AISH_CONTEXT_MAX_CHARS から reducer と budget を決定する。
/// 未設定時は tail（TailWindow + 実用 budget）。legacy を指定すると従来の PassThrough + 大きめ budget。
fn context_strategy_from_env() -> (Arc<dyn crate::domain::HistoryReducer>, ContextBudget) {
    let strategy = std::env::var("AISH_CONTEXT_STRATEGY").unwrap_or_else(|_| "tail".into());
    let (reducer, default_budget) = match strategy.to_lowercase().as_str() {
        "legacy" => (
            Arc::new(PassThroughReducer) as Arc<dyn crate::domain::HistoryReducer>,
            ContextBudget::legacy(),
        ),
        _ => (
            Arc::new(TailWindowReducer) as Arc<dyn crate::domain::HistoryReducer>,
            ContextBudget::tail_default(),
        ),
    };
    let max_messages = std::env::var("AISH_CONTEXT_MAX_MESSAGES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default_budget.max_messages);
    let max_chars = std::env::var("AISH_CONTEXT_MAX_CHARS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default_budget.max_chars);
    let budget = ContextBudget {
        max_messages,
        max_chars,
    };
    (reducer, budget)
}

fn history_load_max_from_budget(budget: ContextBudget) -> usize {
    std::env::var("AISH_HISTORY_LOAD_MAX_MESSAGES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or_else(|| {
            let a = budget.max_messages.saturating_add(16);
            let b = budget.max_messages.saturating_mul(2);
            a.max(b)
        })
}

/// leakscan バイナリと rules のパスを解決する。両方存在すれば Some((binary, rules))、でなければ None。
///
/// rules.json の検索先: `$AISH_HOME/config/rules.json`（他の設定ファイルと同様 config/ 配下）。
/// leakscan バイナリの検索先: `$AISH_HOME/bin/leakscan` または `<ai バイナリの隣>/leakscan`。
fn resolve_leakscan_paths(
    fs: &Arc<dyn FileSystem>,
    env_resolver: &Arc<dyn EnvResolver>,
) -> Option<(PathBuf, PathBuf)> {
    let home = env_resolver.resolve_home_dir().ok()?;
    let rules = home.as_ref().join("config").join("rules.json");
    if !fs.exists(&rules) {
        return None;
    }
    // 1. $AISH_HOME/bin/leakscan
    let binary = home.as_ref().join("bin").join("leakscan");
    if fs.exists(&binary) {
        return Some((binary, rules));
    }
    // 2. カレント exe の隣に leakscan がある場合（開発時など）
    let current_exe = std::env::current_exe().ok()?;
    let bin_dir = current_exe.parent()?;
    let binary_alt = bin_dir.join("leakscan");
    if fs.exists(&binary_alt) {
        return Some((binary_alt, rules));
    }
    None
}

fn build_session_deps(
    fs: &Arc<dyn FileSystem>,
    env_resolver: &Arc<dyn EnvResolver>,
    interrupt_checker: &Arc<dyn crate::ports::outbound::InterruptChecker>,
    non_interactive: bool,
) -> SessionDeps {
    let id_gen = Arc::new(StdIdGenerator::new(Arc::new(StdClock)));
    let part_storage = Arc::new(PartSessionStorage::new(Arc::clone(fs), id_gen));
    let (reducer, budget) = context_strategy_from_env();
    let history_load_max = history_load_max_from_budget(budget);

    let (history_loader, response_saver, prepare_session_for_sensitive_check) =
        if let Some((leakscan_binary, rules_path)) = resolve_leakscan_paths(fs, env_resolver) {
            let leakscan_prepare: Arc<dyn PrepareSessionForSensitiveCheck> = Arc::new(
                LeakscanPrepareSession::new(
                    Arc::clone(fs),
                    leakscan_binary,
                    rules_path,
                    Some(Arc::clone(interrupt_checker)),
                    non_interactive,
                    Some(Arc::new(DeterministicCompactionStrategy)),
                ),
            );
            let reviewed_storage = Arc::new(ManifestReviewedSessionStorage::with_strategies(
                Arc::clone(fs),
                history_load_max,
                Arc::new(ManifestTailCompactionViewStrategy),
                Arc::new(ReviewedTailViewStrategy),
            ));
            let history: Arc<dyn SessionHistoryLoader> =
                Arc::clone(&reviewed_storage) as Arc<dyn SessionHistoryLoader>;
            // response_saver は part_* に書くので PartSessionStorage のまま
            let response_saver: Arc<dyn SessionResponseSaver> =
                Arc::clone(&part_storage) as Arc<dyn SessionResponseSaver>;
            (
                history,
                response_saver,
                Some(leakscan_prepare) as Option<Arc<dyn PrepareSessionForSensitiveCheck>>,
            )
        } else {
            let history_loader: Arc<dyn SessionHistoryLoader> =
                Arc::clone(&part_storage) as Arc<dyn SessionHistoryLoader>;
            let response_saver: Arc<dyn SessionResponseSaver> =
                Arc::clone(&part_storage) as Arc<dyn SessionResponseSaver>;
            (history_loader, response_saver, None)
        };

    let agent_state_storage = Arc::new(FileAgentStateStorage::new(Arc::clone(fs)));
    let agent_state_saver: Arc<dyn AgentStateSaver> =
        Arc::clone(&agent_state_storage) as Arc<dyn AgentStateSaver>;
    let agent_state_loader: Arc<dyn AgentStateLoader> =
        Arc::clone(&agent_state_storage) as Arc<dyn AgentStateLoader>;

    let context_message_builder: Arc<dyn ContextMessageBuilder> =
        Arc::new(StdContextMessageBuilder::new(reducer, budget));

    SessionDeps {
        fs: Arc::clone(fs),
        history_loader,
        context_message_builder,
        response_saver,
        agent_state_saver,
        agent_state_loader,
        prepare_session_for_sensitive_check,
    }
}

fn build_policy_deps(
    env_resolver: &Arc<dyn EnvResolver>,
    interrupt_checker: &Arc<dyn crate::ports::outbound::InterruptChecker>,
    non_interactive: bool,
) -> PolicyDeps {
    let command_allow_rules_loader = Arc::new(StdCommandAllowRulesLoader);
    let approver: Arc<dyn crate::ports::outbound::ToolApproval> = if non_interactive {
        Arc::new(NonInteractiveToolApproval::new())
    } else {
        Arc::new(CliToolApproval::new(Some(Arc::clone(interrupt_checker))))
    };
    let continue_prompt: Arc<dyn crate::ports::outbound::ContinueAfterLimitPrompt> =
        if non_interactive {
            Arc::new(NoContinuePrompt::new())
        } else {
            Arc::new(CliContinuePrompt::new())
        };

    PolicyDeps {
        continue_prompt,
        env_resolver: Arc::clone(env_resolver),
        resolve_memory_dir: Arc::new(StdResolveMemoryDir::new(Arc::clone(env_resolver))),
        command_allow_rules_loader,
        approver,
        interrupt_checker: Arc::clone(interrupt_checker),
    }
}

fn build_tooling_deps(verbose: bool) -> ToolingDeps {
    let sink_factory = Arc::new(StdEventSinkFactory::new(verbose));
    let tools: Vec<Arc<dyn common::tool::Tool>> = vec![
        Arc::new(EchoTool::new()),
        Arc::new(ShellTool::new()),
        Arc::new(QueueShellSuggestionTool::new()),
        Arc::new(ReadFileTool::new()),
        Arc::new(WriteFileTool::new()),
        Arc::new(ReplaceFileTool::new()),
        Arc::new(GrepTool::new()),
        Arc::new(HistoryGetTool::new()),
        Arc::new(HistorySearchTool::new()),
        Arc::new(SaveMemoryTool::new()),
        Arc::new(SearchMemoryTool::new()),
        Arc::new(GetMemoryContentTool::new()),
    ];

    ToolingDeps {
        sink_factory,
        tools,
    }
}

fn build_model_deps(
    fs: &Arc<dyn FileSystem>,
    env_resolver: &Arc<dyn EnvResolver>,
) -> ModelDeps {
    let profile_lister: Arc<dyn crate::ports::outbound::ProfileLister> =
        Arc::new(StdProfileLister::new(Arc::clone(fs), Arc::clone(env_resolver)));
    let resolve_profile_and_model: Arc<dyn crate::ports::outbound::ResolveProfileAndModel> =
        Arc::new(StdResolveProfileAndModel::new(Arc::clone(fs), Arc::clone(env_resolver)));
    let llm_stream_factory: Arc<dyn crate::ports::outbound::LlmEventStreamFactory> =
        Arc::new(StdLlmEventStreamFactory::new(Arc::clone(fs), Arc::clone(env_resolver)));

    ModelDeps {
        profile_lister,
        resolve_profile_and_model,
        llm_stream_factory,
    }
}

fn build_system_deps(process: &Arc<dyn Process>) -> SystemDeps {
    SystemDeps {
        process: Arc::clone(process),
    }
}

fn build_task_runner(
    fs: &Arc<dyn FileSystem>,
    process: &Arc<dyn Process>,
) -> Arc<dyn TaskRunner> {
    Arc::new(StdTaskRunner::new(Arc::clone(fs), Arc::clone(process)))
}

fn build_obs_deps(logger: &Arc<dyn Log>) -> ObsDeps {
    ObsDeps {
        log: Arc::clone(logger),
    }
}

/// ライフサイクルフックを組み立てる。AISH_SELF_IMPROVE=0 または false のときは自己改善ハンドラを登録しない。
fn build_lifecycle_hooks(
    llm_stream_factory: &Arc<dyn crate::ports::outbound::LlmEventStreamFactory>,
    logger: &Arc<dyn Log>,
) -> Arc<dyn LifecycleHooks> {
    let disabled = std::env::var("AISH_SELF_IMPROVE")
        .map(|v| v == "0" || v.eq_ignore_ascii_case("false"))
        .unwrap_or(false);
    let handlers: Vec<Arc<dyn LifecycleHandler>> = if disabled {
        vec![]
    } else {
        let llm_completion: Arc<dyn LlmCompletion> =
            Arc::new(StdLlmCompletion::new(Arc::clone(llm_stream_factory)));
        vec![Arc::new(SelfImproveHandler::new(
            llm_completion,
            Arc::clone(logger),
        ))]
    };
    Arc::new(CompositeLifecycleHooks::new(handlers))
}

fn build_resolve_system_instruction(
    env_resolver: &Arc<dyn EnvResolver>,
    fs: &Arc<dyn FileSystem>,
) -> Arc<dyn ResolveSystemInstruction> {
    Arc::new(StdResolveSystemInstruction::new(
        Arc::clone(env_resolver),
        Arc::clone(fs),
    ))
}

fn build_resolve_mode_config(
    env_resolver: &Arc<dyn EnvResolver>,
    fs: &Arc<dyn FileSystem>,
) -> Arc<dyn ResolveModeConfig> {
    Arc::new(StdResolveModeConfig::new(Arc::clone(env_resolver), Arc::clone(fs)))
}

/// 配線: 標準アダプタで AiUseCase / TaskUseCase を組み立て、App を返す。
///
/// `non_interactive`: true のとき確認プロンプトを出さない（ツール承認は常に拒否・続行はしない・leakscan ヒットは拒否）。CI 向け。
/// `verbose`: true のとき不具合調査用の冗長ログを stderr 等に出力する。
pub fn wire_ai(non_interactive: bool, verbose: bool) -> App {
    let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
    let env_resolver: Arc<dyn EnvResolver> = Arc::new(StdEnvResolver);
    let logger: Arc<dyn Log> = env_resolver
        .resolve_log_file_path()
        .map(|path| Arc::new(FileJsonLog::new(Arc::clone(&fs), path)) as Arc<dyn Log>)
        .unwrap_or_else(|_| Arc::new(NoopLog));
    let interrupt_checker: Arc<dyn crate::ports::outbound::InterruptChecker> =
        SigintChecker::new()
            .map(Arc::new)
            .map(|a| a as Arc<dyn crate::ports::outbound::InterruptChecker>)
            .unwrap_or_else(|_| Arc::new(NoopInterruptChecker::new()));
    let process: Arc<dyn Process> = Arc::new(StdProcess);
    let session = build_session_deps(&fs, &env_resolver, &interrupt_checker, non_interactive);
    let policy = build_policy_deps(&env_resolver, &interrupt_checker, non_interactive);
    let tooling = build_tooling_deps(verbose);
    let model = build_model_deps(&fs, &env_resolver);
    let system = build_system_deps(&process);
    let obs = build_obs_deps(&logger);
    let lifecycle_hooks = build_lifecycle_hooks(&model.llm_stream_factory, &logger);

    let ai_use_case = Arc::new(AiUseCase::new(AiDeps {
        session,
        policy,
        tooling,
        model,
        system,
        obs,
        lifecycle_hooks,
        non_interactive,
    }));
    let run_query: Arc<dyn RunQuery> = Arc::new(AiRunQuery(Arc::clone(&ai_use_case)));
    let task_runner: Arc<dyn TaskRunner> = build_task_runner(&fs, &process);
    let resolve_system_instruction = build_resolve_system_instruction(&env_resolver, &fs);
    let resolve_mode_config = build_resolve_mode_config(&env_resolver, &fs);
    let task_use_case = TaskUseCase::new(task_runner, Arc::clone(&run_query));
    App {
        env_resolver,
        fs,
        task_use_case,
        run_query,
        resolve_system_instruction,
        resolve_mode_config,
        logger,
        ai_use_case,
    }
}

#[cfg(test)]
mod tests {
    use super::context_strategy_from_env;
    use common::llm::provider::Message as LlmMessage;
    use std::env;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn test_context_strategy_tail_with_overrides() {
        let _guard = env_lock().lock().expect("lock poisoned");
        let old_strategy = env::var("AISH_CONTEXT_STRATEGY").ok();
        let old_max_messages = env::var("AISH_CONTEXT_MAX_MESSAGES").ok();
        let old_max_chars = env::var("AISH_CONTEXT_MAX_CHARS").ok();

        env::set_var("AISH_CONTEXT_STRATEGY", "tail");
        env::set_var("AISH_CONTEXT_MAX_MESSAGES", "2");
        env::set_var("AISH_CONTEXT_MAX_CHARS", "100");

        let (reducer, budget) = context_strategy_from_env();
        assert_eq!(budget.max_messages, 2);
        assert_eq!(budget.max_chars, 100);

        let messages = vec![
            LlmMessage::user("a"),
            LlmMessage::user("b"),
            LlmMessage::user("c"),
        ];
        let reduced = reducer.reduce(&messages, budget);
        assert_eq!(reduced.len(), 2);
        assert_eq!(reduced[0].content, "b");
        assert_eq!(reduced[1].content, "c");

        if let Some(v) = old_strategy {
            env::set_var("AISH_CONTEXT_STRATEGY", v);
        } else {
            env::remove_var("AISH_CONTEXT_STRATEGY");
        }
        if let Some(v) = old_max_messages {
            env::set_var("AISH_CONTEXT_MAX_MESSAGES", v);
        } else {
            env::remove_var("AISH_CONTEXT_MAX_MESSAGES");
        }
        if let Some(v) = old_max_chars {
            env::set_var("AISH_CONTEXT_MAX_CHARS", v);
        } else {
            env::remove_var("AISH_CONTEXT_MAX_CHARS");
        }
    }

    #[test]
    fn test_context_strategy_unknown_falls_back_to_tail() {
        let _guard = env_lock().lock().expect("lock poisoned");
        let old_strategy = env::var("AISH_CONTEXT_STRATEGY").ok();
        let old_max_messages = env::var("AISH_CONTEXT_MAX_MESSAGES").ok();
        let old_max_chars = env::var("AISH_CONTEXT_MAX_CHARS").ok();

        env::set_var("AISH_CONTEXT_STRATEGY", "unknown");
        env::remove_var("AISH_CONTEXT_MAX_MESSAGES");
        env::remove_var("AISH_CONTEXT_MAX_CHARS");

        let (reducer, budget) = context_strategy_from_env();
        assert_eq!(budget.max_messages, 40);
        assert_eq!(budget.max_chars, 40_000);

        let messages = vec![
            LlmMessage::user("a"),
            LlmMessage::user("b"),
            LlmMessage::user("c"),
        ];
        let reduced = reducer.reduce(&messages, budget);
        assert_eq!(reduced.len(), 3);

        if let Some(v) = old_strategy {
            env::set_var("AISH_CONTEXT_STRATEGY", v);
        } else {
            env::remove_var("AISH_CONTEXT_STRATEGY");
        }
        if let Some(v) = old_max_messages {
            env::set_var("AISH_CONTEXT_MAX_MESSAGES", v);
        } else {
            env::remove_var("AISH_CONTEXT_MAX_MESSAGES");
        }
        if let Some(v) = old_max_chars {
            env::set_var("AISH_CONTEXT_MAX_CHARS", v);
        } else {
            env::remove_var("AISH_CONTEXT_MAX_CHARS");
        }
    }

    #[test]
    fn test_context_strategy_legacy_uses_legacy_budget() {
        let _guard = env_lock().lock().expect("lock poisoned");
        let old_strategy = env::var("AISH_CONTEXT_STRATEGY").ok();
        let old_max_messages = env::var("AISH_CONTEXT_MAX_MESSAGES").ok();
        let old_max_chars = env::var("AISH_CONTEXT_MAX_CHARS").ok();

        env::set_var("AISH_CONTEXT_STRATEGY", "legacy");
        env::remove_var("AISH_CONTEXT_MAX_MESSAGES");
        env::remove_var("AISH_CONTEXT_MAX_CHARS");

        let (reducer, budget) = context_strategy_from_env();
        assert_eq!(budget.max_messages, 10_000);
        assert_eq!(budget.max_chars, 10_000_000);

        let messages = vec![
            LlmMessage::user("a"),
            LlmMessage::user("b"),
            LlmMessage::user("c"),
        ];
        let reduced = reducer.reduce(&messages, budget);
        assert_eq!(reduced.len(), 3);

        if let Some(v) = old_strategy {
            env::set_var("AISH_CONTEXT_STRATEGY", v);
        } else {
            env::remove_var("AISH_CONTEXT_STRATEGY");
        }
        if let Some(v) = old_max_messages {
            env::set_var("AISH_CONTEXT_MAX_MESSAGES", v);
        } else {
            env::remove_var("AISH_CONTEXT_MAX_MESSAGES");
        }
        if let Some(v) = old_max_chars {
            env::set_var("AISH_CONTEXT_MAX_CHARS", v);
        } else {
            env::remove_var("AISH_CONTEXT_MAX_CHARS");
        }
    }
}
