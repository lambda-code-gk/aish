//! Ports & Adapters のポート定義
//!
//! - inbound: ドライバ（CLI）がアプリを呼び出すインターフェース
//! - outbound: aish 固有の outbound trait はなし（common の FileSystem / PtySpawn / Signal 等を利用）

pub mod inbound;
