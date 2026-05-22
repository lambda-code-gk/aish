//! aibe — LLM API バックエンド常駐プロセス。
//!
//! デフォルトではバックグラウンド（デーモン）で起動する。
//! フォアグラウンドで動かす場合は `--foreground` / `-f` を指定する。

#[cfg(unix)]
fn main() {
    let foreground = std::env::args().any(|a| a == "--foreground" || a == "-f");

    if !foreground {
        if let Err(e) = aibe::daemon::daemonize() {
            eprintln!("aibe: failed to daemonize: {e}");
            std::process::exit(1);
        }
    }

    aibe::run();
}

#[cfg(not(unix))]
fn main() {
    eprintln!("aibe: Unix only");
    std::process::exit(1);
}
