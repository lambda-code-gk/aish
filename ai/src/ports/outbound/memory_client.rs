//! contextual memory wire クライアント port。

use aibe_protocol::{ClientResponse, MemoryOperationDto, MemoryQueryDto};

use super::AgentError;

pub trait MemoryClient: Send + Sync {
    fn memory_apply(
        &self,
        session_id: &str,
        cwd: &str,
        operation: MemoryOperationDto,
    ) -> Result<ClientResponse, AgentError>;

    fn memory_query(
        &self,
        session_id: &str,
        cwd: &str,
        query: MemoryQueryDto,
    ) -> Result<ClientResponse, AgentError>;
}
