//! アダプター（外界の I/O の標準実装）
//!
//! trait は ports/outbound に定義。usecase は port（trait）にのみ依存し、
//! wiring がここから実装（Std* 等）を取得して注入する。

pub mod file_json_log;
pub mod std_clock;
pub mod std_env_resolver;
pub mod std_fs;
pub mod std_id_generator;
pub mod std_path_resolver;
pub mod std_process;

pub use file_json_log::{FileJsonLog, NoopLog};
pub use std_clock::StdClock;
pub use std_id_generator::StdIdGenerator;
pub use std_env_resolver::StdEnvResolver;
pub use std_fs::StdFileSystem;
pub use std_path_resolver::StdPathResolver;
pub use std_process::StdProcess;
