//! memory CLI pack facade（feature on は plugin へ委譲）。

#[cfg(feature = "memory")]
pub use crate::plugin_memory::memory_cli_pack::*;

#[cfg(not(feature = "memory"))]
mod stub {
    use crate::application::memory_cli_context::MemoryCliContext;
    use crate::ports::outbound::{AgentError, MemoryClient};

    use super::super::memory_stub::MEMORY_FEATURE_DISABLED_MESSAGE;

    /// feature off 時の policy プレースホルダ。
    pub struct MemoryCommandPolicy;

    /// feature off 時の pack プレースホルダ。
    pub struct MemoryCliPack<'a> {
        pub client: &'a dyn MemoryClient,
        pub ctx: &'a MemoryCliContext,
        pub policy: &'a MemoryCommandPolicy,
    }

    impl<'a> MemoryCliPack<'a> {
        pub fn new(
            client: &'a dyn MemoryClient,
            ctx: &'a MemoryCliContext,
            policy: &'a MemoryCommandPolicy,
        ) -> Self {
            Self {
                client,
                ctx,
                policy,
            }
        }
    }

    pub fn load_command_policy(
        _client: &dyn MemoryClient,
        _ctx: &MemoryCliContext,
    ) -> Result<MemoryCommandPolicy, AgentError> {
        Err(AgentError::Request(
            MEMORY_FEATURE_DISABLED_MESSAGE.to_string(),
        ))
    }
}

#[cfg(not(feature = "memory"))]
pub use stub::*;
