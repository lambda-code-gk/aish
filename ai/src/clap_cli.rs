//! `clap` CLI 定義と shell 補完生成。

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;

use clap::{Args, Command, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::engine::ArgValueCompleter;
use clap_complete::{generate, shells::Bash, shells::Zsh, CompleteEnv};

use crate::adapters::outbound::{
    complete_preset, complete_profile, complete_session, complete_tools_token,
    recall_env_snippet_for_shell, recall_hook_for_shell,
};
use crate::domain::is_known_cli_head;
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

#[derive(Subcommand)]
pub enum RecallCommand {
    /// Print the next suggested shell command and advance the recall cursor
    Next,
    /// Print the previous suggested shell command and move the recall cursor backward
    Prev,
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
    /// 自動承認された shell_exec の stderr 通知を抑止する
    #[arg(long)]
    pub silent_exec: bool,
    /// TTY 向け console hint（端末サイズに応じた system instruction）を有効にする
    #[arg(long, short = 'H', conflicts_with = "no_console_hint")]
    pub console_hint: bool,
    /// console hint を無効にする
    #[arg(long, short = 'N', conflicts_with = "console_hint")]
    pub no_console_hint: bool,
    /// Smart Preprocessor の route / LLM accounting trace を stderr に出す
    #[arg(long)]
    pub trace_route: bool,
    /// 親 agent の shell_exec を human shell handoff へ変換する（0055 minimal）
    #[arg(long, global = true)]
    pub collaborative: bool,
}

#[derive(Parser)]
#[command(
    name = "ai",
    version,
    about = "aibe client",
    after_help = "Interactive prompt input:\n  Run `ai` with no arguments on a TTY to compose a prompt in an editor.\n  Editor priority: AI_EDITOR -> VISUAL -> EDITOR -> built-in mini editor.\n  Mini editor: Enter newline, Up/Down move between lines, Ctrl+D or Alt+Enter submit, Ctrl+C cancel.",
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
    /// Inspect Smart Preprocessor observation logs
    Smart {
        #[command(subcommand)]
        command: SmartCommand,
    },
    /// Generate shell completion scripts (bash or zsh)
    Complete {
        #[arg(value_enum)]
        shell: CompleteShell,
    },
    /// Insert the next suggested shell command for the current shell prompt
    Recall {
        #[command(subcommand)]
        command: RecallCommand,
    },
    /// Manage project goal (contextual memory)
    Goal {
        #[command(subcommand)]
        command: GoalCommand,
    },
    /// Manage current focus (contextual memory)
    Now {
        #[command(subcommand)]
        command: NowCommand,
    },
    /// Capture ideas (contextual memory, on-demand injection)
    Idea {
        #[command(subcommand)]
        command: IdeaCommand,
    },
    /// Generic contextual memory operations
    Mem {
        #[command(subcommand)]
        command: MemCommand,
    },
    /// Manage contextual memory space (identity split)
    Context {
        #[command(subcommand)]
        command: ContextCommand,
    },
    /// Manage the current work context
    Work {
        #[command(subcommand)]
        command: Option<WorkCommand>,
        #[command(flatten)]
        options: WorkCliOptions,
    },
}

#[derive(Debug, Clone, Args)]
pub struct MemoryCliOptions {
    #[arg(long, value_hint = clap::ValueHint::FilePath)]
    pub socket: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = OutputFormatArg::Tsv)]
    pub format: OutputFormatArg,
    #[arg(long)]
    pub no_start: bool,
}

#[derive(Debug, Clone, Args)]
pub struct WorkCliOptions {
    #[arg(long, global = true, value_hint = clap::ValueHint::FilePath)]
    pub socket: Option<PathBuf>,
    #[arg(long, global = true)]
    pub no_start: bool,
}

#[derive(Subcommand)]
pub enum SmartCommand {
    /// Aggregate recent observations
    Stats {
        #[arg(long, value_enum, default_value_t = OutputFormatArg::Tsv)]
        format: OutputFormatArg,
        #[arg(long, value_hint = clap::ValueHint::FilePath)]
        path: Option<PathBuf>,
        #[arg(long, default_value_t = 1000)]
        limit: usize,
        #[arg(long)]
        since_hours: Option<u64>,
        #[arg(long)]
        session: Option<String>,
    },
    /// Show recent observations without raw user text
    Recent {
        #[arg(long, value_enum, default_value_t = OutputFormatArg::Tsv)]
        format: OutputFormatArg,
        #[arg(long, value_hint = clap::ValueHint::FilePath)]
        path: Option<PathBuf>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        session: Option<String>,
    },
    /// Render a Markdown report for AI evaluation
    Report {
        #[arg(long, value_hint = clap::ValueHint::FilePath)]
        path: Option<PathBuf>,
        #[arg(long, default_value_t = 1000)]
        limit: usize,
        #[arg(long)]
        since_hours: Option<u64>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long, default_value_t = 20)]
        include_recent: usize,
    },
}

#[derive(Subcommand)]
pub enum GoalCommand {
    Set {
        #[arg(required = true, num_args = 1.., allow_hyphen_values = true)]
        text: Vec<String>,
        #[command(flatten)]
        options: MemoryCliOptions,
    },
    Show {
        #[command(flatten)]
        options: MemoryCliOptions,
    },
    Clear {
        #[command(flatten)]
        options: MemoryCliOptions,
    },
}

#[derive(Subcommand)]
pub enum NowCommand {
    Set {
        #[arg(required = true, num_args = 1.., allow_hyphen_values = true)]
        text: Vec<String>,
        #[command(flatten)]
        options: MemoryCliOptions,
    },
    Show {
        #[command(flatten)]
        options: MemoryCliOptions,
    },
    Clear {
        #[command(flatten)]
        options: MemoryCliOptions,
    },
}

#[derive(Subcommand)]
pub enum IdeaCommand {
    Add {
        #[arg(required = true, num_args = 1.., allow_hyphen_values = true)]
        text: Vec<String>,
        #[command(flatten)]
        options: MemoryCliOptions,
    },
    List {
        #[command(flatten)]
        options: MemoryCliOptions,
    },
    Clear {
        #[command(flatten)]
        options: MemoryCliOptions,
    },
}

#[derive(Subcommand)]
pub enum MemCommand {
    Add {
        kind: String,
        #[arg(required = true, num_args = 1.., allow_hyphen_values = true)]
        text: Vec<String>,
        #[command(flatten)]
        options: MemoryCliOptions,
    },
    List {
        #[arg(long)]
        kind: Option<String>,
        #[command(flatten)]
        options: MemoryCliOptions,
    },
    Show {
        /// on-demand idea 解決を含めた prompt block プレビュー用のユーザー query
        #[arg(long)]
        query: Option<String>,
        #[command(flatten)]
        options: MemoryCliOptions,
    },
    Clear {
        kind: String,
        #[command(flatten)]
        options: MemoryCliOptions,
    },
    /// List registered memory kinds from AIBE registry
    Kinds {
        #[command(flatten)]
        options: MemoryCliOptions,
    },
    /// Run a memory recipe by id
    Run {
        recipe: String,
        /// Apply validated proposals after interactive confirmation
        #[arg(long)]
        apply: bool,
        /// Extra instruction passed to the recipe LLM
        #[arg(long)]
        instruction: Option<String>,
        #[command(flatten)]
        options: MemoryCliOptions,
    },
}

#[derive(Subcommand)]
pub enum ContextCommand {
    /// Show current memory space resolution
    Current,
    /// Set current context name (saved to config; `AIBE_CONTEXT_ID` overrides)
    Use { name: String },
    /// Create and switch to a new context name
    New { name: String },
}

#[derive(Subcommand)]
pub enum WorkCommand {
    /// Start a new work context
    Start {
        #[arg(required = true, num_args = 1.., allow_hyphen_values = true)]
        goal: Vec<String>,
    },
    /// Show the current work context
    Status,
    /// List active, paused, deferred, and done works
    List,
    /// Switch active work
    Switch { work_id: u64 },
    /// Start a temporary child work and stack the current work
    Push {
        #[arg(required = true, num_args = 1.., allow_hyphen_values = true)]
        goal: Vec<String>,
    },
    /// Close current child work and return to previous work
    Pop,
    /// Save off-topic work for later without changing active work
    Defer {
        #[arg(required = true, num_args = 1.., allow_hyphen_values = true)]
        text: Vec<String>,
    },
    /// Add an idea to the current work
    Idea {
        #[arg(required = true, num_args = 1.., allow_hyphen_values = true)]
        text: Vec<String>,
    },
    /// Add a note to the current work
    Note {
        #[arg(required = true, num_args = 1.., allow_hyphen_values = true)]
        text: Vec<String>,
    },
    /// Add a decision to the current work
    Decide {
        #[arg(required = true, num_args = 1.., allow_hyphen_values = true)]
        text: Vec<String>,
    },
    /// Update the current focus
    Focus {
        #[arg(required = true, num_args = 1.., allow_hyphen_values = true)]
        text: Vec<String>,
    },
    /// Finish the current work
    Finish,
}

impl AiCli {
    pub fn parse_with_default_ask() -> Result<Self, clap::Error> {
        Self::try_parse_from(normalize_args(std::env::args_os()))
    }

    pub fn normalized_args_for_completion() -> Vec<OsString> {
        normalize_args(std::env::args_os())
    }

    pub fn run_complete(shell: CompleteShell) -> io::Result<()> {
        let mut cmd = command_for_shell_completion();
        let shell_name = match shell {
            CompleteShell::Bash => "bash",
            CompleteShell::Zsh => "zsh",
        };
        match shell {
            CompleteShell::Bash => generate(Bash, &mut cmd, "ai", &mut io::stdout()),
            CompleteShell::Zsh => generate(Zsh, &mut cmd, "ai", &mut io::stdout()),
        }
        if let Some(env) = recall_env_snippet_for_shell(shell_name) {
            print!("{env}");
        }
        if let Some(hook) = recall_hook_for_shell(shell_name) {
            print!("{hook}");
        }
        Ok(())
    }

    pub fn try_complete_env() -> bool {
        CompleteEnv::with_factory(command_for_shell_completion)
            .try_complete(
                normalize_args_for_shell_completion(std::env::args_os()),
                None,
            )
            .unwrap_or(false)
    }
}

/// implicit `ask` 向けに、ルートでも `ask` と同じフラグを補完できるよう Command を拡張する。
fn command_for_shell_completion() -> Command {
    let cmd = AiCli::command();
    let Some(ask_opts) = cmd
        .find_subcommand("ask")
        .map(|ask| ask.get_opts().cloned().collect::<Vec<_>>())
    else {
        return cmd;
    };
    ask_opts.into_iter().fold(cmd, |cmd, arg| cmd.arg(arg))
}

/// 実行時の implicit `ask` 挿入。`ai hello` や bare `ai` を `ai ask` へ正規化する。
fn normalize_args(args: impl IntoIterator<Item = OsString>) -> Vec<OsString> {
    let mut args: Vec<OsString> = args.into_iter().collect();
    if args.len() <= 1 {
        args.push(OsString::from("ask"));
        return args;
    }

    let first = args[1].to_string_lossy();
    if is_known_cli_head(&first) {
        return args;
    }

    args.insert(1, OsString::from("ask"));
    args
}

/// シェル補完用。サブコマンド候補は壊さず、`ai --flag` だけ implicit `ask` へ寄せる。
fn normalize_args_for_shell_completion(args: impl IntoIterator<Item = OsString>) -> Vec<OsString> {
    let mut args: Vec<OsString> = args.into_iter().collect();
    if args.len() <= 1 {
        return args;
    }

    let first = args[1].to_string_lossy();
    if is_known_cli_head(&first) || !first.starts_with('-') {
        return args;
    }

    args.insert(1, OsString::from("ask"));
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn os_vec(parts: &[&str]) -> Vec<OsString> {
        parts.iter().map(|s| OsString::from(*s)).collect()
    }

    #[test]
    fn is_known_cli_head_covers_clap_subcommands() {
        for sub in AiCli::command().get_subcommands() {
            assert!(
                is_known_cli_head(sub.get_name()),
                "add `{}` to domain::is_known_cli_head",
                sub.get_name()
            );
        }
        for flag in ["-h", "--help", "-V", "--version"] {
            assert!(is_known_cli_head(flag), "missing {flag}");
        }
    }

    #[test]
    fn normalize_args_inserts_implicit_ask_for_message() {
        let out = normalize_args(os_vec(&["ai", "hello"]));
        assert_eq!(out, os_vec(&["ai", "ask", "hello"]));
    }

    #[test]
    fn normalize_args_inserts_implicit_ask_for_bare_invocation() {
        let out = normalize_args(os_vec(&["ai"]));
        assert_eq!(out, os_vec(&["ai", "ask"]));
    }

    #[test]
    fn shell_completion_normalizes_bare_long_options_to_ask() {
        let out = normalize_args_for_shell_completion(os_vec(&["ai", "--preset"]));
        assert_eq!(out, os_vec(&["ai", "ask", "--preset"]));
    }

    #[test]
    fn shell_completion_normalizes_bare_short_options_to_ask() {
        let out = normalize_args_for_shell_completion(os_vec(&["ai", "-q"]));
        assert_eq!(out, os_vec(&["ai", "ask", "-q"]));
    }

    #[test]
    fn shell_completion_keeps_top_level_help_and_version() {
        assert_eq!(
            normalize_args_for_shell_completion(os_vec(&["ai", "--help"])),
            os_vec(&["ai", "--help"])
        );
        assert_eq!(
            normalize_args_for_shell_completion(os_vec(&["ai", "-V"])),
            os_vec(&["ai", "-V"])
        );
    }

    #[test]
    fn shell_completion_keeps_subcommand_prefixes() {
        assert_eq!(
            normalize_args_for_shell_completion(os_vec(&["ai", "hist"])),
            os_vec(&["ai", "hist"])
        );
        assert_eq!(
            normalize_args_for_shell_completion(os_vec(&["ai", "history", "--limit"])),
            os_vec(&["ai", "history", "--limit"])
        );
    }

    #[test]
    fn shell_completion_keeps_bare_command_for_subcommand_listing() {
        assert_eq!(
            normalize_args_for_shell_completion(os_vec(&["ai"])),
            os_vec(&["ai"])
        );
    }

    #[test]
    fn command_for_shell_completion_includes_implicit_ask_flags() {
        let cmd = command_for_shell_completion();
        assert!(
            cmd.get_opts().any(|arg| arg.get_long() == Some("preset")),
            "expected ask flags on root command for shell completion"
        );
    }

    #[test]
    fn zsh_completion_script_lists_implicit_ask_flags_at_root() {
        use clap_complete::{generate, shells::Zsh};

        let mut cmd = command_for_shell_completion();
        let mut buf = Vec::new();
        generate(Zsh, &mut cmd, "ai", &mut buf);
        let script = String::from_utf8(buf).expect("utf8");
        let root_section = script.split("case $state in").next().expect("root section");
        assert!(
            root_section.contains("--preset"),
            "zsh root completion should include implicit ask flags: {root_section}"
        );
    }

    #[test]
    fn work_subcommands_parse_successfully() {
        for args in [
            vec!["ai", "work"],
            vec!["ai", "work", "start", "goal"],
            vec!["ai", "work", "status"],
            vec!["ai", "work", "list"],
            vec!["ai", "work", "switch", "1"],
            vec!["ai", "work", "push", "child"],
            vec!["ai", "work", "pop"],
            vec!["ai", "work", "defer", "later"],
            vec!["ai", "work", "idea", "idea"],
            vec!["ai", "work", "note", "note"],
            vec!["ai", "work", "decide", "decision"],
            vec!["ai", "work", "focus", "focus"],
            vec!["ai", "work", "finish"],
            vec!["ai", "work", "status", "--no-start"],
        ] {
            AiCli::try_parse_from(args.clone()).unwrap_or_else(|error| {
                panic!("failed to parse {args:?}: {error}");
            });
        }
    }
}
