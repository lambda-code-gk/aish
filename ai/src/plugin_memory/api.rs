//! memory CLI 共有型（plugin モジュール内）。

use std::path::PathBuf;

use aibe_protocol::MemoryContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Json,
    Tsv,
    Env,
}

pub struct MemoryCliContext {
    pub socket_path: PathBuf,
    pub session_id: String,
    pub memory_context: MemoryContext,
    pub cwd: PathBuf,
    pub format: OutputFormat,
}

pub use crate::ports::outbound::{AgentError, MemoryClient};

pub fn append_env_line(out: &mut String, key: &str, value: &str) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(key);
    out.push('=');
    let escaped = value.replace('\'', "'\\''");
    out.push('\'');
    out.push_str(&escaped);
    out.push('\'');
    out.push('\n');
}
