use crate::ports::outbound::{HumanTaskGate, TurnCancellation, TurnEventSink};
use aibe_protocol::{ClientRequest, ClientResponse, HumanTaskRequest, HumanTaskResult};
use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncWriteExt, BufReader, Lines};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::Mutex;
use tokio::time::timeout;

static PROMPT_SEQ: AtomicU64 = AtomicU64::new(0);

pub struct ConnectionHumanTaskGate {
    turn_id: String,
    writer: Arc<Mutex<OwnedWriteHalf>>,
    lines: Arc<Mutex<Lines<BufReader<OwnedReadHalf>>>>,
    events: Option<Arc<dyn TurnEventSink>>,
    cancellation: Option<Arc<TurnCancellation>>,
}

impl ConnectionHumanTaskGate {
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
impl HumanTaskGate for ConnectionHumanTaskGate {
    async fn execute_human_task(
        &self,
        tool_call_id: &str,
        request: HumanTaskRequest,
    ) -> Option<HumanTaskResult> {
        let id = format!("human-task-{}", PROMPT_SEQ.fetch_add(1, Ordering::Relaxed));
        // shell_exec approval と同様、対話 UI 前に WaitingApproval を出して
        // クライアント側の tool_call スピナーを止める（Human Shell の TTY を奪わない）。
        if let Some(events) = &self.events {
            events
                .progress(
                    &self.turn_id,
                    aibe_protocol::ProgressPhase::WaitingApproval,
                    Some("human_task".into()),
                )
                .await;
        }
        let response = ClientResponse::HumanTaskExecutionRequest {
            id: id.clone(),
            turn_id: self.turn_id.clone(),
            tool_call_id: tool_call_id.into(),
            request,
        };
        let payload = serde_json::to_string(&response).ok()? + "\n";
        {
            let mut writer = self.writer.lock().await;
            writer.write_all(payload.as_bytes()).await.ok()?;
            writer.flush().await.ok()?;
        }
        loop {
            if self.cancellation.as_ref().is_some_and(|c| c.is_cancelled()) {
                return None;
            }
            let line = {
                let mut lines = self.lines.lock().await;
                match timeout(Duration::from_millis(100), lines.next_line()).await {
                    Ok(Ok(Some(line))) => line,
                    Ok(Ok(None)) | Ok(Err(_)) => return None,
                    Err(_) => continue,
                }
            };
            let ClientRequest::HumanTaskExecutionResult {
                id: response_id,
                turn_id,
                tool_call_id: response_call,
                result,
            } = serde_json::from_str(&line).ok()?
            else {
                return None;
            };
            if response_id == id && turn_id == self.turn_id && response_call == tool_call_id {
                return Some(result);
            }
            return None;
        }
    }
}
