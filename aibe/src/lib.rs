//! LLM API バックエンド。Hexagonal（Ports & Adapters）で Unix socket 経由のエージェント処理を提供する。

#![cfg(unix)]

pub mod adapters;
pub mod application;
pub mod daemon;
pub mod domain;
pub mod ports;
pub mod protocol;

use std::path::PathBuf;

/// 常駐サーバのエントリポイント。
pub fn run() -> ! {
    if let Err(e) = try_run() {
        eprintln!("aibe: {e}");
        std::process::exit(1);
    }
    std::process::exit(0);
}

fn try_run() -> anyhow::Result<()> {
    let config = adapters::outbound::EnvConfig::load()?;
    let llm = adapters::outbound::MockLlm::new();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(application::server::run(config.socket_path, llm))
}

/// デフォルトの Unix socket パス（`$HOME/.local/share/aibe/run.sock`）。
pub fn default_socket_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home)
        .join(".local/share/aibe")
        .join("run.sock")
}
