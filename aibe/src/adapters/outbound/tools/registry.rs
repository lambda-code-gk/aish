//! ツール名から executor を解決する（adapter 実装）。

use std::collections::HashMap;
use std::sync::Arc;

use thiserror::Error;

use crate::domain::ToolName;
use crate::ports::outbound::{ToolExecutor, ToolRegistry};

#[derive(Debug, Error, PartialEq, Eq)]
#[error("duplicate tool name: {0}")]
pub struct DuplicateToolNameError(pub String);

pub struct DefaultToolRegistry {
    executors: HashMap<ToolName, Arc<dyn ToolExecutor>>,
}

impl DefaultToolRegistry {
    pub fn from_executors(
        executors: impl IntoIterator<Item = Arc<dyn ToolExecutor>>,
    ) -> Result<Self, DuplicateToolNameError> {
        let mut map = HashMap::new();
        for executor in executors {
            let name = executor.name();
            if map.contains_key(&name) {
                return Err(DuplicateToolNameError(name.as_str().to_string()));
            }
            map.insert(name, executor);
        }
        Ok(Self { executors: map })
    }

    pub fn new(
        shell: Arc<dyn ToolExecutor>,
        read_file: Arc<dyn ToolExecutor>,
        list_dir: Arc<dyn ToolExecutor>,
        grep: Arc<dyn ToolExecutor>,
        git_diff: Arc<dyn ToolExecutor>,
        git_status: Arc<dyn ToolExecutor>,
    ) -> Self {
        Self::from_executors([shell, read_file, list_dir, grep, git_diff, git_status])
            .expect("built-in registry must have unique tool names")
    }
}

impl ToolRegistry for DefaultToolRegistry {
    fn get(&self, name: &ToolName) -> Option<Arc<dyn ToolExecutor>> {
        self.executors.get(name).cloned()
    }
}
