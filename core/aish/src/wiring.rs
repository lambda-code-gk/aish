//! 配線: 標準アダプタで UseCase を組み立てる（Unix 専用）

use std::sync::Arc;

use common::adapter::{FileSystem, StdClock, StdFileSystem, StdPathResolver};
use common::part_id::{IdGenerator, StdIdGenerator};

use crate::adapter::{StdShellRunner, UnixPtySpawn, UnixSignal};
use crate::usecase::app::AishUseCase;

/// 配線: 標準アダプタで AishUseCase を組み立てる（Unix 専用）
#[cfg(unix)]
pub fn wire_aish() -> AishUseCase {
    let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
    let id_gen: Arc<dyn IdGenerator> = Arc::new(StdIdGenerator::new(Arc::new(StdClock)));
    let path_resolver = Arc::new(StdPathResolver);
    let signal = Arc::new(UnixSignal);
    let pty_spawn = Arc::new(UnixPtySpawn);
    let shell_runner = Arc::new(StdShellRunner::new(
        Arc::clone(&fs),
        Arc::clone(&id_gen),
        signal.clone(),
        pty_spawn.clone(),
    ));
    AishUseCase::new(fs, path_resolver, signal, shell_runner)
}
