//! `clap` CLI 定義と shell 補完生成。

use std::io;
use std::path::PathBuf;

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{generate, shells::Bash, shells::Zsh, CompleteEnv};

use crate::domain::OutputFormat;

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum OutputFormatArg {
    #[default]
    Tsv,
    Json,
    Env,
}

impl From<OutputFormatArg> for OutputFormat {
    fn from(value: OutputFormatArg) -> Self {
        match value {
            OutputFormatArg::Tsv => OutputFormat::Tsv,
            OutputFormatArg::Json => OutputFormat::Json,
            OutputFormatArg::Env => OutputFormat::Env,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CompleteShell {
    Bash,
    Zsh,
}

#[derive(Parser)]
#[command(
    name = "aish",
    version,
    about = "Shell command execution and JSONL session logging"
)]
pub struct AishCli {
    #[command(subcommand)]
    pub command: AishCommand,
}

#[derive(Subcommand)]
pub enum AishCommand {
    /// Run a command and append events to a JSONL log
    Exec {
        #[arg(long, value_enum, default_value_t = OutputFormatArg::Tsv)]
        format: OutputFormatArg,
        #[arg(long, value_hint = clap::ValueHint::FilePath)]
        log: Option<PathBuf>,
        /// Program and args after `--`
        #[arg(
            last = true,
            num_args = 1..,
            required = true,
            allow_hyphen_values = true
        )]
        command: Vec<String>,
    },
    /// Start an interactive shell with session logging
    Shell {
        #[arg(long, value_enum, default_value_t = OutputFormatArg::Tsv)]
        format: OutputFormatArg,
    },
    /// Print current session metadata
    Session {
        #[arg(long, value_enum, default_value_t = OutputFormatArg::Tsv)]
        format: OutputFormatArg,
    },
    /// Generate shell completion scripts (bash or zsh)
    Complete {
        #[arg(value_enum)]
        shell: CompleteShell,
    },
}

impl AishCli {
    pub fn run_complete(shell: CompleteShell) -> io::Result<()> {
        let mut cmd = Self::command();
        match shell {
            CompleteShell::Bash => generate(Bash, &mut cmd, "aish", &mut io::stdout()),
            CompleteShell::Zsh => generate(Zsh, &mut cmd, "aish", &mut io::stdout()),
        }
        Ok(())
    }

    pub fn try_complete_env() -> bool {
        CompleteEnv::with_factory(Self::command)
            .try_complete(std::env::args_os(), None)
            .unwrap_or(false)
    }
}
