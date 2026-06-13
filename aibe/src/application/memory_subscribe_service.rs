//! MemorySubscribe RPC と subscribe 専用接続 push。

use std::path::Path;
use std::sync::Arc;

use aibe_protocol::{
    is_valid_session_id, ClientResponse, ErrorCode, MemorySubscribeRequestBody,
    MemorySubscribeStatus,
};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use crate::domain::MemorySubscriptionFilter;
use crate::ports::outbound::{MemorySpaceResolver, MemorySubscription, MemorySubscriptionBroker};

pub struct MemorySubscribeService {
    broker: Arc<dyn MemorySubscriptionBroker>,
    resolver: Arc<dyn MemorySpaceResolver>,
}

impl MemorySubscribeService {
    pub fn new(
        broker: Arc<dyn MemorySubscriptionBroker>,
        resolver: Arc<dyn MemorySpaceResolver>,
    ) -> Self {
        Self { broker, resolver }
    }

    pub fn begin(
        &self,
        body: MemorySubscribeRequestBody,
    ) -> (ClientResponse, Option<MemorySubscription>) {
        if let Err(msg) = validate_session_id(&body.session_id) {
            return (invalid(body.id, msg), None);
        }
        let cwd_path = body.context.cwd.as_deref().map(Path::new);
        let store_ctx =
            match self
                .resolver
                .resolve_store_context(&body.session_id, &body.context, cwd_path)
            {
                Ok(ctx) => ctx,
                Err(e) => return (invalid(body.id, &e.to_string()), None),
            };
        let filter = MemorySubscriptionFilter {
            kind: body.kind.clone(),
        };
        let subscription = self
            .broker
            .subscribe(store_ctx.memory_space_id.clone(), filter);
        (
            ClientResponse::MemorySubscribeResult {
                id: body.id.clone(),
                status: MemorySubscribeStatus::Ok,
                memory_space_id: store_ctx.memory_space_id,
            },
            Some(subscription),
        )
    }

    pub async fn push_until_disconnect(
        subscribe_id: String,
        memory_space_id: String,
        mut subscription: MemorySubscription,
        writer: Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
        lines: Arc<Mutex<crate::ports::inbound::SubscribeConnectionLines>>,
    ) -> anyhow::Result<()> {
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
                            if Self::write_response_line(&writer, &response).await.is_err() {
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
                            let _ = Self::write_response_line(&writer, &response).await;
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

    pub async fn write_response_line(
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
}

fn validate_session_id(session_id: &str) -> Result<(), &'static str> {
    if is_valid_session_id(session_id) {
        Ok(())
    } else {
        Err("invalid session_id")
    }
}

fn invalid(id: String, message: &str) -> ClientResponse {
    ClientResponse::error(id, ErrorCode::InvalidRequest, message)
}
