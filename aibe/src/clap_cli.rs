//! `clap` CLI 定義と shell 補完生成。

use std::io;

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{generate, shells::Bash, shells::Zsh, CompleteEnv};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CompleteShell {
    Bash,
    Zsh,
}

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum StatusFormat {
    #[default]
    Json,
}

#[derive(Parser)]
#[command(name = "aibe", version, about = "LLM agent backend daemon")]
pub struct AibeCli {
    /// Run in foreground (do not daemonize)
    #[arg(long, short = 'f')]
    pub foreground: bool,
    #[command(subcommand)]
    pub command: Option<AibeCommand>,
}

#[derive(Subcommand)]
pub enum AibeCommand {
    /// Generate shell completion scripts (bash or zsh)
    Complete {
        #[arg(value_enum)]
        shell: CompleteShell,
    },
    /// Gracefully stop the running daemon
    Stop,
    /// Gracefully restart the daemon with the current config
    Restart,
    /// Report daemon status
    Status {
        #[arg(long, value_enum, default_value_t = StatusFormat::Json)]
        format: StatusFormat,
    },
}

impl AibeCli {
    pub fn run_complete(shell: CompleteShell) -> io::Result<()> {
        let mut cmd = Self::command();
        match shell {
            CompleteShell::Bash => generate(Bash, &mut cmd, "aibe", &mut io::stdout()),
            CompleteShell::Zsh => generate(Zsh, &mut cmd, "aibe", &mut io::stdout()),
        }
        Ok(())
    }

    pub fn try_complete_env() -> bool {
        CompleteEnv::with_factory(Self::command)
            .try_complete(std::env::args_os(), None)
            .unwrap_or(false)
    }

    pub fn is_control_command(&self) -> bool {
        matches!(
            self.command,
            Some(AibeCommand::Stop | AibeCommand::Restart | AibeCommand::Status { .. })
        )
    }
}
