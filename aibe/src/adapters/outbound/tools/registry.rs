//! ツール名から executor を解決する（adapter 実装）。

use std::collections::HashMap;
use std::sync::Arc;

use crate::domain::ToolName;
use crate::ports::outbound::{ToolExecutor, ToolRegistry};

pub struct DefaultToolRegistry {
    executors: HashMap<ToolName, Arc<dyn ToolExecutor>>,
}

impl DefaultToolRegistry {
    pub fn new(shell: Arc<dyn ToolExecutor>, read_file: Arc<dyn ToolExecutor>) -> Self {
        let mut executors = HashMap::new();
        executors.insert(shell.name(), shell);
        executors.insert(read_file.name(), read_file);
        Self { executors }
    }
}

impl ToolRegistry for DefaultToolRegistry {
    fn get(&self, name: &ToolName) -> Option<Arc<dyn ToolExecutor>> {
        self.executors.get(name).cloned()
    }
}
