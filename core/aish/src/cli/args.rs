use crate::domain::command::Command;
use clap::builder::ArgAction;
use clap::value_parser;
use clap_complete::Shell;
use common::error::Error;

/// CLI から受け取った生の設定（command は文字列のまま保持）
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub help: bool,
    pub session_dir: Option<String>,
    pub home_dir: Option<String>,
    /// -v / --verbose: 不具合調査用の冗長ログを出力する
    pub verbose: bool,
    /// コマンド名（None の場合は Shell）
    pub command_name: Option<String>,
    pub command_args: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            help: false,
            session_dir: None,
            home_dir: None,
            verbose: false,
            command_name: None,
            command_args: Vec::new(),
        }
    }
}

/// 解析結果: 通常の Config または補完スクリプト生成
#[derive(Debug, Clone)]
pub enum ParseOutcome {
    Config(Config),
    GenerateCompletion(Shell),
}

fn global_args(cmd: clap::Command) -> clap::Command {
    cmd.disable_help_flag(true)
    .arg(
        clap::Arg::new("help")
            .short('h')
            .long("help")
            .help("Print help")
            .action(ArgAction::SetTrue),
    )
    .arg(
        clap::Arg::new("session-dir")
            .short('s')
            .long("session-dir")
            .value_name("directory")
            .help("Specify a session directory (for resume)")
            .num_args(1),
    )
    .arg(
        clap::Arg::new("home-dir")
            .short('d')
            .long("home-dir")
            .value_name("directory")
            .help("Specify a home directory (sets AISH_HOME for this process)")
            .num_args(1),
    )
    .arg(
        clap::Arg::new("verbose")
            .short('v')
            .long("verbose")
            .help("Emit verbose debug logs (for troubleshooting)")
            .action(ArgAction::SetTrue),
    )
    .arg(
        clap::Arg::new("generate")
            .long("generate")
            .value_name("shell")
            .help("Generate shell completion script")
            .value_parser(value_parser!(Shell))
            .num_args(1),
    )
}

fn build_sysq_subcommand() -> clap::Command {
    clap::Command::new("sysq")
        .about("System prompt (sysq) list / enable / disable")
        .subcommand_required(true)
        .subcommand(clap::Command::new("list").about("List system prompts and their enabled state"))
        .subcommand(
            clap::Command::new("enable")
                .about("Enable system prompt(s)")
                .arg(clap::Arg::new("ids").num_args(0..).value_name("id")),
        )
        .subcommand(
            clap::Command::new("disable")
                .about("Disable system prompt(s)")
                .arg(clap::Arg::new("ids").num_args(0..).value_name("id")),
        )
}

fn build_memory_subcommand() -> clap::Command {
    clap::Command::new("memory")
        .about("Memory list / get / remove (persistent knowledge used by ai)")
        .subcommand_required(true)
        .subcommand(clap::Command::new("list").about("List all memories (id, category, subject)"))
        .subcommand(
            clap::Command::new("get")
                .about("Get memory content by ID(s)")
                .arg(clap::Arg::new("ids").num_args(1..).value_name("id").required(true)),
        )
        .subcommand(
            clap::Command::new("remove")
                .about("Remove memory by ID(s)")
                .arg(clap::Arg::new("ids").num_args(1..).value_name("id").required(true)),
        )
}

fn build_clap_command() -> clap::Command {
    let sysq = build_sysq_subcommand();
    let memory = build_memory_subcommand();

    global_args(
        clap::Command::new("aish")
            .about("CUI automation framework with LLM integration")
            .subcommand_required(false)
            .subcommand(clap::Command::new("shell").about("Start the interactive shell (default)"))
            .subcommand(clap::Command::new("help").about("Display this help message"))
            .subcommand(
                clap::Command::new("truncate_console_log")
                    .about("Truncate console buffer and log file (used by ai command)"),
            )
            .subcommand(
                clap::Command::new("rollout")
                    .about("Flush console buffer and rollover console log (SIGUSR1 equivalent)"),
            )
            .subcommand(
                clap::Command::new("mute")
                    .about("Rollout console log and stop recording console.txt"),
            )
            .subcommand(
                clap::Command::new("unmute")
                    .about("Resume recording console.txt"),
            )
            .subcommand(
                clap::Command::new("clear")
                    .about("Clear all part files in the session directory (delete conversation history)"),
            )
            .subcommand(sysq)
            .subcommand(memory)
            .subcommand(
                clap::Command::new("resume")
                    .about("Resume last or specified session")
                    .arg(
                        clap::Arg::new("id")
                            .value_name("id")
                            .help("Session id to resume (omit for latest)")
                            .num_args(0..=1),
                    ),
            )
            .subcommand(clap::Command::new("sessions").about("List sessions")),
    )
}

fn matches_to_config(matches: &clap::ArgMatches) -> Config {
    let help = matches.get_flag("help") || matches.subcommand_matches("help").is_some();
    let session_dir = matches
        .get_one::<String>("session-dir")
        .cloned();
    let home_dir = matches.get_one::<String>("home-dir").cloned();
    let verbose = matches.get_flag("verbose");

    let (command_name, command_args) = match matches.subcommand() {
        None => (None, Vec::new()),
        Some(("help", _)) => (None, Vec::new()),
        Some(("shell", _)) => (None, Vec::new()),
        Some(("truncate_console_log", _)) => (Some("truncate_console_log".to_string()), vec![]),
        Some(("rollout", _)) => (Some("rollout".to_string()), vec![]),
        Some(("mute", _)) => (Some("mute".to_string()), vec![]),
        Some(("unmute", _)) => (Some("unmute".to_string()), vec![]),
        Some(("clear", _)) => (Some("clear".to_string()), vec![]),
        Some(("resume", m)) => {
            let id = m.get_one::<String>("id").cloned();
            let args = id.into_iter().collect::<Vec<_>>();
            (Some("resume".to_string()), args)
        }
        Some(("sysq", sysq_m)) => {
            let (sub, args) = match sysq_m.subcommand() {
                Some(("list", _)) => ("list", vec![]),
                Some(("enable", m)) => (
                    "enable",
                    m.get_many::<String>("ids")
                        .map(|i| i.cloned().collect())
                        .unwrap_or_default(),
                ),
                Some(("disable", m)) => (
                    "disable",
                    m.get_many::<String>("ids")
                        .map(|i| i.cloned().collect())
                        .unwrap_or_default(),
                ),
                _ => ("", vec![]),
            };
            let mut command_args = vec![sub.to_string()];
            command_args.extend(args);
            (Some("sysq".to_string()), command_args)
        }
        Some(("memory", memory_m)) => {
            let (sub, args) = match memory_m.subcommand() {
                Some(("list", _)) => ("list", vec![]),
                Some(("get", m)) => (
                    "get",
                    m.get_many::<String>("ids")
                        .map(|i| i.cloned().collect())
                        .unwrap_or_default(),
                ),
                Some(("remove", m)) => (
                    "remove",
                    m.get_many::<String>("ids")
                        .map(|i| i.cloned().collect())
                        .unwrap_or_default(),
                ),
                _ => ("", vec![]),
            };
            let mut command_args = vec![sub.to_string()];
            command_args.extend(args);
            (Some("memory".to_string()), command_args)
        }
        Some((name, _)) => (Some(name.to_string()), vec![]),
    };

    Config {
        help,
        session_dir,
        home_dir,
        verbose,
        command_name,
        command_args,
    }
}

/// コマンドラインを解析する。補完生成が要求された場合は ParseOutcome::GenerateCompletion を返す。
pub fn parse_args() -> Result<ParseOutcome, Error> {
    let cmd = build_clap_command();
    let matches = cmd.try_get_matches().map_err(|e| {
        Error::invalid_argument(e.to_string())
    })?;

    if let Some(&shell) = matches.get_one::<Shell>("generate") {
        return Ok(ParseOutcome::GenerateCompletion(shell));
    }

    Ok(ParseOutcome::Config(matches_to_config(&matches)))
}

/// 補完スクリプトを標準出力に出力する。
/// 注: clap_complete::generate は当コマンド構成でパニックするため、簡易フォールバックを常に使用する。
pub fn print_completion(shell: Shell) {
    emit_fallback_completion(shell);
}

fn emit_fallback_completion(shell: Shell) {
    let subcommands = [
        "clear", "help", "memory", "resume", "rollout", "sessions", "shell", "sysq", "truncate_console_log",
    ];
    match shell {
        Shell::Bash => {
            println!(
                r#"# Fallback completion for aish (subcommands only)
_aish() {{
  local cur="${{COMP_WORDS[COMP_CWORD]}}"
  COMPREPLY=($(compgen -W "{}" -- "$cur"))
}}
complete -F _aish aish
"#,
                subcommands.join(" ")
            );
        }
        Shell::Zsh => {
            println!(
                r#"# Fallback completion for aish (subcommands only)
#compdef aish
local subcommands
subcommands=({})
_describe 'command' subcommands
"#,
                subcommands.iter().map(|s| format!("\"{}\"", s)).collect::<Vec<_>>().join(" ")
            );
        }
        Shell::Fish => {
            println!(
                r#"# Fallback completion for aish (subcommands only)
complete -c aish -a "{}"
"#,
                subcommands.join(" ")
            );
        }
        _ => {}
    }
}

/// Config を Command に変換する
pub fn config_to_command(config: &Config) -> Command {
    if config.help {
        return Command::Help;
    }
    match &config.command_name {
        Some(name) => Command::parse_with_args(name, &config.command_args),
        None => Command::Shell,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_help_flag() {
        let config = Config::default();
        assert!(!config.help);
    }

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert!(!config.help);
        assert_eq!(config.session_dir, None);
        assert_eq!(config.home_dir, None);
        assert!(!config.verbose);
        assert_eq!(config.command_name, None);
        assert_eq!(config.command_args.len(), 0);
    }

    #[test]
    fn test_config_to_command_default_is_shell() {
        let config = Config::default();
        assert_eq!(config_to_command(&config), Command::Shell);
    }

    #[test]
    fn test_config_to_command_help() {
        let config = Config {
            help: true,
            ..Default::default()
        };
        assert_eq!(config_to_command(&config), Command::Help);
    }

    #[test]
    fn test_config_to_command_with_command_name() {
        let config = Config {
            command_name: Some("truncate_console_log".to_string()),
            ..Default::default()
        };
        assert_eq!(config_to_command(&config), Command::TruncateConsoleLog);
    }

    #[test]
    fn test_config_to_command_with_mute() {
        let config = Config {
            command_name: Some("mute".to_string()),
            ..Default::default()
        };
        assert_eq!(config_to_command(&config), Command::Mute);
    }

    #[test]
    fn test_config_to_command_with_unmute() {
        let config = Config {
            command_name: Some("unmute".to_string()),
            ..Default::default()
        };
        assert_eq!(config_to_command(&config), Command::Unmute);
    }
}
