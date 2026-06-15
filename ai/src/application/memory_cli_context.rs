//! memory CLI 共有コンテキスト。

#[cfg(feature = "memory")]
pub use crate::plugin_memory::{MemoryCliContext, OutputFormat};

#[cfg(not(feature = "memory"))]
use std::path::PathBuf;

#[cfg(not(feature = "memory"))]
use crate::domain::OutputFormat;

#[cfg(not(feature = "memory"))]
pub struct MemoryCliContext {
    pub socket_path: PathBuf,
    pub session_id: String,
    pub memory_context: aibe_protocol::MemoryContext,
    pub cwd: PathBuf,
    pub format: OutputFormat,
}

#[cfg(feature = "memory")]
pub fn to_plugin_format(format: crate::domain::OutputFormat) -> OutputFormat {
    match format {
        crate::domain::OutputFormat::Json => OutputFormat::Json,
        crate::domain::OutputFormat::Tsv => OutputFormat::Tsv,
        crate::domain::OutputFormat::Env => OutputFormat::Env,
    }
}
