//! Unix NDJSON 等の駆動アダプタが呼ぶリクエスト処理 port。

use std::sync::Arc;

use aibe_protocol::{ClientRequest, ClientResponse, MemorySubscribeRequestBody};
use async_trait::async_trait;
use tokio::io::BufReader;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::Mutex;

use crate::ports::inbound::ShutdownCoordinator;
use crate::ports::outbound::{
    ClientToolGate, HumanTaskGate, ShellExecApprovalGate, ToolApprovalGate, TurnCancellation,
    TurnEventSink,
};

pub type SubscribeConnectionLines = tokio::io::Lines<BufReader<OwnedReadHalf>>;

#[async_trait]
pub trait ClientRequestHandler: Send + Sync {
    #[allow(clippy::too_many_arguments)]
    async fn handle_with_events(
        &self,
        request: ClientRequest,
        approval_gate: Option<Arc<dyn ShellExecApprovalGate>>,
        tool_approval_gate: Option<Arc<dyn ToolApprovalGate>>,
        client_tool_gate: Option<Arc<dyn ClientToolGate>>,
        human_task_gate: Option<Arc<dyn HumanTaskGate>>,
        events: Option<Arc<dyn TurnEventSink>>,
        cancellation: Option<Arc<TurnCancellation>>,
    ) -> ClientResponse;

    async fn handle_memory_subscribe(
        &self,
        body: MemorySubscribeRequestBody,
        writer: Arc<Mutex<OwnedWriteHalf>>,
        lines: Arc<Mutex<SubscribeConnectionLines>>,
        shutdown: Option<Arc<ShutdownCoordinator>>,
    ) -> anyhow::Result<()> {
        let _ = (body, writer, lines, shutdown);
        Err(anyhow::anyhow!("memory_subscribe is not supported"))
    }
}
