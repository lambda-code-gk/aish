mod config_allowlist;
mod read_file;
mod registry;
mod shell_exec;
mod tool_output;

pub use config_allowlist::ConfigAllowlistPolicy;
pub use read_file::ReadFileTool;
pub use registry::DefaultToolRegistry;
pub use shell_exec::ShellExecTool;

use std::sync::Arc;

use crate::ports::outbound::{CommandPolicy, ToolRegistry, ToolsConfig};

pub fn build_registry(tools_cfg: &ToolsConfig) -> Arc<dyn ToolRegistry> {
    let max_output = tool_output::clamp_max_tool_output_bytes(tools_cfg.max_tool_output_bytes);
    let policy: Arc<dyn CommandPolicy> =
        Arc::new(ConfigAllowlistPolicy::new(tools_cfg.shell_exec.clone()));
    Arc::new(DefaultToolRegistry::new(
        Arc::new(ShellExecTool::new(policy, max_output)),
        Arc::new(ReadFileTool::new(tools_cfg.read_file.clone(), max_output)),
    ))
}
