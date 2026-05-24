//! ツールレジストリ outbound port。

use std::sync::Arc;

use super::ToolExecutor;
use crate::domain::ToolName;

/// 名前から [`ToolExecutor`] を取得する。
pub trait ToolRegistry: Send + Sync {
    fn get(&self, name: &ToolName) -> Option<Arc<dyn ToolExecutor>>;
}
