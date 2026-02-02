//! シグナル Outbound ポート（Unix 用）
//!
//! SIGUSR1/2, SIGWINCH の設定・ポール・送信を抽象化する。実装は aish 側に置く。

use crate::error::Error;

/// シグナル設定・ポール・送信の抽象（Outbound ポート）
#[cfg(unix)]
pub trait Signal: Send + Sync {
    fn setup_sigwinch(&self) -> Result<(), Error>;
    fn setup_sigusr1(&self) -> Result<(), Error>;
    fn setup_sigusr2(&self) -> Result<(), Error>;
    fn check_sigwinch(&self) -> bool;
    fn check_sigusr1(&self) -> bool;
    fn check_sigusr2(&self) -> bool;
    fn send_signal(&self, pid: i32, sig: i32) -> Result<(), Error>;
}
