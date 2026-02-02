//! Ports & Adapters のポート定義
//!
//! - inbound: ドライバ（CLI）がアプリを呼び出すインターフェース
//! - outbound: アプリが外界（承認・LLM ストリーム等）を使うための trait

pub mod inbound;
pub mod outbound;
