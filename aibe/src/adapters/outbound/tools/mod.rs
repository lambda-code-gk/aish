mod apply_patch;
mod config_allowlist;
pub mod diff_preview;
pub mod file_atomic;
pub(crate) mod file_change_common;
mod git_common;
mod git_diff;
mod git_status;
mod grep;
mod human_task;
mod list_dir;
mod patch_parser;
mod read_file;
mod registry;
mod safe_path;
mod shell_exec;
pub(crate) mod subprocess;
mod tool_output;
mod write_file;

pub use apply_patch::ApplyPatchTool;

pub use config_allowlist::ConfigAllowlistPolicy;
pub use diff_preview::build_unified_diff_preview;
pub use file_atomic::{
    atomic_write_file, dir_has_temp_leftovers, temp_file_prefix, AtomicWriteError,
};
pub use git_diff::GitDiffTool;
pub use git_status::GitStatusTool;
pub use grep::GrepTool;
pub use human_task::HumanTaskTool;
pub use list_dir::ListDirTool;
pub use read_file::{ReadFileTool, FILE_METADATA_PREFIX};
pub use registry::DefaultToolRegistry;
pub use safe_path::{ReadPathPolicy, SafePathError, WritePathPolicy};
pub use shell_exec::ShellExecTool;
pub use write_file::WriteFileTool;

use std::sync::Arc;

use crate::ports::outbound::{
    CommandPolicy, ExternalCommandConfig, FileChangeExecutor, ToolExecutor, ToolRegistry,
    ToolsConfig,
};

pub fn build_registry(
    tools_cfg: &ToolsConfig,
    external_commands: &[ExternalCommandConfig],
    file_change: Arc<dyn FileChangeExecutor>,
) -> Arc<dyn ToolRegistry> {
    build_registry_with_extras(tools_cfg, external_commands, file_change, Vec::new())
}

pub fn build_registry_with_extras(
    tools_cfg: &ToolsConfig,
    external_commands: &[ExternalCommandConfig],
    file_change: Arc<dyn FileChangeExecutor>,
    extra_executors: Vec<Arc<dyn ToolExecutor>>,
) -> Arc<dyn ToolRegistry> {
    let max_output = tool_output::clamp_max_tool_output_bytes(tools_cfg.max_tool_output_bytes);
    let policy: Arc<dyn CommandPolicy> =
        Arc::new(ConfigAllowlistPolicy::new(tools_cfg.shell_exec.clone()));
    let file_change_service = file_change;
    let mut executors = vec![
        Arc::new(ShellExecTool::new(
            policy,
            max_output,
            external_commands.to_vec(),
        )) as Arc<dyn ToolExecutor>,
        Arc::new(HumanTaskTool),
        Arc::new(ReadFileTool::new(tools_cfg.read_file.clone(), max_output)),
        Arc::new(ListDirTool::new(max_output, tools_cfg.explore.clone())),
        Arc::new(GrepTool::new(max_output, tools_cfg.explore.clone())),
        Arc::new(GitDiffTool::new(max_output)),
        Arc::new(GitStatusTool::new(max_output)),
        Arc::new(WriteFileTool::new(
            tools_cfg.file_write.clone(),
            Arc::clone(&file_change_service),
        )),
        Arc::new(ApplyPatchTool::new(
            tools_cfg.file_write.clone(),
            file_change_service,
        )),
    ];
    executors.extend(extra_executors);
    Arc::new(
        DefaultToolRegistry::from_executors(executors)
            .expect("built-in registry must have unique tool names"),
    )
}
