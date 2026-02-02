//! PTY Outbound ポートの re-export（定義は ports/outbound/pty）

#[cfg(unix)]
pub use crate::ports::outbound::pty::{Pty, PtyProcessStatus, PtySpawn, Winsize};
