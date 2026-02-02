//! シグナル Outbound ポートの re-export（定義は ports/outbound/signal）

#[cfg(unix)]
pub use crate::ports::outbound::Signal;
