//! 配線: 標準アダプタで UseCase を組み立てる

use std::sync::Arc;

use common::adapter::{StdClock, StdFileSystem, StdProcess};
use common::part_id::StdIdGenerator;

use crate::usecase::app::AiUseCase;

/// 配線: 標準アダプタで AiUseCase を組み立てる
pub fn wire_ai() -> AiUseCase {
    let fs = Arc::new(StdFileSystem);
    let id_gen = Arc::new(StdIdGenerator::new(Arc::new(StdClock)));
    let process = Arc::new(StdProcess);
    AiUseCase::new(fs, id_gen, process)
}
