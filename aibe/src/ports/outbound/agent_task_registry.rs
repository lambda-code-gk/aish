//! Agent Task Worker lookup port。

use std::sync::Arc;

use crate::domain::WorkerId;

use super::AgentTaskWorker;

pub trait AgentTaskWorkerRegistry: Send + Sync {
    fn get(&self, id: &WorkerId) -> Option<Arc<dyn AgentTaskWorker>>;
    fn timeout_limit_secs(&self, id: &WorkerId) -> Option<u64>;
    fn permission_profile(&self, id: &WorkerId) -> Option<&str>;
    fn is_empty(&self) -> bool;
}
