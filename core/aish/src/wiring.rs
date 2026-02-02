//! 配線: 標準アダプタで UseCase を組み立てる（Unix 専用）

use std::sync::Arc;

use common::adapter::{StdClock, StdFileSystem};
use common::part_id::StdIdGenerator;

use crate::adapter::{UnixPtySpawn, UnixSignal};
use crate::usecase::app::AishUseCase;

/// 配線: 標準アダプタで AishUseCase を組み立てる（Unix 専用）
#[cfg(unix)]
pub fn wire_aish() -> AishUseCase {
    let fs = Arc::new(StdFileSystem);
    let id_gen = Arc::new(StdIdGenerator::new(Arc::new(StdClock)));
    let signal = Arc::new(UnixSignal);
    let pty_spawn = Arc::new(UnixPtySpawn);
    AishUseCase::new(fs, id_gen, signal, pty_spawn)
}
