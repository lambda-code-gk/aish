//! ターン単位のキャンセル（SIGINT / SIGTERM。プロセス全体で handler は 1 回だけ登録）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once};

static CTRLC_INIT: Once = Once::new();
static SIGTERM_INIT: Once = Once::new();
static ACTIVE_CANCEL: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);
static SIGNAL_CANCEL_REQUESTED: AtomicBool = AtomicBool::new(false);

fn store_active_cancel() {
    if let Ok(active) = ACTIVE_CANCEL.lock() {
        if let Some(flag) = active.as_ref() {
            flag.store(true, Ordering::SeqCst);
        }
    }
}

/// 現在のターン用 cancel フラグを有効化する。`ctrlc` / SIGTERM handler は初回のみ登録する。
pub fn register_turn_cancel(flag: Arc<AtomicBool>) -> Result<(), ctrlc::Error> {
    SIGNAL_CANCEL_REQUESTED.store(false, Ordering::SeqCst);
    *ACTIVE_CANCEL.lock().expect("ACTIVE_CANCEL mutex poisoned") = Some(flag);
    let mut init_error = None;
    CTRLC_INIT.call_once(|| {
        if let Err(e) = ctrlc::set_handler(|| {
            store_active_cancel();
        }) {
            init_error = Some(e);
        }
    });
    SIGTERM_INIT.call_once(|| {
        install_sigterm_handler();
    });
    init_error.map_or(Ok(()), Err)
}

fn install_sigterm_handler() {
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = sigterm_handler as usize;
        action.sa_flags = libc::SA_RESTART;
        libc::sigemptyset(&mut action.sa_mask);
        let _ = libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut());
    }
}

extern "C" fn sigterm_handler(_sig: libc::c_int) {
    SIGNAL_CANCEL_REQUESTED.store(true, Ordering::SeqCst);
}

/// OS signal handler が受けた SIGTERM を通常制御フローから読む。
pub fn signal_cancel_requested() -> bool {
    SIGNAL_CANCEL_REQUESTED.load(Ordering::SeqCst)
}

/// ターン終了時に active cancel フラグを外す。
pub fn clear_turn_cancel() {
    SIGNAL_CANCEL_REQUESTED.store(false, Ordering::SeqCst);
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

    #[test]
    fn sigterm_sets_signal_cancel_flag() {
        let guard = TurnCancelGuard::new().expect("register");
        assert!(!signal_cancel_requested());
        unsafe {
            libc::raise(libc::SIGTERM);
        }
        assert!(signal_cancel_requested());
        drop(guard);
    }
}
