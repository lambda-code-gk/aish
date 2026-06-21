//! graceful shutdown の協調 port。

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use tokio::sync::Notify;

#[derive(Debug)]
pub struct ShutdownCoordinator {
    notify: Notify,
    triggered: AtomicBool,
}

impl ShutdownCoordinator {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            notify: Notify::new(),
            triggered: AtomicBool::new(false),
        })
    }

    pub fn trigger(&self) {
        if self.triggered.swap(true, Ordering::SeqCst) {
            return;
        }
        self.notify.notify_waiters();
    }

    pub fn is_triggered(&self) -> bool {
        self.triggered.load(Ordering::SeqCst)
    }

    pub async fn wait(&self) {
        if self.is_triggered() {
            return;
        }
        self.notify.notified().await;
    }
}

impl Default for ShutdownCoordinator {
    fn default() -> Self {
        Self {
            notify: Notify::new(),
            triggered: AtomicBool::new(false),
        }
    }
}
