//! contextual memory wire クライアント port。

use aibe_protocol::{ClientResponse, MemoryContext, MemoryOperationDto, MemoryQueryDto};

use super::AgentError;

pub trait MemoryClient: Send + Sync {
    fn memory_apply(
        &self,
        session_id: &str,
        context: &MemoryContext,
        operation: MemoryOperationDto,
    ) -> Result<ClientResponse, AgentError>;

    fn memory_query(
        &self,
        session_id: &str,
        context: &MemoryContext,
        query: MemoryQueryDto,
    ) -> Result<ClientResponse, AgentError>;

    fn memory_kind_list(
        &self,
        session_id: &str,
        context: &MemoryContext,
    ) -> Result<ClientResponse, AgentError>;
}
