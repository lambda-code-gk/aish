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

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum ReplayListFormatArg {
    #[default]
    Tsv,
    Json,
}

impl From<ReplayListFormatArg> for OutputFormat {
    fn from(value: ReplayListFormatArg) -> Self {
        match value {
            ReplayListFormatArg::Tsv => OutputFormat::Tsv,
            ReplayListFormatArg::Json => OutputFormat::Json,
        }
    }
}

impl From<ReplayListFormatArg> for aish_replay::OutputFormat {
    fn from(value: ReplayListFormatArg) -> Self {
        match value {
            ReplayListFormatArg::Tsv => Self::Tsv,
            ReplayListFormatArg::Json => Self::Json,
        }
    }
}

#[derive(Subcommand)]
pub enum ReplayCommand {
    /// List replayable command spans
    List {
        #[arg(long, value_hint = clap::ValueHint::FilePath)]
        log: Option<PathBuf>,
        #[arg(long)]
        index: Option<u32>,
        #[arg(long, value_enum, default_value_t = ReplayListFormatArg::Tsv)]
        format: ReplayListFormatArg,
    },
    /// Show recorded output for a command span
    Show {
        #[arg(long, value_hint = clap::ValueHint::FilePath)]
        log: Option<PathBuf>,
        /// Command span index (`-1` = last entry in `replay list`)
        #[arg(value_name = "INDEX", allow_hyphen_values = true)]
        index: Option<i64>,
        /// Same as positional `INDEX`
        #[arg(long = "index", allow_hyphen_values = true, value_name = "INDEX")]
        index_long: Option<i64>,
        #[arg(long)]
        stderr: bool,
    },
    /// Interactively pick a command span and show its output
    Pick {
        #[arg(long, value_hint = clap::ValueHint::FilePath)]
        log: Option<PathBuf>,
        #[arg(long)]
        index: Option<u32>,
        #[arg(long)]
        stderr: bool,
    },
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
    /// Replay recorded command output without re-execution
    Replay {
        #[command(subcommand)]
        command: ReplayCommand,
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn replay_show_accepts_positional_index() {
        let cli = AishCli::parse_from(["aish", "replay", "show", "3"]);
        let AishCommand::Replay {
            command: ReplayCommand::Show { index, .. },
        } = cli.command
        else {
            panic!("expected replay show");
        };
        assert_eq!(index, Some(3));
    }

    #[test]
    fn replay_show_accepts_negative_positional_index() {
        let cli = AishCli::parse_from(["aish", "replay", "show", "-1"]);
        let AishCommand::Replay {
            command: ReplayCommand::Show { index, .. },
        } = cli.command
        else {
            panic!("expected replay show");
        };
        assert_eq!(index, Some(-1));
    }

    #[test]
    fn replay_show_accepts_long_index_flag() {
        let cli = AishCli::parse_from(["aish", "replay", "show", "--index", "-2"]);
        let AishCommand::Replay {
            command: ReplayCommand::Show {
                index, index_long, ..
            },
        } = cli.command
        else {
            panic!("expected replay show");
        };
        assert_eq!(index, None);
        assert_eq!(index_long, Some(-2));
    }
}
