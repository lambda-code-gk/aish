//! Work CLI facade。

#[cfg(feature = "memory")]
pub use crate::plugin_memory::work_cli::*;

#[cfg(not(feature = "memory"))]
pub use super::memory_stub::{run_work_apply, run_work_query};
