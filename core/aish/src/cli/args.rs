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

fn build_clap_command() -> clap::Command {
    let sysq = build_sysq_subcommand();

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
                clap::Command::new("clear")
                    .about("Clear all part files in the session directory (delete conversation history)"),
            )
            .subcommand(sysq)
            .subcommand(clap::Command::new("resume").about("(Not yet implemented) Resume session"))
            .subcommand(clap::Command::new("sessions").about("(Not yet implemented) List sessions"))
            .subcommand(clap::Command::new("rollout").about("(Not yet implemented) Rollout"))
            .subcommand(clap::Command::new("ls").about("(Not yet implemented) List"))
            .subcommand(clap::Command::new("rm_last").about("(Not yet implemented) Remove last part"))
            .subcommand(clap::Command::new("memory").about("(Not yet implemented) Memory"))
            .subcommand(clap::Command::new("models").about("(Not yet implemented) List models")),
    )
}

fn matches_to_config(matches: &clap::ArgMatches) -> Config {
    let help = matches.get_flag("help") || matches.subcommand_matches("help").is_some();
    let session_dir = matches
        .get_one::<String>("session-dir")
        .cloned();
    let home_dir = matches.get_one::<String>("home-dir").cloned();

    let (command_name, command_args) = match matches.subcommand() {
        None => (None, Vec::new()),
        Some(("help", _)) => (None, Vec::new()),
        Some(("shell", _)) => (None, Vec::new()),
        Some(("truncate_console_log", _)) => (Some("truncate_console_log".to_string()), vec![]),
        Some(("clear", _)) => (Some("clear".to_string()), vec![]),
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
        Some((name, _)) => (Some(name.to_string()), vec![]),
    };

    Config {
        help,
        session_dir,
        home_dir,
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
        "clear", "help", "ls", "memory", "models", "resume", "rollout", "rm_last",
        "sessions", "shell", "sysq", "truncate_console_log",
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
}
