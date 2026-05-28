//! ツール名から executor を解決する（adapter 実装）。

use std::collections::HashMap;
use std::sync::Arc;

use crate::domain::ToolName;
use crate::ports::outbound::{ToolExecutor, ToolRegistry};

pub struct DefaultToolRegistry {
    executors: HashMap<ToolName, Arc<dyn ToolExecutor>>,
}

impl DefaultToolRegistry {
    pub fn new(
        shell: Arc<dyn ToolExecutor>,
        read_file: Arc<dyn ToolExecutor>,
        list_dir: Arc<dyn ToolExecutor>,
        grep: Arc<dyn ToolExecutor>,
        git_diff: Arc<dyn ToolExecutor>,
        git_status: Arc<dyn ToolExecutor>,
    ) -> Self {
        let mut executors = HashMap::new();
        executors.insert(shell.name(), shell);
        executors.insert(read_file.name(), read_file);
        executors.insert(list_dir.name(), list_dir);
        executors.insert(grep.name(), grep);
        executors.insert(git_diff.name(), git_diff);
        executors.insert(git_status.name(), git_status);
        Self { executors }
    }
}

impl ToolRegistry for DefaultToolRegistry {
    fn get(&self, name: &ToolName) -> Option<Arc<dyn ToolExecutor>> {
        self.executors.get(name).cloned()
    }
}
