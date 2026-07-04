//! 同一 Unix 接続上での `shell_exec` / write-like tool 承認往復。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::{AsyncWriteExt, BufReader, Lines};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::Mutex;
use tokio::time::{timeout, Instant};

use aibe_client::ShellExecApprovalDecision;
use aibe_protocol::{ClientRequest, ClientResponse, ToolRiskClass};

use crate::ports::outbound::{
    ShellExecApprovalGate, ToolApprovalGate, ToolApprovalGateOutcome, ToolApprovalPromptRequest,
    TurnCancellation, TurnEventSink, FILE_WRITE_APPROVAL_TIMEOUT_MS,
};

static PROMPT_SEQ: AtomicU64 = AtomicU64::new(0);

pub struct ConnectionApprovalGate {
    turn_id: String,
    writer: Arc<Mutex<OwnedWriteHalf>>,
    lines: Arc<Mutex<Lines<BufReader<OwnedReadHalf>>>>,
    events: Option<Arc<dyn TurnEventSink>>,
    cancellation: Option<Arc<TurnCancellation>>,
}

impl ConnectionApprovalGate {
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
impl ShellExecApprovalGate for ConnectionApprovalGate {
    async fn request_shell_exec_approval(
        &self,
        tool_call_id: &str,
        command: &str,
        args: &[String],
    ) -> Option<ShellExecApprovalDecision> {
        let seq = PROMPT_SEQ.fetch_add(1, Ordering::Relaxed);
        let prompt_id = format!("shell-exec-approval-{seq}");
        let prompt = ClientResponse::ShellExecApprovalPrompt {
            id: prompt_id.clone(),
            turn_id: self.turn_id.clone(),
            tool_call_id: tool_call_id.to_string(),
            command: command.to_string(),
            args: args.to_vec(),
        };
        if let Some(events) = &self.events {
            events
                .progress(
                    &self.turn_id,
                    aibe_protocol::ProgressPhase::WaitingApproval,
                    Some(format!("shell_exec: {command}")),
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
            let Ok(ClientRequest::ShellExecApproval {
                id,
                turn_id,
                tool_call_id: tc_id,
                approved,
                approval_origin,
                ..
            }) = serde_json::from_str::<ClientRequest>(line.trim())
            else {
                return None;
            };

            if id == prompt_id && turn_id == self.turn_id && tc_id == tool_call_id {
                return Some(ShellExecApprovalDecision {
                    approved,
                    approval_origin,
                });
            }
            return None;
        }
    }
}

#[async_trait]
impl ToolApprovalGate for ConnectionApprovalGate {
    async fn request_tool_approval(
        &self,
        tool_call_id: &str,
        prompt: ToolApprovalPromptRequest,
    ) -> ToolApprovalGateOutcome {
        let seq = PROMPT_SEQ.fetch_add(1, Ordering::Relaxed);
        let prompt_id = format!("tool-approval-{seq}");
        let wire = ClientResponse::ToolApprovalPrompt {
            id: prompt_id.clone(),
            turn_id: self.turn_id.clone(),
            tool_call_id: tool_call_id.to_string(),
            tool_name: prompt.tool_name,
            risk_class: ToolRiskClass::WriteLike,
            summary: prompt.summary,
            paths: prompt.paths,
            preview: prompt.preview,
            preview_truncated: prompt.preview_truncated,
        };
        if let Some(events) = &self.events {
            events
                .progress(
                    &self.turn_id,
                    aibe_protocol::ProgressPhase::WaitingApproval,
                    Some("file write approval".into()),
                )
                .await;
        }
        if write_response(&self.writer, &wire).await.is_err() {
            return ToolApprovalGateOutcome::Unavailable;
        }

        let deadline = Instant::now() + Duration::from_millis(FILE_WRITE_APPROVAL_TIMEOUT_MS);
        loop {
            if Instant::now() >= deadline {
                return ToolApprovalGateOutcome::Timeout;
            }
            if let Some(cancel) = &self.cancellation {
                if cancel.is_cancelled() {
                    return ToolApprovalGateOutcome::Cancelled;
                }
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            let wait_for = remaining.min(Duration::from_millis(100));
            let line = {
                let mut lines = self.lines.lock().await;
                match timeout(wait_for, lines.next_line()).await {
                    Ok(Ok(Some(l))) => l,
                    Ok(Ok(None)) => return ToolApprovalGateOutcome::Unavailable,
                    Ok(Err(_)) => return ToolApprovalGateOutcome::Unavailable,
                    Err(_) => continue,
                }
            };
            let Ok(ClientRequest::ToolApproval {
                id,
                turn_id,
                tool_call_id: tc_id,
                approved,
                approval_origin,
            }) = serde_json::from_str::<ClientRequest>(line.trim())
            else {
                return ToolApprovalGateOutcome::Unavailable;
            };

            if id == prompt_id && turn_id == self.turn_id && tc_id == tool_call_id {
                return if approved {
                    ToolApprovalGateOutcome::Approved(approval_origin)
                } else if approval_origin == aibe_protocol::ToolApprovalOrigin::Unavailable {
                    ToolApprovalGateOutcome::Unavailable
                } else {
                    ToolApprovalGateOutcome::Denied(approval_origin)
                };
            }
            return ToolApprovalGateOutcome::Unavailable;
        }
    }
}

async fn write_response(
    writer: &Arc<Mutex<OwnedWriteHalf>>,
    response: &ClientResponse,
) -> anyhow::Result<()> {
    let out = serde_json::to_string(response)? + "\n";
    let mut w = writer.lock().await;
    w.write_all(out.as_bytes()).await?;
    w.flush().await?;
    Ok(())
}
