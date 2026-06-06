//! agent_turn の progress / streaming / cancellation port。

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use async_trait::async_trait;
use tokio::sync::Notify;

use aibe_protocol::ProgressPhase;

#[derive(Debug)]
pub struct TurnCancellation {
    notify: Notify,
    cancelled: AtomicBool,
}

impl TurnCancellation {
    pub fn new() -> Self {
        Self {
            notify: Notify::new(),
            cancelled: AtomicBool::new(false),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub async fn wait(&self) {
        self.notify.notified().await;
    }
}

impl Default for TurnCancellation {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
pub trait TurnEventSink: Send + Sync {
    async fn progress(&self, id: &str, phase: ProgressPhase, message: Option<String>);
    async fn assistant_streaming(&self, id: &str, delta: String);
    async fn final_response(&self, id: &str);
}

pub type SharedTurnCancellation = Arc<TurnCancellation>;
