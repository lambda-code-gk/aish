//! aibe — LLM API バックエンド常駐プロセス。
//!
//! デフォルトではバックグラウンド（デーモン）で起動する。
//! フォアグラウンドで動かす場合は `--foreground` / `-f` を指定する。

#[cfg(unix)]
fn main() {
    use aibe::clap_cli::{AibeCli, AibeCommand};
    use clap::Parser;

    if AibeCli::try_complete_env() {
        return;
    }

    let cli = AibeCli::parse();
    if let Some(AibeCommand::Complete { shell }) = cli.command {
        if let Err(e) = AibeCli::run_complete(shell) {
            eprintln!("aibe: {e}");
            std::process::exit(1);
        }
        return;
    }

    if !cli.foreground {
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
