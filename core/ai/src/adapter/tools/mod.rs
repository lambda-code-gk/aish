//! ツール実装（adapter 層）
//!
//! OS 副作用を伴う具象ツールをここに配置する。

pub(crate) mod run_shell;
pub(crate) use run_shell::ShellTool;
