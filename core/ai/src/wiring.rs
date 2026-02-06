//! 配線: 標準アダプタで UseCase を組み立てる

use std::sync::Arc;

use common::adapter::{FileSystem, Process, StdClock, StdEnvResolver, StdFileSystem, StdProcess};
use common::part_id::StdIdGenerator;
use common::ports::outbound::EnvResolver;
use common::tool::EchoTool;

use crate::adapter::{
    CliToolApproval, PartSessionStorage, StdCommandAllowRulesLoader, StdEventSinkFactory,
    StdResolveSystemInstruction, StdTaskRunner, ShellTool,
};
use crate::domain::Query;
use crate::ports::outbound::{
    ResolveSystemInstruction, RunQuery, SessionHistoryLoader, SessionResponseSaver, TaskRunner,
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
        query: &Query,
        system_instruction: Option<&str>,
    ) -> Result<i32, common::error::Error> {
        self.0.run_query(session_dir, provider, model, query, system_instruction)
    }
}

/// 配線で組み立てた use case 群（main の Command ディスパッチで利用）
pub struct App {
    pub env_resolver: Arc<dyn EnvResolver>,
    pub task_use_case: TaskUseCase,
    pub run_query: Arc<dyn RunQuery>,
    pub resolve_system_instruction: Arc<dyn ResolveSystemInstruction>,
    /// テスト用に露出（Query 実行・session/history の単体テストで利用）
    #[cfg_attr(not(test), allow(dead_code))]
    pub ai_use_case: Arc<AiUseCase>,
}

/// 配線: 標準アダプタで AiUseCase / TaskUseCase を組み立て、App を返す
pub fn wire_ai() -> App {
    let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
    let id_gen = Arc::new(StdIdGenerator::new(Arc::new(StdClock)));
    let part_storage = Arc::new(PartSessionStorage::new(Arc::clone(&fs), id_gen));
    let history_loader: Arc<dyn SessionHistoryLoader> =
        Arc::clone(&part_storage) as Arc<dyn SessionHistoryLoader>;
    let response_saver: Arc<dyn SessionResponseSaver> =
        Arc::clone(&part_storage) as Arc<dyn SessionResponseSaver>;
    let env_resolver: Arc<dyn EnvResolver> = Arc::new(StdEnvResolver);
    let process: Arc<dyn Process> = Arc::new(StdProcess);
    let task_runner: Arc<dyn TaskRunner> = Arc::new(StdTaskRunner::new(Arc::clone(&fs), Arc::clone(&process)));
    let command_allow_rules_loader = Arc::new(StdCommandAllowRulesLoader);
    let sink_factory = Arc::new(StdEventSinkFactory);
    let tools: Vec<Arc<dyn common::tool::Tool>> = vec![
        Arc::new(EchoTool::new()),
        Arc::new(ShellTool::new()),
    ];
    let approver: Arc<dyn crate::ports::outbound::ToolApproval> = Arc::new(CliToolApproval::new());
    let resolve_system_instruction: Arc<dyn ResolveSystemInstruction> =
        Arc::new(StdResolveSystemInstruction::new(Arc::clone(&env_resolver), Arc::clone(&fs)));
    let ai_use_case = Arc::new(AiUseCase::new(
        fs,
        history_loader,
        response_saver,
        Arc::clone(&env_resolver),
        process,
        command_allow_rules_loader,
        sink_factory,
        tools,
        approver,
    ));
    let run_query: Arc<dyn RunQuery> = Arc::new(AiRunQuery(Arc::clone(&ai_use_case)));
    let task_use_case = TaskUseCase::new(task_runner, Arc::clone(&run_query));
    App {
        env_resolver,
        task_use_case,
        run_query,
        resolve_system_instruction,
        ai_use_case,
    }
}
