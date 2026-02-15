//! 複数ハンドラを順次実行するライフサイクルフックの実装

use crate::domain::LifecycleEvent;
use crate::ports::outbound::LifecycleHooks;
use common::error::Error;
use std::sync::Arc;

/// 1 つのライフサイクルイベントに対する処理（adapter 内で実装する trait）
pub(crate) trait LifecycleHandler: Send + Sync {
    fn on_event(&self, event: &LifecycleEvent) -> Result<(), Error>;
}

/// 登録されたハンドラを順に実行するコンポジット
pub struct CompositeLifecycleHooks {
    handlers: Vec<Arc<dyn LifecycleHandler>>,
}

impl CompositeLifecycleHooks {
    pub fn new(handlers: Vec<Arc<dyn LifecycleHandler>>) -> Self {
        Self { handlers }
    }
}

impl LifecycleHooks for CompositeLifecycleHooks {
    fn on_event(&self, event: &LifecycleEvent) -> Result<(), Error> {
        for h in &self.handlers {
            h.on_event(event)?;
        }
        Ok(())
    }
}
