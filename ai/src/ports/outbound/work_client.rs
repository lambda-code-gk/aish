//! Work RPC client port。

use aibe_protocol::{ClientResponse, MemoryContext, WorkOperationDto};

use super::AgentError;

pub trait WorkClient: Send + Sync {
    fn work_query(
        &self,
        session_id: &str,
        context: &MemoryContext,
    ) -> Result<ClientResponse, AgentError>;

    fn work_apply(
        &self,
        session_id: &str,
        context: &MemoryContext,
        operation: WorkOperationDto,
    ) -> Result<ClientResponse, AgentError>;
}
