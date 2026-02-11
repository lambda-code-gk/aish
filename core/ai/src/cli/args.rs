use crate::domain::{AiCommand, Query, TaskName};
use clap::builder::ArgAction;
use clap::value_parser;
use clap_complete::Shell;
use common::domain::{ModelName, ProviderName};
use common::error::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub help: bool,
    /// -L / --list-profiles: 現在有効なプロファイル一覧を表示
    pub list_profiles: bool,
    /// --list-tools: 指定プロバイダで有効なツール一覧を表示（-p と併用でプロバイダ指定）
    pub list_tools: bool,
    /// -c / --continue: 保存された会話状態から再開する
    pub continue_flag: bool,
    /// --no-interactive: 確認プロンプトを出さず CI 等でブロックしない（承認は常に拒否・続行はしない・leakscan ヒットは拒否）
    pub non_interactive: bool,
    /// -v / --verbose: 不具合調査用の冗長ログを stderr 等に出力する
    pub verbose: bool,
    pub profile: Option<ProviderName>,
    pub model: Option<ModelName>,
    pub system: Option<String>,
    pub task: Option<TaskName>,
    pub message_args: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            help: false,
            list_profiles: false,
            list_tools: false,
            continue_flag: false,
            non_interactive: false,
            verbose: false,
            profile: None,
            model: None,
            system: None,
            task: None,
            message_args: Vec::new(),
        }
    }
}

/// 解析結果: 通常の Config / 補完スクリプト生成 / タスク一覧表示
#[derive(Debug, Clone)]
pub enum ParseOutcome {
    Config(Config),
    GenerateCompletion(Shell),
    /// --list-tasks が指定された（main でタスク一覧を表示して終了）
    ListTasks,
}

fn build_clap_command() -> clap::Command {
    clap::Command::new("ai")
        .about("Send a message to the LLM or run a task script")
        .disable_help_flag(true)
        .arg(
            clap::Arg::new("help")
                .short('h')
                .long("help")
                .help("Show this help message")
                .action(ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("list-profiles")
                .short('L')
                .long("list-profiles")
                .help("List currently available provider profiles")
                .action(ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("list-tools")
                .long("list-tools")
                .help("List tools enabled for the given profile (use with -p/--profile, e.g. -p echo)")
                .action(ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("continue")
                .short('c')
                .long("continue")
                .help("Resume the agent loop from the last saved state")
                .action(ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("no-interactive")
                .long("no-interactive")
                .help("Do not prompt for confirmations (CI-friendly: tool approval denied, no continue, leakscan deny)")
                .action(ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("verbose")
                .short('v')
                .long("verbose")
                .help("Emit verbose debug logs to stderr (for troubleshooting)")
                .action(ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("profile")
                .short('p')
                .long("profile")
                .value_name("profile")
                .help("Specify LLM profile (gemini, gpt, echo, etc.)")
                .num_args(1),
        )
        .arg(
            clap::Arg::new("system")
                .short('S')
                .long("system")
                .value_name("instruction")
                .help("Set system instruction for this query")
                .num_args(1),
        )
        .arg(
            clap::Arg::new("model")
                .short('m')
                .long("model")
                .value_name("model")
                .help("Specify model name (e.g. gemini-2.0, gpt-4)")
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
        .arg(
            clap::Arg::new("list-tasks")
                .long("list-tasks")
                .help("List available task names (for completion)")
                .action(ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("positional")
                .index(1)
                .help("Task name then message/args, or just message words")
                .num_args(0..)
                .trailing_var_arg(true),
        )
}

fn matches_to_config(matches: &clap::ArgMatches) -> Config {
    let help = matches.get_flag("help");
    let list_profiles = matches.get_flag("list-profiles");
    let list_tools = matches.get_flag("list-tools");
    let continue_flag = matches.get_flag("continue");
    let non_interactive = matches.get_flag("no-interactive");
    let verbose = matches.get_flag("verbose");
    let profile = matches
        .get_one::<String>("profile")
        .map(|s| ProviderName::new(s.clone()));
    let model = matches
        .get_one::<String>("model")
        .map(|s| ModelName::new(s.clone()));
    let system = matches.get_one::<String>("system").cloned();
    let positional: Vec<String> = matches
        .get_many::<String>("positional")
        .map(|i| i.cloned().collect())
        .unwrap_or_default();
    let (task, message_args) = match positional.split_first() {
        Some((first, rest)) => (
            Some(TaskName::new(first.clone())),
            rest.to_vec(),
        ),
        None => (None, vec![]),
    };

    Config {
        help,
        list_profiles,
        list_tools,
        continue_flag,
        non_interactive,
        verbose,
        profile,
        model,
        system,
        task,
        message_args,
    }
}

/// コマンドラインを解析する。補完生成が要求された場合は ParseOutcome::GenerateCompletion を返す。
pub fn parse_args() -> Result<ParseOutcome, Error> {
    let cmd = build_clap_command();
    let matches = cmd
        .try_get_matches()
        .map_err(|e| Error::invalid_argument(e.to_string()))?;

    if let Some(&shell) = matches.get_one::<Shell>("generate") {
        return Ok(ParseOutcome::GenerateCompletion(shell));
    }

    if matches.get_flag("list-tasks") {
        return Ok(ParseOutcome::ListTasks);
    }

    Ok(ParseOutcome::Config(matches_to_config(&matches)))
}

/// テスト用: 引数スライスから解析する
#[allow(dead_code)]
pub fn parse_args_from(args: &[String]) -> Result<Config, Error> {
    let cmd = build_clap_command();
    let matches = cmd
        .try_get_matches_from(args)
        .map_err(|e| Error::invalid_argument(e.to_string()))?;
    Ok(matches_to_config(&matches))
}

/// 補完スクリプトを標準出力に出力する。
pub fn print_completion(shell: Shell) {
    emit_fallback_completion(shell);
}

fn emit_fallback_completion(shell: Shell) {
    let opts = "-h --help -L --list-profiles -c --continue --no-interactive -v --verbose -p --profile -S --system -m --model --generate --list-tasks";
    match shell {
        Shell::Bash => {
            println!(
                r#"# Fallback completion for ai (options + task names via ai --list-tasks)
_ai() {{
  local cur="${{COMP_WORDS[COMP_CWORD]}}"
  local tasks
  tasks=$(ai --list-tasks 2>/dev/null)
  COMPREPLY=($(compgen -W "$tasks {opts}" -- "$cur"))
}}
complete -F _ai ai
"#,
                opts = opts
            );
        }
        Shell::Zsh => {
            println!(
                r#"# Fallback completion for ai (options + task names via ai --list-tasks)
#compdef ai
local -a reply
reply=($(ai --list-tasks 2>/dev/null) {opts})
_describe 'ai' reply
"#,
                opts = opts
            );
        }
        Shell::Fish => {
            println!(
                r#"# Fallback completion for ai (options + task names)
complete -c ai -l help -s h -d "Show help"
complete -c ai -l list-profiles -s L -d "List profiles"
complete -c ai -l continue -s c -d "Resume session"
complete -c ai -l no-interactive -d "Do not prompt (CI-friendly)"
complete -c ai -l profile -s p -d "LLM profile" -r
complete -c ai -l system -s S -d "System instruction" -r
complete -c ai -l model -s m -d "Model name" -r
complete -c ai -l generate -d "Generate completion script" -r -a "bash zsh fish"
complete -c ai -l list-tasks -d "List task names"
complete -c ai -a "(ai --list-tasks 2>/dev/null)"
"#
            );
        }
        _ => {}
    }
}

/// Config を AiCommand に変換する
pub fn config_to_command(config: Config) -> AiCommand {
    if config.help {
        return AiCommand::Help;
    }

    if config.list_profiles {
        return AiCommand::ListProfiles;
    }

    if config.list_tools {
        return AiCommand::ListTools {
            profile: config.profile,
        };
    }

    if config.continue_flag {
        return AiCommand::Resume {
            profile: config.profile,
            model: config.model,
            system: config.system,
        };
    }

    if let Some(task) = config.task {
        let args = config.message_args;
        let profile = config.profile;
        let model = config.model;
        let system = config.system;
        return AiCommand::Task {
            name: task,
            args,
            profile,
            model,
            system,
        };
    }

    let query = Query::new(config.message_args.join(" "));
    AiCommand::Query {
        profile: config.profile,
        model: config.model,
        query,
        system: config.system,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert!(!config.help);
        assert!(!config.list_profiles);
        assert!(!config.continue_flag);
        assert!(!config.verbose);
        assert!(config.profile.is_none());
        assert!(config.model.is_none());
        assert!(config.system.is_none());
        assert!(config.task.is_none());
        assert_eq!(config.message_args.len(), 0);
    }

    #[test]
    fn test_config_with_task() {
        let mut config = Config::default();
        config.task = Some(TaskName::new("agent"));
        config.message_args.push("hello".to_string());
        assert_eq!(config.task.as_ref().map(|t| t.as_ref()), Some("agent"));
        assert_eq!(config.message_args.len(), 1);
        assert_eq!(config.message_args[0], "hello");
    }

    #[test]
    fn test_parse_args_no_args() {
        let args = vec!["ai".to_string()];
        let config = parse_args_from(&args).unwrap();
        assert!(!config.help);
        assert_eq!(config.task, None);
        assert_eq!(config.message_args.len(), 0);
    }

    #[test]
    fn test_parse_args_help_short() {
        let args = vec!["ai".to_string(), "-h".to_string()];
        let config = parse_args_from(&args).unwrap();
        assert!(config.help);
        assert_eq!(config.task, None);
        assert_eq!(config.message_args.len(), 0);
    }

    #[test]
    fn test_parse_args_help_long() {
        let args = vec!["ai".to_string(), "--help".to_string()];
        let config = parse_args_from(&args).unwrap();
        assert!(config.help);
        assert_eq!(config.task, None);
        assert_eq!(config.message_args.len(), 0);
    }

    #[test]
    fn test_parse_args_unknown_option() {
        let args = vec!["ai".to_string(), "--unknown".to_string()];
        let result = parse_args_from(&args);
        assert!(result.is_err(), "unknown long option must be rejected");
        let err = result.unwrap_err();
        assert_eq!(err.exit_code(), 64);
    }

    #[test]
    fn test_parse_args_unknown_option_short() {
        let args = vec!["ai".to_string(), "-x".to_string()];
        let result = parse_args_from(&args);
        assert!(result.is_err(), "unknown short option -x must be rejected");
        let err = result.unwrap_err();
        assert_eq!(err.exit_code(), 64);
    }

    #[test]
    fn test_parse_args_task_only() {
        let args = vec!["ai".to_string(), "agent".to_string()];
        let config = parse_args_from(&args).unwrap();
        assert!(!config.help);
        assert_eq!(config.task.as_ref().map(|t| t.as_ref()), Some("agent"));
        assert_eq!(config.message_args.len(), 0);
    }

    #[test]
    fn test_parse_args_task_with_message() {
        let args = vec!["ai".to_string(), "agent".to_string(), "hello".to_string()];
        let config = parse_args_from(&args).unwrap();
        assert!(!config.help);
        assert_eq!(config.task.as_ref().map(|t| t.as_ref()), Some("agent"));
        assert_eq!(config.message_args.len(), 1);
        assert_eq!(config.message_args[0], "hello");
    }

    #[test]
    fn test_parse_args_task_with_multiple_messages() {
        let args = vec![
            "ai".to_string(),
            "agent".to_string(),
            "hello".to_string(),
            "world".to_string(),
            "test".to_string(),
        ];
        let config = parse_args_from(&args).unwrap();
        assert!(!config.help);
        assert_eq!(config.task.as_ref().map(|t| t.as_ref()), Some("agent"));
        assert_eq!(config.message_args.len(), 3);
        assert_eq!(config.message_args[0], "hello");
        assert_eq!(config.message_args[1], "world");
        assert_eq!(config.message_args[2], "test");
    }

    #[test]
    fn test_parse_args_help_with_task() {
        let args = vec!["ai".to_string(), "-h".to_string(), "agent".to_string()];
        let config = parse_args_from(&args).unwrap();
        assert!(config.help);
    }

    #[test]
    fn test_parse_args_profile() {
        let args = vec!["ai".to_string(), "-p".to_string(), "gemini".to_string()];
        let config = parse_args_from(&args).unwrap();
        assert_eq!(config.profile.as_ref().map(|p| p.as_ref()), Some("gemini"));
    }

    #[test]
    fn test_parse_args_profile_long() {
        let args = vec!["ai".to_string(), "--profile".to_string(), "gpt".to_string()];
        let config = parse_args_from(&args).unwrap();
        assert_eq!(config.profile.as_ref().map(|p| p.as_ref()), Some("gpt"));
    }

    #[test]
    fn test_parse_args_profile_requires_arg() {
        let args = vec!["ai".to_string(), "-p".to_string()];
        let result = parse_args_from(&args);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("argument") || err.to_string().contains("required"));
        assert_eq!(err.exit_code(), 64);
    }

    #[test]
    fn test_parse_args_profile_with_message() {
        let args = vec![
            "ai".to_string(),
            "-p".to_string(),
            "echo".to_string(),
            "Hello".to_string(),
        ];
        let config = parse_args_from(&args).unwrap();
        assert_eq!(config.profile.as_ref().map(|p| p.as_ref()), Some("echo"));
        assert_eq!(config.task.as_ref().map(|t| t.as_ref()), Some("Hello"));
    }

    #[test]
    fn test_parse_args_system() {
        let args = vec![
            "ai".to_string(),
            "-S".to_string(),
            "You are helpful.".to_string(),
            "Hello".to_string(),
        ];
        let config = parse_args_from(&args).unwrap();
        assert_eq!(config.system.as_deref(), Some("You are helpful."));
        assert_eq!(config.task.as_ref().map(|t| t.as_ref()), Some("Hello"));
    }

    #[test]
    fn test_parse_args_system_long() {
        let args = vec![
            "ai".to_string(),
            "--system".to_string(),
            "Answer in Japanese.".to_string(),
            "こんにちは".to_string(),
        ];
        let config = parse_args_from(&args).unwrap();
        assert_eq!(config.system.as_deref(), Some("Answer in Japanese."));
        assert_eq!(config.task.as_ref().map(|t| t.as_ref()), Some("こんにちは"));
    }

    #[test]
    fn test_parse_args_system_requires_arg() {
        let args = vec!["ai".to_string(), "--system".to_string()];
        let result = parse_args_from(&args);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("argument") || err.to_string().contains("required"));
        assert_eq!(err.exit_code(), 64);
    }

    #[test]
    fn test_parse_args_model_short() {
        let args = vec!["ai".to_string(), "-m".to_string(), "gemini-2.0".to_string()];
        let config = parse_args_from(&args).unwrap();
        assert_eq!(config.model.as_ref().map(|m| m.as_ref()), Some("gemini-2.0"));
    }

    #[test]
    fn test_parse_args_model_long() {
        let args = vec!["ai".to_string(), "--model".to_string(), "gpt-4".to_string()];
        let config = parse_args_from(&args).unwrap();
        assert_eq!(config.model.as_ref().map(|m| m.as_ref()), Some("gpt-4"));
    }

    #[test]
    fn test_parse_args_model_requires_arg() {
        let args = vec!["ai".to_string(), "-m".to_string()];
        let result = parse_args_from(&args);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("argument") || err.to_string().contains("required"));
        assert_eq!(err.exit_code(), 64);
    }

    #[test]
    fn test_parse_args_continue_short() {
        let args = vec!["ai".to_string(), "-c".to_string()];
        let config = parse_args_from(&args).unwrap();
        assert!(config.continue_flag);
        assert!(config.task.is_none());
        assert_eq!(config.message_args.len(), 0);
    }

    #[test]
    fn test_parse_args_continue_long() {
        let args = vec!["ai".to_string(), "--continue".to_string()];
        let config = parse_args_from(&args).unwrap();
        assert!(config.continue_flag);
    }

    #[test]
    fn test_config_to_command_continue_returns_resume() {
        let config = Config {
            continue_flag: true,
            ..Default::default()
        };
        let cmd = config_to_command(config);
        assert!(matches!(cmd, AiCommand::Resume { .. }));
    }

    #[test]
    fn test_parse_args_list_profiles_short() {
        let args = vec!["ai".to_string(), "-L".to_string()];
        let config = parse_args_from(&args).unwrap();
        assert!(config.list_profiles);
    }

    #[test]
    fn test_parse_args_list_profiles_long() {
        let args = vec!["ai".to_string(), "--list-profiles".to_string()];
        let config = parse_args_from(&args).unwrap();
        assert!(config.list_profiles);
    }

    #[test]
    fn test_parse_args_no_interactive() {
        let args = vec!["ai".to_string(), "--no-interactive".to_string(), "hello".to_string()];
        let config = parse_args_from(&args).unwrap();
        assert!(config.non_interactive);
    }

    #[test]
    fn test_parse_args_verbose_short() {
        let args = vec!["ai".to_string(), "-v".to_string(), "hello".to_string()];
        let config = parse_args_from(&args).unwrap();
        assert!(config.verbose);
    }

    #[test]
    fn test_parse_args_verbose_long() {
        let args = vec!["ai".to_string(), "--verbose".to_string()];
        let config = parse_args_from(&args).unwrap();
        assert!(config.verbose);
    }

    #[test]
    fn test_config_to_command_list_profiles_returns_list_profiles() {
        let config = Config {
            list_profiles: true,
            ..Default::default()
        };
        let cmd = config_to_command(config);
        assert!(matches!(cmd, AiCommand::ListProfiles));
    }

    #[test]
    fn test_config_to_command_list_tools_returns_list_tools() {
        let config = Config {
            list_tools: true,
            profile: Some(ProviderName::new("echo".to_string())),
            ..Default::default()
        };
        let cmd = config_to_command(config);
        assert!(matches!(cmd, AiCommand::ListTools { profile: Some(_) }));
    }
}
