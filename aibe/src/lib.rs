//! LLM API バックエンド。Hexagonal（Ports & Adapters）で Unix socket 経由のエージェント処理を提供する。

#![cfg(unix)]

pub mod adapters;
pub mod application;

pub use domain::{
    is_known_tool, ShellLogTail, ToolName, UnknownToolError, KNOWN_TOOLS, READ_FILE, SHELL_EXEC,
};
pub mod client;
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
    let config = adapters::outbound::TomlConfig::load()?;
    if client::ping(&config.socket_path) {
        eprintln!("aibe: already running at {}", config.socket_path.display());
        return Ok(());
    }

    let llm = adapters::outbound::build_llm(&config)?;
    let termination_capability = adapters::outbound::termination_capability(&config.llm);
    let tools_config = config.tools.clone();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(application::server::run(
        config.socket_path,
        llm,
        tools_config,
        termination_capability,
    ))
}

/// デフォルトの Unix socket パス（`$HOME/.local/share/aibe/run.sock`）。
pub fn default_socket_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home)
        .join(".local/share/aibe")
        .join("run.sock")
}
