//! Ctrl+C（SIGINT）で割り込みフラグを立てる InterruptChecker 実装
//!
//! コンストラクタで ctrlc ハンドラを登録し、is_interrupted() でフラグを読む。

use crate::ports::outbound::InterruptChecker;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Ctrl+C を受けたらフラグを立てる実装
pub struct SigintChecker {
    flag: Arc<AtomicBool>,
}

impl SigintChecker {
    /// 新しいチェッカーを作成し、SIGINT ハンドラを登録する。
    /// 複数回呼んでもハンドラは初回のみ登録される（ctrlc の仕様）。
    pub fn new() -> Result<Self, ctrlc::Error> {
        let flag = Arc::new(AtomicBool::new(false));
        let flag_clone = Arc::clone(&flag);
        ctrlc::set_handler(move || {
            flag_clone.store(true, Ordering::Relaxed);
        })?;
        Ok(Self { flag })
    }
}

impl InterruptChecker for SigintChecker {
    fn is_interrupted(&self) -> bool {
        self.flag.load(Ordering::Relaxed)
    }
}

/// 割り込みを検知しないスタブ（ハンドラ登録に失敗した場合などに使用）
pub struct NoopInterruptChecker;

impl NoopInterruptChecker {
    pub fn new() -> Self {
        Self
    }
}

impl InterruptChecker for NoopInterruptChecker {
    fn is_interrupted(&self) -> bool {
        false
    }
}
