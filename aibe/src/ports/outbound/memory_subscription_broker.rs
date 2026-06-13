//! memory 変更通知 broker port。

use crate::domain::{MemoryChangeEvent, MemorySubscriptionFilter};

/// 1 件の購読。drop または `close` で broker から解除される。
pub struct MemorySubscription {
    receiver: tokio::sync::mpsc::UnboundedReceiver<MemoryChangeEvent>,
    _guard: SubscriptionGuard,
}

struct SubscriptionGuard {
    unregister: Box<dyn FnOnce() + Send>,
}

impl MemorySubscription {
    pub(crate) fn new(
        receiver: tokio::sync::mpsc::UnboundedReceiver<MemoryChangeEvent>,
        unregister: impl FnOnce() + Send + 'static,
    ) -> Self {
        Self {
            receiver,
            _guard: SubscriptionGuard {
                unregister: Box::new(unregister),
            },
        }
    }

    pub async fn recv(&mut self) -> Option<MemoryChangeEvent> {
        self.receiver.recv().await
    }
}

impl Drop for SubscriptionGuard {
    fn drop(&mut self) {
        let unregister = std::mem::replace(&mut self.unregister, Box::new(|| {}));
        unregister();
    }
}

pub trait MemorySubscriptionBroker: Send + Sync {
    fn publish(&self, memory_space_id: &str, event: MemoryChangeEvent);
    fn subscribe(
        &self,
        memory_space_id: String,
        filter: MemorySubscriptionFilter,
    ) -> MemorySubscription;
}
