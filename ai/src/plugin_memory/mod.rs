//! contextual memory CLI plugin 実装（Phase D）。

pub mod api;
pub mod memory_cli;
pub mod memory_cli_pack;
pub mod memory_command_policy;

pub use crate::ports::outbound::{AgentError, MemoryClient};
pub use api::{append_env_line, MemoryCliContext, OutputFormat};
