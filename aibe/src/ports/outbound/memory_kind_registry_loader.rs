//! effective MemoryKindRegistry の読み込み port。

use crate::domain::{MemoryKindRegistry, MemoryKindRegistryError};

pub trait MemoryKindRegistryLoader: Send + Sync {
    /// explicit memory RPC 用。parse 失敗時は error。
    fn load_strict(
        &self,
        memory_space_id: &str,
    ) -> Result<MemoryKindRegistry, MemoryKindRegistryError>;

    /// AgentTurn の prompt 解決用。失敗時は built-in にフォールバックし警告ログを残す。
    fn load_best_effort(&self, memory_space_id: &str) -> MemoryKindRegistry;
}
