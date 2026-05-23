//! ツール executor 解決 outbound port。

use std::sync::Arc;

use super::ToolExecutor;

/// 名前から [`ToolExecutor`] を取得する。
pub trait ToolRegistry: Send + Sync {
    fn get(&self, name: &str) -> Option<Arc<dyn ToolExecutor>>;
}
