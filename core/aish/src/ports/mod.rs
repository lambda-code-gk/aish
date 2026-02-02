//! Ports & Adapters のポート定義
//!
//! - inbound: ドライバ（CLI）がアプリを呼び出すインターフェース
//! - outbound: 対話シェル起動等の trait（common の FileSystem / PtySpawn / Signal 等も利用）

pub mod inbound;
pub mod outbound;
