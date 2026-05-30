//! `clap` CLI 定義と shell 補完生成。

use std::io;
use std::path::PathBuf;

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::engine::ArgValueCompleter;
use clap_complete::{generate, shells::Bash, shells::Zsh, CompleteEnv};

use crate::adapters::outbound::{complete_profile, complete_session, complete_tools_token};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CompleteShell {
    Bash,
    Zsh,
}

#[derive(Parser)]
#[command(name = "ai", version, about = "aibe client")]
pub struct AiCli {
    #[command(subcommand)]
    pub command: AiCommand,
}

#[derive(Subcommand)]
pub enum AiCommand {
    /// Send a message to the agent
    Ask {
        #[arg(long, value_hint = clap::ValueHint::FilePath)]
        log: Option<PathBuf>,
        #[arg(
            long,
            add = ArgValueCompleter::new(complete_session)
        )]
        session: Option<String>,
        #[arg(long)]
        no_log: bool,
        #[arg(long, value_hint = clap::ValueHint::FilePath)]
        socket: Option<PathBuf>,
        #[arg(long)]
        no_start: bool,
        #[arg(
            long,
            add = ArgValueCompleter::new(complete_tools_token)
        )]
        tools: Option<String>,
        #[arg(
            long,
            add = ArgValueCompleter::new(complete_profile)
        )]
        profile: Option<String>,
        #[arg(long)]
        verbose_tools: bool,
        #[arg(required = true, num_args = 1..)]
        message: Vec<String>,
    },
    /// Generate shell completion scripts (bash or zsh)
    Complete {
        #[arg(value_enum)]
        shell: CompleteShell,
    },
}

impl AiCli {
    pub fn run_complete(shell: CompleteShell) -> io::Result<()> {
        let mut cmd = Self::command();
        match shell {
            CompleteShell::Bash => generate(Bash, &mut cmd, "ai", &mut io::stdout()),
            CompleteShell::Zsh => generate(Zsh, &mut cmd, "ai", &mut io::stdout()),
        }
        Ok(())
    }

    pub fn try_complete_env() -> bool {
        CompleteEnv::with_factory(Self::command)
            .try_complete(std::env::args_os(), None)
            .unwrap_or(false)
    }
}
