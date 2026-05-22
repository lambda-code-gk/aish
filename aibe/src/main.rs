//! aibe — LLM API バックエンド常駐プロセス。
//!
//! デフォルトではバックグラウンド（デーモン）で起動する。
//! フォアグラウンドで動かす場合は `--foreground` / `-f` を指定する。

fn main() {
    let foreground = std::env::args().any(|a| a == "--foreground" || a == "-f");

    #[cfg(unix)]
    if !foreground {
        if let Err(e) = aibe::daemon::daemonize() {
            eprintln!("aibe: failed to daemonize: {e}");
            std::process::exit(1);
        }
    }

    #[cfg(not(unix))]
    if !foreground {
        eprintln!("aibe: daemon mode is only supported on Unix; running in foreground");
    }

    aibe::run();
}
