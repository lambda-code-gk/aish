//! memory CLI facade（feature on は plugin へ委譲）。

#[cfg(feature = "memory")]
pub use crate::plugin_memory::memory_cli::*;

#[cfg(not(feature = "memory"))]
pub use super::memory_stub::*;
