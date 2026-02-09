//! ツール実装（adapter 層）
//!
//! OS 副作用を伴う具象ツールをここに配置する。

pub(crate) mod grep;
pub(crate) mod read_file;
pub(crate) mod replace_file;
pub(crate) mod run_shell;
pub(crate) mod write_file;

pub(crate) use grep::GrepTool;
pub(crate) use read_file::ReadFileTool;
pub(crate) use replace_file::ReplaceFileTool;
pub(crate) use run_shell::ShellTool;
pub(crate) use write_file::WriteFileTool;
