mod config_allowlist;
mod git_common;
mod git_diff;
mod git_status;
mod grep;
mod list_dir;
mod read_file;
mod registry;
mod shell_exec;
mod subprocess;
mod tool_output;

pub use config_allowlist::ConfigAllowlistPolicy;
pub use git_diff::GitDiffTool;
pub use git_status::GitStatusTool;
pub use grep::GrepTool;
pub use list_dir::ListDirTool;
pub use read_file::ReadFileTool;
pub use registry::DefaultToolRegistry;
pub use shell_exec::ShellExecTool;

use std::sync::Arc;

use crate::ports::outbound::{CommandPolicy, ExternalCommandConfig, ToolRegistry, ToolsConfig};

pub fn build_registry(
    tools_cfg: &ToolsConfig,
    external_commands: &[ExternalCommandConfig],
) -> Arc<dyn ToolRegistry> {
    let max_output = tool_output::clamp_max_tool_output_bytes(tools_cfg.max_tool_output_bytes);
    let policy: Arc<dyn CommandPolicy> =
        Arc::new(ConfigAllowlistPolicy::new(tools_cfg.shell_exec.clone()));
    Arc::new(DefaultToolRegistry::new(
        Arc::new(ShellExecTool::new(
            policy,
            max_output,
            external_commands.to_vec(),
        )),
        Arc::new(ReadFileTool::new(tools_cfg.read_file.clone(), max_output)),
        Arc::new(ListDirTool::new(max_output, tools_cfg.explore.clone())),
        Arc::new(GrepTool::new(max_output, tools_cfg.explore.clone())),
        Arc::new(GitDiffTool::new(max_output)),
        Arc::new(GitStatusTool::new(max_output)),
    ))
}
