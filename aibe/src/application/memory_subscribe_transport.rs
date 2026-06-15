//! memory subscribe 専用接続の transport ヘルパ（core）。

use std::sync::Arc;

use aibe_protocol::ClientResponse;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use crate::ports::inbound::SubscribeConnectionLines;
use crate::ports::outbound::MemorySubscription;

pub async fn write_subscribe_response_line(
    writer: &Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
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

pub async fn push_memory_subscription_until_disconnect(
    subscribe_id: String,
    memory_space_id: String,
    mut subscription: MemorySubscription,
    writer: Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
    lines: Arc<Mutex<SubscribeConnectionLines>>,
) -> anyhow::Result<()> {
    use aibe_protocol::ErrorCode;

    loop {
        tokio::select! {
            event = subscription.recv() => {
                match event {
                    Some(event) => {
                        let response = ClientResponse::MemoryChanged {
                            id: subscribe_id.clone(),
                            memory_space_id: memory_space_id.clone(),
                            event: event.to_dto(),
                        };
                        if write_subscribe_response_line(&writer, &response).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
            line = async {
                let mut guard = lines.lock().await;
                guard.next_line().await
            } => {
                match line {
                    Ok(Some(line)) if !line.trim().is_empty() => {
                        let response = ClientResponse::error(
                            subscribe_id.clone(),
                            ErrorCode::InvalidRequest,
                            "memory_subscribe connection does not accept other requests",
                        );
                        let _ = write_subscribe_response_line(&writer, &response).await;
                        break;
                    }
                    Ok(None) => break,
                    Ok(Some(_)) => {}
                    Err(e) => return Err(e.into()),
                }
            }
        }
    }
    Ok(())
}
