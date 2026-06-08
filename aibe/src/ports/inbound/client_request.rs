//! Unix NDJSON 等の駆動アダプタが呼ぶリクエスト処理 port。

use std::sync::Arc;

use aibe_protocol::{ClientRequest, ClientResponse};
use async_trait::async_trait;

use crate::ports::outbound::{ShellExecApprovalGate, TurnCancellation, TurnEventSink};

#[async_trait]
pub trait ClientRequestHandler: Send + Sync {
    async fn handle_with_events(
        &self,
        request: ClientRequest,
        approval_gate: Option<Arc<dyn ShellExecApprovalGate>>,
        events: Option<Arc<dyn TurnEventSink>>,
        cancellation: Option<Arc<TurnCancellation>>,
    ) -> ClientResponse;
}
