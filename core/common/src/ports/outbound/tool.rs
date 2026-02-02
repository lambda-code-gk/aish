//! ツール実行 Outbound ポート（re-export）
//!
//! トレイト定義は tool モジュールにあり、循環参照を避けるためここで re-export する。

pub use crate::tool::Tool;
