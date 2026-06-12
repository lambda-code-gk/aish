//! memory space 解決 port（env / cwd I/O は adapter 実装）。

use std::path::Path;

use aibe_protocol::MemoryContext;

use super::{ContextualMemoryStoreError, MemoryStoreContext};

pub trait MemorySpaceResolver: Send + Sync {
    fn resolve_store_context<'a>(
        &self,
        session_id: &'a str,
        context: &MemoryContext,
        cwd_path: Option<&'a Path>,
    ) -> Result<MemoryStoreContext<'a>, ContextualMemoryStoreError>;

    /// turn 注入用の解決。explicit id（request 由来）を最優先し、cwd 無し・
    /// project key 解決失敗時は legacy session space へフォールバックする（best-effort）。
    fn resolve_for_turn<'a>(
        &self,
        session_id: &'a str,
        explicit_memory_space_id: Option<&str>,
        cwd_path: Option<&'a Path>,
    ) -> Result<MemoryStoreContext<'a>, ContextualMemoryStoreError>;
}
