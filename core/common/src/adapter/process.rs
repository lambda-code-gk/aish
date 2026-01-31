//! サブプロセス実行の抽象（アダプター）
//!
//! タスク実行や `aish truncate_console_log` など、外部コマンド起動を trait で抽象化する。

use crate::error::Error;
use std::path::Path;

/// サブプロセス実行の抽象
///
/// 実装は `common::adapter::StdProcess`（std::process::Command）など。
pub trait Process: Send + Sync {
    /// プログラムを引数付きで実行し、終了コードを返す
    fn run(&self, program: &Path, args: &[String]) -> Result<i32, Error>;
}
