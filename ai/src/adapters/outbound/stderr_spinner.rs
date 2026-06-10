//! stderr 上の単行スピナー（stdout / REPL 行と干渉しない）。

use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const FRAMES: &[char] = &['|', '/', '-', '\\'];
const TICK_MS: u64 = 120;

struct SpinnerInner {
    active: Arc<AtomicBool>,
    label: Arc<Mutex<String>>,
    handle: Option<JoinHandle<()>>,
}

impl Default for SpinnerInner {
    fn default() -> Self {
        Self::new()
    }
}

impl SpinnerInner {
    fn new() -> Self {
        Self {
            active: Arc::new(AtomicBool::new(false)),
            label: Arc::new(Mutex::new(String::new())),
            handle: None,
        }
    }

    fn start(&mut self, label: String) {
        self.stop();
        *self.label.lock().expect("spinner label lock") = label;
        self.active.store(true, Ordering::SeqCst);
        let active = Arc::clone(&self.active);
        let label_arc = Arc::clone(&self.label);
        self.handle = Some(thread::spawn(move || {
            let mut frame = 0usize;
            let mut stderr = std::io::stderr();
            while active.load(Ordering::SeqCst) {
                let text = label_arc.lock().expect("spinner label lock").clone();
                let ch = FRAMES[frame % FRAMES.len()];
                let _ = write!(stderr, "\r\x1b[2Kai: {ch} {text}");
                let _ = stderr.flush();
                frame = frame.wrapping_add(1);
                thread::sleep(Duration::from_millis(TICK_MS));
            }
        }));
    }

    fn set_label(&self, label: String) {
        *self.label.lock().expect("spinner label lock") = label;
    }

    fn stop(&mut self) {
        self.active.store(false, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        if std::io::stderr().is_terminal() {
            let mut stderr = std::io::stderr();
            let _ = write!(stderr, "\r\x1b[2K");
            let _ = stderr.flush();
        }
    }

    fn is_running(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }
}

/// TTY stderr 向けの進行スピナー。`stop` で行を消してから他の stderr 出力を行う。
#[derive(Default)]
pub struct StderrSpinner {
    inner: Mutex<SpinnerInner>,
}

impl StderrSpinner {
    pub fn start(&self, label: impl Into<String>) {
        self.inner.lock().expect("spinner lock").start(label.into());
    }

    pub fn set_label(&self, label: impl Into<String>) {
        let inner = self.inner.lock().expect("spinner lock");
        if inner.is_running() {
            inner.set_label(label.into());
        }
    }

    pub fn stop(&self) {
        self.inner.lock().expect("spinner lock").stop();
    }

    pub fn is_running(&self) -> bool {
        self.inner.lock().expect("spinner lock").is_running()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_stop_does_not_panic() {
        let spinner = StderrSpinner::default();
        spinner.start("working…");
        spinner.set_label("thinking…");
        spinner.stop();
        assert!(!spinner.is_running());
    }
}
