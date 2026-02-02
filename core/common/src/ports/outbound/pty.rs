//! PTY Outbound ポート（Unix 用）
//!
//! シェル spawn・wait・resize を抽象化する。common は libc に依存しないため、実装は aish 側に置く。

#[cfg(unix)]
use std::os::unix::io::RawFd;

use crate::error::Error;

/// ウィンドウサイズ（libc::winsize の抽象）
#[derive(Debug, Clone, Copy)]
pub struct Winsize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

impl Default for Winsize {
    fn default() -> Self {
        Self {
            ws_row: 24,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        }
    }
}

/// PTY 子プロセスの終了状態
#[derive(Debug, Clone, Copy)]
pub enum PtyProcessStatus {
    Exited(i32),
    Signaled(i32),
}

/// PTY の抽象（Outbound ポート）
#[cfg(unix)]
pub trait Pty: Send + Sync {
    fn master_fd(&self) -> RawFd;
    fn wait_nonblocking(&self) -> Result<Option<PtyProcessStatus>, Error>;
    fn set_winsize(&self, ws: &Winsize) -> Result<(), Error>;
}

/// PTY を spawn する抽象（Outbound ポート）
#[cfg(unix)]
pub trait PtySpawn: Send + Sync {
    fn spawn(
        &self,
        cmd: Option<&[String]>,
        cwd: Option<&str>,
        env: &[(String, String)],
    ) -> Result<Box<dyn Pty>, Error>;
}
