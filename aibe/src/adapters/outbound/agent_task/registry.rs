use std::collections::HashMap;
use std::sync::Arc;

use thiserror::Error;

use crate::domain::WorkerId;
use crate::ports::outbound::{AgentTaskWorker, AgentTaskWorkerConfig, AgentTaskWorkerRegistry};

use super::ExternalCommandWorker;

struct Entry {
    worker: Arc<dyn AgentTaskWorker>,
    timeout_secs: u64,
    permission_profile: String,
}

pub struct DefaultAgentTaskWorkerRegistry {
    entries: HashMap<WorkerId, Entry>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AgentTaskRegistryBuildError {
    #[error("invalid worker id: {0}")]
    InvalidWorkerId(String),
    #[error("duplicate worker id: {0}")]
    DuplicateWorkerId(String),
}

impl DefaultAgentTaskWorkerRegistry {
    pub fn empty() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub fn from_configs(
        configs: &[AgentTaskWorkerConfig],
    ) -> Result<Self, AgentTaskRegistryBuildError> {
        let mut registry = Self::empty();
        for config in configs {
            let id = WorkerId::parse(config.id.clone())
                .map_err(|_| AgentTaskRegistryBuildError::InvalidWorkerId(config.id.clone()))?;
            let entry = Entry {
                worker: Arc::new(ExternalCommandWorker::new(config.clone())),
                timeout_secs: config.timeout_secs,
                permission_profile: config.permission_profile.clone(),
            };
            if registry.entries.insert(id, entry).is_some() {
                return Err(AgentTaskRegistryBuildError::DuplicateWorkerId(
                    config.id.clone(),
                ));
            }
        }
        Ok(registry)
    }

    pub fn from_workers(
        entries: Vec<(WorkerId, Arc<dyn AgentTaskWorker>, u64, String)>,
    ) -> Result<Self, AgentTaskRegistryBuildError> {
        let mut registry = Self::empty();
        for (id, worker, timeout_secs, permission_profile) in entries {
            let raw = id.as_str().to_string();
            if registry
                .entries
                .insert(
                    id,
                    Entry {
                        worker,
                        timeout_secs,
                        permission_profile,
                    },
                )
                .is_some()
            {
                return Err(AgentTaskRegistryBuildError::DuplicateWorkerId(raw));
            }
        }
        Ok(registry)
    }
}

impl AgentTaskWorkerRegistry for DefaultAgentTaskWorkerRegistry {
    fn get(&self, id: &WorkerId) -> Option<Arc<dyn AgentTaskWorker>> {
        self.entries.get(id).map(|entry| Arc::clone(&entry.worker))
    }

    fn timeout_limit_secs(&self, id: &WorkerId) -> Option<u64> {
        self.entries.get(id).map(|entry| entry.timeout_secs)
    }

    fn permission_profile(&self, id: &WorkerId) -> Option<&str> {
        self.entries
            .get(id)
            .map(|entry| entry.permission_profile.as_str())
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
