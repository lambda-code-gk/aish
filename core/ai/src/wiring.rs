//! 配線: 標準アダプタで UseCase を組み立てる

use std::sync::Arc;

use common::adapter::{FileSystem, Process, StdClock, StdEnvResolver, StdFileSystem, StdProcess};
use common::part_id::StdIdGenerator;
use common::tool::EchoTool;

use crate::adapter::{CliToolApproval, StdCommandAllowRulesLoader, StdEventSinkFactory, StdTaskRunner, ShellTool};
use crate::usecase::app::AiUseCase;

/// 配線: 標準アダプタで AiUseCase を組み立てる
pub fn wire_ai() -> AiUseCase {
    let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
    let id_gen = Arc::new(StdIdGenerator::new(Arc::new(StdClock)));
    let env_resolver = Arc::new(StdEnvResolver);
    let process: Arc<dyn Process> = Arc::new(StdProcess);
    let task_runner = Arc::new(StdTaskRunner::new(Arc::clone(&fs), Arc::clone(&process)));
    let command_allow_rules_loader = Arc::new(StdCommandAllowRulesLoader);
    let sink_factory = Arc::new(StdEventSinkFactory);
    let tools: Vec<Arc<dyn common::tool::Tool>> = vec![
        Arc::new(EchoTool::new()),
        Arc::new(ShellTool::new()),
    ];
    let approver: Arc<dyn crate::ports::outbound::ToolApproval> = Arc::new(CliToolApproval::new());
    AiUseCase::new(
        fs,
        id_gen,
        env_resolver,
        process,
        task_runner,
        command_allow_rules_loader,
        sink_factory,
        tools,
        approver,
    )
}
