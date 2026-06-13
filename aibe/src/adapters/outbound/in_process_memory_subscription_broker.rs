//! 同一 process 内の memory 変更 broker。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::domain::{MemoryChangeEvent, MemorySubscriptionFilter};
use crate::ports::outbound::{MemorySubscription, MemorySubscriptionBroker};

struct Subscriber {
    memory_space_id: String,
    kind_filter: Option<String>,
    sender: tokio::sync::mpsc::UnboundedSender<MemoryChangeEvent>,
}

struct BrokerState {
    subscribers: Mutex<Vec<(u64, Subscriber)>>,
    next_id: AtomicU64,
}

pub struct InProcessMemorySubscriptionBroker {
    state: Arc<BrokerState>,
}

impl Default for InProcessMemorySubscriptionBroker {
    fn default() -> Self {
        Self::new()
    }
}

impl InProcessMemorySubscriptionBroker {
    pub fn new() -> Self {
        Self {
            state: Arc::new(BrokerState {
                subscribers: Mutex::new(Vec::new()),
                next_id: AtomicU64::new(1),
            }),
        }
    }

    /// 登録 subscriber 数（テスト・診断用）。
    pub fn subscriber_count(&self) -> usize {
        self.state.subscribers.lock().expect("lock").len()
    }
}

impl MemorySubscriptionBroker for InProcessMemorySubscriptionBroker {
    fn publish(&self, memory_space_id: &str, event: MemoryChangeEvent) {
        let guard = self.state.subscribers.lock().expect("lock");
        for (_, sub) in guard.iter() {
            if sub.memory_space_id != memory_space_id {
                continue;
            }
            if let Some(kind) = &sub.kind_filter {
                if kind != &event.kind {
                    continue;
                }
            }
            let _ = sub.sender.send(event.clone());
        }
    }

    fn subscribe(
        &self,
        memory_space_id: String,
        filter: MemorySubscriptionFilter,
    ) -> MemorySubscription {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let id = self.state.next_id.fetch_add(1, Ordering::Relaxed);
        self.state.subscribers.lock().expect("lock").push((
            id,
            Subscriber {
                memory_space_id,
                kind_filter: filter.kind,
                sender,
            },
        ));
        let state = Arc::clone(&self.state);
        MemorySubscription::new(receiver, move || {
            state
                .subscribers
                .lock()
                .expect("lock")
                .retain(|(sub_id, _)| *sub_id != id);
        })
    }
}
