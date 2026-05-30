//! 同一 Unix 接続上での `shell_exec` 承認往復。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::io::{AsyncWriteExt, BufReader, Lines};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::Mutex;

use aibe_protocol::{ClientRequest, ClientResponse};

use crate::ports::outbound::ShellExecApprovalGate;

static PROMPT_SEQ: AtomicU64 = AtomicU64::new(0);

pub struct ConnectionApprovalGate {
    turn_id: String,
    writer: Arc<Mutex<OwnedWriteHalf>>,
    lines: Arc<Mutex<Lines<BufReader<OwnedReadHalf>>>>,
}

impl ConnectionApprovalGate {
    pub fn new(
        turn_id: String,
        writer: Arc<Mutex<OwnedWriteHalf>>,
        lines: Arc<Mutex<Lines<BufReader<OwnedReadHalf>>>>,
    ) -> Self {
        Self {
            turn_id,
            writer,
            lines,
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
    ) -> bool {
        let seq = PROMPT_SEQ.fetch_add(1, Ordering::Relaxed);
        let prompt_id = format!("shell-exec-approval-{seq}");
        let prompt = ClientResponse::ShellExecApprovalPrompt {
            id: prompt_id.clone(),
            turn_id: self.turn_id.clone(),
            tool_call_id: tool_call_id.to_string(),
            command: command.to_string(),
            args: args.to_vec(),
        };
        if write_response(&self.writer, &prompt).await.is_err() {
            return false;
        }

        let line = {
            let mut lines = self.lines.lock().await;
            match lines.next_line().await {
                Ok(Some(l)) => l,
                _ => return false,
            }
        };

        let Ok(ClientRequest::ShellExecApproval {
            id,
            turn_id,
            tool_call_id: tc_id,
            approved,
            ..
        }) = serde_json::from_str::<ClientRequest>(line.trim())
        else {
            return false;
        };

        id == prompt_id && turn_id == self.turn_id && tc_id == tool_call_id && approved
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
