//! LLM API バックエンド。Hexagonal（Ports & Adapters）で Unix socket 経由のエージェント処理を提供する。

#![cfg(unix)]

pub mod adapters;
pub mod application;
pub mod clap_cli;
pub mod daemon;
pub mod domain;
pub mod ports;

#[cfg(feature = "memory")]
pub mod plugin_memory;

pub use adapters::inbound::control_plane::{run_restart, run_status, run_stop};
pub use clap_cli::StatusFormat;

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
    if aibe_client::ping(&config.socket_path) {
        eprintln!("aibe: already running at {}", config.socket_path.display());
        return Ok(());
    }

    daemon::cleanup_stale_pid_file_before_start(&config.socket_path);

    let profile_registry = adapters::outbound::build_profile_registry(&config.llm)?;
    let tools_config = config.tools.clone();
    let external_commands = config.external_commands.clone();
    let agent_task_config = config.agent_task.clone();
    let config_path = application::server::resolve_config_path();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(application::server::run_with_agent_task(
        config.socket_path,
        config_path,
        profile_registry,
        tools_config,
        external_commands,
        agent_task_config,
        config.router.profile,
        config.conversation_store_root,
        config.memory,
    ))
}
