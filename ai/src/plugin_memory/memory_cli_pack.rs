//! memory CLI の command-policy 境界。

use aibe_protocol::ClientResponse;

use super::api::MemoryCliContext;
use super::api::{AgentError, MemoryClient};
use super::memory_command_policy::MemoryCommandPolicy;

/// `MemoryClient` + context + kind policy snapshot を束ねる command 単位の pack。
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

/// `memory_kind_list` を 1 回だけ呼び、policy snapshot を構築する。
pub fn load_command_policy(
    client: &dyn MemoryClient,
    ctx: &MemoryCliContext,
) -> Result<MemoryCommandPolicy, AgentError> {
    let response = client.memory_kind_list(&ctx.session_id, &ctx.memory_context)?;
    match response {
        ClientResponse::MemoryKindListResult { kinds, .. } => {
            Ok(MemoryCommandPolicy::from_kinds(kinds))
        }
        ClientResponse::Error { message, .. } => Err(AgentError::Request(message)),
        other => Err(AgentError::Request(format!(
            "unexpected response: {other:?}"
        ))),
    }
}
