//! memory command policy facade（feature on は plugin へ委譲）。

#[cfg(feature = "memory")]
pub use crate::plugin_memory::memory_command_policy::*;

#[cfg(not(feature = "memory"))]
pub use super::memory_cli_pack::MemoryCommandPolicy;
