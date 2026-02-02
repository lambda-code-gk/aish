//! アダプター（外界の I/O を trait で抽象化）
//!
//! usecase はこのモジュールの trait 経由でのみファイル・時刻・プロセスに触れる。
//! 実装は標準実装（Std*）やテスト用のモックを注入する。

pub mod env_resolver;
pub mod fs;
pub mod clock;
pub mod path_resolver;
pub mod process;
pub mod std_env_resolver;
pub mod std_fs;
pub mod std_clock;
pub mod std_path_resolver;
pub mod std_process;

#[cfg(unix)]
pub mod pty;
#[cfg(unix)]
pub mod signal;

pub use env_resolver::EnvResolver;
pub use fs::{FileMetadata, FileSystem};
pub use clock::Clock;
pub use path_resolver::{PathResolver, PathResolverInput};
pub use process::Process;
pub use std_env_resolver::StdEnvResolver;
pub use std_fs::StdFileSystem;
pub use std_clock::StdClock;
pub use std_path_resolver::StdPathResolver;
pub use std_process::StdProcess;

#[cfg(unix)]
pub use pty::{Pty, PtyProcessStatus, PtySpawn, Winsize};
#[cfg(unix)]
pub use signal::Signal;
