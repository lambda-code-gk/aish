//! LLM API バックエンド。プロトコルでクライアントと通信し、API 呼び出しを処理する。

#[cfg(unix)]
pub mod daemon;

use std::time::Duration;

/// 常駐サーバのメインループ。将来ここにプロトコル受付と LLM API ディスパッチを実装する。
pub fn run() -> ! {
    // TODO: プロトコルリスナー起動、リクエストキュー、LLM API 呼び出し
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}
