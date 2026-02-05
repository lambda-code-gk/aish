//! 配線: 標準アダプタで UseCase を組み立てる（Unix 専用）

use std::sync::Arc;

use common::adapter::{EnvResolver, FileSystem, StdClock, StdEnvResolver, StdFileSystem, StdPathResolver};
use common::part_id::{IdGenerator, StdIdGenerator};
use common::ports::outbound::{PathResolver, Signal};

use crate::adapter::{StdShellRunner, StdSysqRepository, UnixPtySpawn, UnixSignal};
use crate::ports::outbound::{ShellRunner, SysqRepository};

/// 配線で組み立てたポート群（main の Command ディスパッチで利用）
#[cfg(unix)]
pub struct App {
    pub path_resolver: Arc<dyn PathResolver>,
    pub fs: Arc<dyn FileSystem>,
    pub signal: Arc<dyn Signal>,
    pub shell_runner: Arc<dyn ShellRunner>,
    pub sysq_repository: Arc<dyn SysqRepository>,
}

/// 配線: 標準アダプタで App を組み立てる（Unix 専用）
#[cfg(unix)]
pub fn wire_aish() -> App {
    let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
    let id_gen: Arc<dyn IdGenerator> = Arc::new(StdIdGenerator::new(Arc::new(StdClock)));
    let path_resolver: Arc<dyn PathResolver> = Arc::new(StdPathResolver);
    let signal: Arc<dyn Signal> = Arc::new(UnixSignal);
    let pty_spawn = Arc::new(UnixPtySpawn);
    let shell_runner: Arc<dyn ShellRunner> = Arc::new(StdShellRunner::new(
        Arc::clone(&fs),
        Arc::clone(&id_gen),
        Arc::clone(&signal) as Arc<dyn Signal>,
        pty_spawn,
    ));
    let env_resolver: Arc<dyn EnvResolver> = Arc::new(StdEnvResolver);
    let sysq_repository: Arc<dyn SysqRepository> =
        Arc::new(StdSysqRepository::new(Arc::clone(&env_resolver), Arc::clone(&fs)));
    App {
        path_resolver,
        fs,
        signal,
        shell_runner,
        sysq_repository,
    }
}
