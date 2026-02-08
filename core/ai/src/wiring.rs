//! 配線: 標準アダプタで UseCase を組み立てる

use std::path::PathBuf;
use std::sync::Arc;

use common::adapter::{
    FileJsonLog, NoopLog, StdClock, StdEnvResolver, StdFileSystem, StdProcess,
};
use common::part_id::StdIdGenerator;
use common::ports::outbound::{EnvResolver, FileSystem, Log, Process};
use common::tool::EchoTool;

use crate::adapter::{
    CliContinuePrompt, CliToolApproval, FileAgentStateStorage, LeakscanPrepareSession,
    NoContinuePrompt, NoopInterruptChecker, NonInteractiveToolApproval, PartSessionStorage,
    ReviewedSessionStorage, SigintChecker, StdCommandAllowRulesLoader, StdEventSinkFactory,
    StdLlmEventStreamFactory, StdProfileLister, StdResolveProfileAndModel, StdResolveSystemInstruction,
    StdTaskRunner, ShellTool,
};
use crate::domain::Query;
use crate::ports::outbound::{
    AgentStateLoader, AgentStateSaver, PrepareSessionForSensitiveCheck, ResolveSystemInstruction,
    RunQuery, SessionHistoryLoader, SessionResponseSaver, TaskRunner,
};
use crate::usecase::app::AiUseCase;
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
    ) -> Result<i32, common::error::Error> {
        self.0.run_query(
            session_dir,
            provider,
            model,
            query,
            system_instruction,
            max_turns_override,
        )
    }
}

/// 配線で組み立てた use case 群（main の Command ディスパッチで利用）
pub struct App {
    pub env_resolver: Arc<dyn EnvResolver>,
    pub task_use_case: TaskUseCase,
    pub run_query: Arc<dyn RunQuery>,
    pub resolve_system_instruction: Arc<dyn ResolveSystemInstruction>,
    /// 構造化ログ（ファイルへ JSONL）。エラー時のコンソール表示とは別。
    pub logger: Arc<dyn Log>,
    /// テスト用に露出（Query 実行・session/history の単体テストで利用）
    #[cfg_attr(not(test), allow(dead_code))]
    pub ai_use_case: Arc<AiUseCase>,
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

/// 配線: 標準アダプタで AiUseCase / TaskUseCase を組み立て、App を返す。
///
/// `non_interactive`: true のとき確認プロンプトを出さない（ツール承認は常に拒否・続行はしない・leakscan ヒットは拒否）。CI 向け。
pub fn wire_ai(non_interactive: bool) -> App {
    let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
    let env_resolver: Arc<dyn EnvResolver> = Arc::new(StdEnvResolver);
    let logger: Arc<dyn Log> = env_resolver
        .resolve_log_file_path()
        .map(|path| Arc::new(FileJsonLog::new(Arc::clone(&fs), path)) as Arc<dyn Log>)
        .unwrap_or_else(|_| Arc::new(NoopLog));
    let id_gen = Arc::new(StdIdGenerator::new(Arc::new(StdClock)));
    let part_storage = Arc::new(PartSessionStorage::new(Arc::clone(&fs), id_gen));
    let interrupt_checker: Arc<dyn crate::ports::outbound::InterruptChecker> =
        SigintChecker::new()
            .map(Arc::new)
            .map(|a| a as Arc<dyn crate::ports::outbound::InterruptChecker>)
            .unwrap_or_else(|_| Arc::new(NoopInterruptChecker::new()));

    let (history_loader, response_saver, prepare_session_for_sensitive_check) =
        if let Some((leakscan_binary, rules_path)) = resolve_leakscan_paths(&fs, &env_resolver) {
            let leakscan_prepare: Arc<dyn PrepareSessionForSensitiveCheck> = Arc::new(
                LeakscanPrepareSession::new(
                    Arc::clone(&fs),
                    leakscan_binary,
                    rules_path,
                    Some(Arc::clone(&interrupt_checker)),
                    non_interactive,
                ),
            );
            let reviewed_storage = Arc::new(ReviewedSessionStorage::new(Arc::clone(&fs)));
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
    let process: Arc<dyn Process> = Arc::new(StdProcess);
    let task_runner: Arc<dyn TaskRunner> = Arc::new(StdTaskRunner::new(Arc::clone(&fs), Arc::clone(&process)));
    let command_allow_rules_loader = Arc::new(StdCommandAllowRulesLoader);
    let sink_factory = Arc::new(StdEventSinkFactory);
    let tools: Vec<Arc<dyn common::tool::Tool>> = vec![
        Arc::new(EchoTool::new()),
        Arc::new(ShellTool::new()),
    ];
    let approver: Arc<dyn crate::ports::outbound::ToolApproval> = if non_interactive {
        Arc::new(NonInteractiveToolApproval::new())
    } else {
        Arc::new(CliToolApproval::new(Some(Arc::clone(&interrupt_checker))))
    };
    let agent_state_storage = Arc::new(FileAgentStateStorage::new(Arc::clone(&fs)));
    let agent_state_saver: Arc<dyn AgentStateSaver> =
        Arc::clone(&agent_state_storage) as Arc<dyn AgentStateSaver>;
    let agent_state_loader: Arc<dyn AgentStateLoader> =
        Arc::clone(&agent_state_storage) as Arc<dyn AgentStateLoader>;
    let continue_prompt: Arc<dyn crate::ports::outbound::ContinueAfterLimitPrompt> =
        if non_interactive {
            Arc::new(NoContinuePrompt::new())
        } else {
            Arc::new(CliContinuePrompt::new())
        };
    let resolve_system_instruction: Arc<dyn ResolveSystemInstruction> =
        Arc::new(StdResolveSystemInstruction::new(Arc::clone(&env_resolver), Arc::clone(&fs)));
    let profile_lister: Arc<dyn crate::ports::outbound::ProfileLister> =
        Arc::new(StdProfileLister::new(Arc::clone(&fs), Arc::clone(&env_resolver)));
    let resolve_profile_and_model: Arc<dyn crate::ports::outbound::ResolveProfileAndModel> =
        Arc::new(StdResolveProfileAndModel::new(Arc::clone(&fs), Arc::clone(&env_resolver)));
    let llm_stream_factory: Arc<dyn crate::ports::outbound::LlmEventStreamFactory> =
        Arc::new(StdLlmEventStreamFactory::new(Arc::clone(&fs), Arc::clone(&env_resolver)));
    let ai_use_case = Arc::new(AiUseCase::new(
        fs,
        history_loader,
        response_saver,
        agent_state_saver,
        agent_state_loader,
        continue_prompt,
        Arc::clone(&env_resolver),
        process,
        command_allow_rules_loader,
        sink_factory,
        tools,
        approver,
        interrupt_checker,
        Arc::clone(&logger),
        profile_lister,
        resolve_profile_and_model,
        llm_stream_factory,
        prepare_session_for_sensitive_check,
    ));
    let run_query: Arc<dyn RunQuery> = Arc::new(AiRunQuery(Arc::clone(&ai_use_case)));
    let task_use_case = TaskUseCase::new(task_runner, Arc::clone(&run_query));
    App {
        env_resolver,
        task_use_case,
        run_query,
        resolve_system_instruction,
        logger,
        ai_use_case,
    }
}
