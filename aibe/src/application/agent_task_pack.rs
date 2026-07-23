use std::sync::Arc;

use crate::ports::outbound::AgentTaskWorkerRegistry;

struct EmptyAgentTaskWorkerRegistry;

impl AgentTaskWorkerRegistry for EmptyAgentTaskWorkerRegistry {
    fn get(
        &self,
        _id: &crate::domain::WorkerId,
    ) -> Option<Arc<dyn crate::ports::outbound::AgentTaskWorker>> {
        None
    }

    fn timeout_limit_secs(&self, _id: &crate::domain::WorkerId) -> Option<u64> {
        None
    }

    fn permission_profile(&self, _id: &crate::domain::WorkerId) -> Option<&str> {
        None
    }

    fn is_empty(&self) -> bool {
        true
    }
}

pub trait AgentTaskPack: Send + Sync {
    fn registry(&self) -> Arc<dyn AgentTaskWorkerRegistry>;
    fn publishes_tool(&self) -> bool;
}

pub struct ActiveAgentTaskPack {
    registry: Arc<dyn AgentTaskWorkerRegistry>,
}

impl ActiveAgentTaskPack {
    pub fn new(registry: Arc<dyn AgentTaskWorkerRegistry>) -> Self {
        Self { registry }
    }
}

impl AgentTaskPack for ActiveAgentTaskPack {
    fn registry(&self) -> Arc<dyn AgentTaskWorkerRegistry> {
        Arc::clone(&self.registry)
    }

    fn publishes_tool(&self) -> bool {
        !self.registry.is_empty()
    }
}

pub struct BasicAgentTaskPack {
    registry: Arc<dyn AgentTaskWorkerRegistry>,
}

impl Default for BasicAgentTaskPack {
    fn default() -> Self {
        Self {
            registry: Arc::new(EmptyAgentTaskWorkerRegistry),
        }
    }
}

impl AgentTaskPack for BasicAgentTaskPack {
    fn registry(&self) -> Arc<dyn AgentTaskWorkerRegistry> {
        Arc::clone(&self.registry)
    }

    fn publishes_tool(&self) -> bool {
        false
    }
}
