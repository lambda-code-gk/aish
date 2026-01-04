use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Mutex;

// グローバルなデバッグログファイルハンドル
static DEBUG_LOG: Mutex<Option<std::fs::File>> = Mutex::new(None);

// デバッグログを初期化（環境変数からファイル名を取得）
#[cfg(debug_assertions)]
pub fn init_debug_log() -> Option<String> {
    if let Ok(log_file) = std::env::var("AISH_DEBUG_LOG") {
        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_file)
        {
            Ok(file) => {
                *DEBUG_LOG.lock().unwrap() = Some(file);
                Some(log_file)
            }
            Err(e) => {
                eprintln!("Warning: Failed to open debug log file {}: {}", log_file, e);
                None
            }
        }
    } else {
        None
    }
}

#[cfg(not(debug_assertions))]
pub fn init_debug_log() -> Option<String> {
    None
}

// デバッグログに書き込む
#[cfg(debug_assertions)]
pub fn debug_log(tool_name: &str, message: &str) {
    if let Ok(mut guard) = DEBUG_LOG.lock() {
        if let Some(ref mut file) = *guard {
            use std::time::{SystemTime, UNIX_EPOCH};
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis();
            let log_line = format!("[{}] {}: {}\n", timestamp, tool_name, message);
            let _ = file.write_all(log_line.as_bytes());
            let _ = file.flush();
        }
    }
}

#[cfg(not(debug_assertions))]
pub fn debug_log(_tool_name: &str, _message: &str) {
    // リリースビルドでは何もしない
}

