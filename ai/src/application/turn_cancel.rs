//! ターン単位の Ctrl+C キャンセル（プロセス全体で handler は 1 回だけ登録）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once};

static CTRLC_INIT: Once = Once::new();
static ACTIVE_CANCEL: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);

/// 現在のターン用 cancel フラグを有効化する。`ctrlc` handler は初回のみ登録する。
pub fn register_turn_cancel(flag: Arc<AtomicBool>) -> Result<(), ctrlc::Error> {
    *ACTIVE_CANCEL.lock().expect("ACTIVE_CANCEL mutex poisoned") = Some(flag);
    let mut init_error = None;
    CTRLC_INIT.call_once(|| {
        if let Err(e) = ctrlc::set_handler(|| {
            if let Ok(active) = ACTIVE_CANCEL.lock() {
                if let Some(flag) = active.as_ref() {
                    flag.store(true, Ordering::SeqCst);
                }
            }
        }) {
            init_error = Some(e);
        }
    });
    init_error.map_or(Ok(()), Err)
}

/// ターン終了時に active cancel フラグを外す。
pub fn clear_turn_cancel() {
    if let Ok(mut active) = ACTIVE_CANCEL.lock() {
        *active = None;
    }
}

/// ターン終了時に `clear_turn_cancel` する RAII ガード。
pub struct TurnCancelGuard {
    flag: Arc<AtomicBool>,
}

impl TurnCancelGuard {
    pub fn new() -> Result<Self, ctrlc::Error> {
        let flag = Arc::new(AtomicBool::new(false));
        register_turn_cancel(Arc::clone(&flag))?;
        Ok(Self { flag })
    }

    pub fn flag(&self) -> &Arc<AtomicBool> {
        &self.flag
    }
}

impl Drop for TurnCancelGuard {
    fn drop(&mut self) {
        clear_turn_cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_twice_does_not_fail() {
        let first = TurnCancelGuard::new().expect("first register");
        drop(first);
        let second = TurnCancelGuard::new().expect("second register");
        drop(second);
    }
}
