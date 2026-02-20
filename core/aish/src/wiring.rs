//! 配線: 標準アダプタで UseCase を組み立てる（Unix 専用）

use std::sync::Arc;

use common::adapter::{
    FileJsonLog, NoopLog, StdClock, StdEnvResolver, StdFileSystem, StdPathResolver,
};
use common::part_id::{IdGenerator, StdIdGenerator};
use common::ports::outbound::{EnvResolver, FileSystem, Log, PathResolver, Signal};

use crate::adapter::{
    LoggingMemoryRepository, StdMemoryRepository, StdShellRunner, StdSysqRepository, UnixPtySpawn,
    UnixSignal,
};
use crate::ports::outbound::{MemoryRepository, ShellRunner, SysqRepository};
use crate::usecase::{
    ClearUseCase, InitUseCase, MemoryUseCase, RolloutUseCase, ResumeUseCase, SessionsUseCase,
    ShellUseCase, SysqUseCase, TruncateConsoleLogUseCase, MuteUseCase, UnmuteUseCase,
};

/// 配線で組み立てたポート群とユースケース（main の Command ディスパッチで利用）
#[cfg(unix)]
pub struct App {
    #[allow(dead_code)] // ユースケース構築に使用。main からは use_case 経由で利用
    pub path_resolver: Arc<dyn PathResolver>,
    #[allow(dead_code)]
    pub fs: Arc<dyn FileSystem>,
    #[allow(dead_code)]
    pub signal: Arc<dyn Signal>,
    #[allow(dead_code)]
    pub shell_runner: Arc<dyn ShellRunner>,
    #[allow(dead_code)] // sysq_use_case 構築に使用。main からは sysq_use_case 経由で利用
    pub sysq_repository: Arc<dyn SysqRepository>,
    pub sysq_use_case: SysqUseCase,
    pub memory_use_case: MemoryUseCase,
    pub shell_use_case: ShellUseCase,
    pub clear_use_case: ClearUseCase,
    pub truncate_console_log_use_case: TruncateConsoleLogUseCase,
    pub rollout_use_case: RolloutUseCase,
    pub mute_use_case: MuteUseCase,
    pub unmute_use_case: UnmuteUseCase,
    pub resume_use_case: ResumeUseCase,
    pub sessions_use_case: SessionsUseCase,
    pub init_use_case: InitUseCase,
    /// 構造化ログ（ファイルへ JSONL）。エラー時のコンソール表示とは別。main で lifecycle/error に利用予定。
    #[allow(dead_code)]
    pub logger: Arc<dyn Log>,
}

/// 配線: 標準アダプタで App を組み立てる（Unix 専用）
#[cfg(unix)]
pub fn wire_aish() -> App {
    let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
    let env_resolver: Arc<dyn EnvResolver> = Arc::new(StdEnvResolver);
    let logger: Arc<dyn Log> = env_resolver
        .resolve_log_file_path()
        .map(|path| Arc::new(FileJsonLog::new(Arc::clone(&fs), path)) as Arc<dyn Log>)
        .unwrap_or_else(|_| Arc::new(NoopLog));
    let id_gen: Arc<dyn IdGenerator> = Arc::new(StdIdGenerator::new(Arc::new(StdClock)));
    let path_resolver: Arc<dyn PathResolver> =
        Arc::new(StdPathResolver::new(Arc::clone(&env_resolver)));
    let signal: Arc<dyn Signal> = Arc::new(UnixSignal);
    let pty_spawn = Arc::new(UnixPtySpawn);
    let shell_runner: Arc<dyn ShellRunner> = Arc::new(StdShellRunner::new(
        Arc::clone(&env_resolver),
        Arc::clone(&fs),
        Arc::clone(&id_gen),
        Arc::clone(&signal) as Arc<dyn Signal>,
        pty_spawn,
    ));
    let sysq_repository: Arc<dyn SysqRepository> =
        Arc::new(StdSysqRepository::new(Arc::clone(&env_resolver), Arc::clone(&fs)));
    let sysq_use_case = SysqUseCase::new(Arc::clone(&sysq_repository));
    let memory_repository: Arc<dyn MemoryRepository> = Arc::new(LoggingMemoryRepository::new(
        Arc::new(StdMemoryRepository::new(Arc::clone(&env_resolver))),
        Arc::clone(&logger),
    ));
    let memory_use_case = MemoryUseCase::new(memory_repository);
    let shell_use_case = ShellUseCase::new(Arc::clone(&path_resolver), Arc::clone(&shell_runner));
    let clear_use_case = ClearUseCase::new(Arc::clone(&path_resolver), Arc::clone(&fs));
    let truncate_console_log_use_case = TruncateConsoleLogUseCase::new(
        Arc::clone(&path_resolver),
        Arc::clone(&fs),
        Arc::clone(&signal),
    );
    let rollout_use_case = RolloutUseCase::new(
        Arc::clone(&path_resolver),
        Arc::clone(&fs),
        Arc::clone(&signal),
    );
    let mute_use_case = MuteUseCase::new(
        Arc::clone(&path_resolver),
        Arc::clone(&fs),
        Arc::clone(&signal),
    );
    let unmute_use_case = UnmuteUseCase::new(
        Arc::clone(&path_resolver),
        Arc::clone(&fs),
    );
    let resume_use_case =
        ResumeUseCase::new(Arc::clone(&path_resolver), Arc::clone(&fs), Arc::clone(&shell_runner));
    let sessions_use_case = SessionsUseCase::new(
        Arc::clone(&path_resolver),
        Arc::clone(&fs),
        Arc::clone(&env_resolver),
    );
    let init_use_case = InitUseCase::new(Arc::clone(&env_resolver), Arc::clone(&fs));
    App {
        path_resolver,
        fs,
        signal,
        shell_runner,
        sysq_repository,
        sysq_use_case,
        memory_use_case,
        shell_use_case,
        clear_use_case,
        truncate_console_log_use_case,
        rollout_use_case,
        mute_use_case,
        unmute_use_case,
        resume_use_case,
        sessions_use_case,
        init_use_case,
        logger,
    }
}
