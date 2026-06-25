//! 同一 Unix 接続上での client-provided tool 往復。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::{AsyncWriteExt, BufReader, Lines};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::Mutex;
use tokio::time::timeout;

use aibe_protocol::{ClientRequest, ClientResponse, ClientToolResult};

use crate::ports::outbound::{ClientToolGate, TurnCancellation, TurnEventSink};

static CALL_SEQ: AtomicU64 = AtomicU64::new(0);

pub struct ConnectionClientToolGate {
    turn_id: String,
    writer: Arc<Mutex<OwnedWriteHalf>>,
    lines: Arc<Mutex<Lines<BufReader<OwnedReadHalf>>>>,
    events: Option<Arc<dyn TurnEventSink>>,
    cancellation: Option<Arc<TurnCancellation>>,
}

impl ConnectionClientToolGate {
    pub fn new(
        turn_id: String,
        writer: Arc<Mutex<OwnedWriteHalf>>,
        lines: Arc<Mutex<Lines<BufReader<OwnedReadHalf>>>>,
        events: Option<Arc<dyn TurnEventSink>>,
        cancellation: Option<Arc<TurnCancellation>>,
    ) -> Self {
        Self {
            turn_id,
            writer,
            lines,
            events,
            cancellation,
        }
    }
}

#[async_trait]
impl ClientToolGate for ConnectionClientToolGate {
    async fn request_client_tool(
        &self,
        call_id: &str,
        name: &str,
        arguments: &serde_json::Value,
    ) -> Option<ClientToolResult> {
        let seq = CALL_SEQ.fetch_add(1, Ordering::Relaxed);
        let request_id = format!("client-tool-{seq}");
        let prompt = ClientResponse::ClientToolCallRequested {
            id: request_id.clone(),
            turn_id: self.turn_id.clone(),
            call_id: call_id.to_string(),
            name: name.to_string(),
            arguments: arguments.clone(),
        };
        if let Some(events) = &self.events {
            events
                .progress(
                    &self.turn_id,
                    aibe_protocol::ProgressPhase::ToolCall,
                    Some(format!("client_tool: {name}")),
                )
                .await;
        }
        if write_response(&self.writer, &prompt).await.is_err() {
            return None;
        }

        loop {
            if let Some(cancel) = &self.cancellation {
                if cancel.is_cancelled() {
                    return None;
                }
            }
            let line = {
                let mut lines = self.lines.lock().await;
                match timeout(Duration::from_millis(100), lines.next_line()).await {
                    Ok(Ok(Some(l))) => l,
                    Ok(Ok(None)) => return None,
                    Ok(Err(_)) => return None,
                    Err(_) => continue,
                }
            };
            let Ok(ClientRequest::ClientToolResult(result)) =
                serde_json::from_str::<ClientRequest>(line.trim())
            else {
                return None;
            };

            if result.id == request_id
                && result.turn_id == self.turn_id
                && result.call_id == call_id
            {
                return Some(result);
            }
            return None;
        }
    }
}

async fn write_response(
    writer: &Arc<Mutex<OwnedWriteHalf>>,
    response: &ClientResponse,
) -> anyhow::Result<()> {
    use std::io::ErrorKind;

    let out = serde_json::to_string(response)? + "\n";
    let mut w = writer.lock().await;
    if let Err(e) = w.write_all(out.as_bytes()).await {
        if e.kind() == ErrorKind::BrokenPipe {
            return Ok(());
        }
        return Err(e.into());
    }
    if let Err(e) = w.flush().await {
        if e.kind() == ErrorKind::BrokenPipe {
            return Ok(());
        }
        return Err(e.into());
    }
    Ok(())
}
