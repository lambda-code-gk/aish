//! `clap` CLI 定義と shell 補完生成。

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;

use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::engine::ArgValueCompleter;
use clap_complete::{generate, shells::Bash, shells::Zsh, CompleteEnv};

use crate::adapters::outbound::{
    complete_preset, complete_profile, complete_session, complete_tools_token,
};
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

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum HistoryStatusArg {
    Ok,
    Error,
}

#[derive(Debug, Clone, Args, Default)]
pub struct TurnOptions {
    #[arg(long, short = 'q')]
    pub quiet: bool,
    #[arg(long, value_enum)]
    pub format: Option<OutputFormatArg>,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long, add = ArgValueCompleter::new(complete_preset))]
    pub preset: Option<String>,
    #[arg(long)]
    pub log_tail: Option<usize>,
    #[arg(long, value_hint = clap::ValueHint::FilePath)]
    pub log: Option<PathBuf>,
    #[arg(
        long,
        add = ArgValueCompleter::new(complete_session)
    )]
    pub session: Option<String>,
    #[arg(long)]
    pub no_log: bool,
    #[arg(long, value_hint = clap::ValueHint::FilePath)]
    pub socket: Option<PathBuf>,
    #[arg(long)]
    pub no_start: bool,
    #[arg(
        long,
        add = ArgValueCompleter::new(complete_tools_token)
    )]
    pub tools: Option<String>,
    #[arg(
        long,
        add = ArgValueCompleter::new(complete_profile)
    )]
    pub profile: Option<String>,
    #[arg(long)]
    pub new: bool,
    #[arg(long)]
    pub verbose_tools: bool,
    /// 進行表示（TTY では既定 ON）。非 TTY では `--progress` で明示有効化。
    #[arg(long, conflicts_with = "no_progress")]
    pub progress: bool,
    /// 進行表示を無効にする
    #[arg(long, conflicts_with = "progress")]
    pub no_progress: bool,
    #[arg(long)]
    pub timeout: Option<u64>,
    #[arg(long)]
    pub yes_exec: bool,
    /// TTY 向け console hint（端末サイズに応じた system instruction）を有効にする
    #[arg(long, short = 'H', conflicts_with = "no_console_hint")]
    pub console_hint: bool,
    /// console hint を無効にする
    #[arg(long, short = 'N', conflicts_with = "console_hint")]
    pub no_console_hint: bool,
}

#[derive(Parser)]
#[command(
    name = "ai",
    version,
    about = "aibe client",
    arg_required_else_help = false
)]
pub struct AiCli {
    #[command(subcommand)]
    pub command: AiCommand,
}

#[derive(Subcommand)]
pub enum AiCommand {
    /// Send a message to the agent
    Ask {
        #[command(flatten)]
        turn: TurnOptions,
        #[arg(long, value_hint = clap::ValueHint::FilePath)]
        file: Option<PathBuf>,
        #[arg(required = false, num_args = 0.., allow_hyphen_values = true)]
        message: Vec<String>,
    },
    /// Multi-turn chat REPL
    Chat {
        #[command(flatten)]
        turn: TurnOptions,
    },
    /// Re-run the same content with the current defaults
    Retry {
        #[command(flatten)]
        turn: TurnOptions,
        history_id: String,
    },
    /// Re-run the saved request envelope from local history
    Rerun {
        #[command(flatten)]
        turn: TurnOptions,
        history_id: String,
    },
    /// Show local request history
    History {
        #[arg(long, short = 'q')]
        quiet: bool,
        #[arg(long, value_enum, default_value_t = OutputFormatArg::Tsv)]
        format: OutputFormatArg,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long, add = ArgValueCompleter::new(complete_session))]
        session: Option<String>,
        #[arg(long)]
        command: Option<String>,
        #[arg(long, value_enum)]
        status: Option<HistoryStatusArg>,
    },
    /// Report local status of the ai client
    Status {
        #[arg(long, short = 'q')]
        quiet: bool,
        #[arg(long, value_enum, default_value_t = OutputFormatArg::Tsv)]
        format: OutputFormatArg,
        #[arg(long, value_hint = clap::ValueHint::FilePath)]
        socket: Option<PathBuf>,
    },
    /// Human-oriented alias of `status`
    Doctor {
        #[arg(long, short = 'q')]
        quiet: bool,
        #[arg(long, value_enum, default_value_t = OutputFormatArg::Tsv)]
        format: OutputFormatArg,
        #[arg(long, value_hint = clap::ValueHint::FilePath)]
        socket: Option<PathBuf>,
    },
    /// Check whether the aibe socket is alive
    Ping {
        #[arg(long, short = 'q')]
        quiet: bool,
        #[arg(long, value_enum, default_value_t = OutputFormatArg::Tsv)]
        format: OutputFormatArg,
        #[arg(long, value_hint = clap::ValueHint::FilePath)]
        socket: Option<PathBuf>,
    },
    /// Generate shell completion scripts (bash or zsh)
    Complete {
        #[arg(value_enum)]
        shell: CompleteShell,
    },
}

impl AiCli {
    pub fn parse_with_default_ask() -> Result<Self, clap::Error> {
        Self::try_parse_from(normalize_args(std::env::args_os()))
    }

    pub fn normalized_args_for_completion() -> Vec<OsString> {
        normalize_args(std::env::args_os())
    }

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

fn normalize_args(args: impl IntoIterator<Item = OsString>) -> Vec<OsString> {
    let mut args: Vec<OsString> = args.into_iter().collect();
    if args.len() <= 1 {
        args.push(OsString::from("ask"));
        return args;
    }

    let first = args[1].to_string_lossy();
    if first == "ask"
        || first == "chat"
        || first == "retry"
        || first == "rerun"
        || first == "history"
        || first == "status"
        || first == "doctor"
        || first == "ping"
        || first == "complete"
        || first == "help"
        || first == "-h"
        || first == "--help"
        || first == "-V"
        || first == "--version"
    {
        return args;
    }

    args.insert(1, OsString::from("ask"));
    args
}
