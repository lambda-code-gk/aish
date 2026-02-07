use crate::domain::{AiCommand, Query, TaskName};
use common::domain::{ModelName, ProviderName};

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub help: bool,
    /// -c / --continue: 保存された会話状態から再開する
    pub continue_flag: bool,
    pub provider: Option<ProviderName>,
    pub model: Option<ModelName>,
    pub system: Option<String>,
    pub task: Option<TaskName>,
    pub message_args: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            help: false,
            continue_flag: false,
            provider: None,
            model: None,
            system: None,
            task: None,
            message_args: Vec::new(),
        }
    }
}

use common::error::Error;

pub fn parse_args() -> Result<Config, Error> {
    let args: Vec<String> = std::env::args().collect();
    parse_args_from(&args)
}

fn parse_args_from(args: &[String]) -> Result<Config, Error> {
    let mut config = Config::default();
    
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                config.help = true;
                i += 1;
            }
            "-c" | "--continue" => {
                config.continue_flag = true;
                i += 1;
            }
            "-p" | "--provider" => {
                i += 1;
                if i >= args.len() {
                    return Err(Error::invalid_argument("Option -p/--provider requires an argument"));
                }
                config.provider = Some(ProviderName::new(args[i].clone()));
                i += 1;
            }
            "-S" | "--system" => {
                i += 1;
                if i >= args.len() {
                    return Err(Error::invalid_argument("Option -S/--system requires an argument"));
                }
                config.system = Some(args[i].clone());
                i += 1;
            }
            "-m" | "--model" => {
                i += 1;
                if i >= args.len() {
                    return Err(Error::invalid_argument("Option -m/--model requires an argument"));
                }
                config.model = Some(ModelName::new(args[i].clone()));
                i += 1;
            }
            _ if args[i].starts_with('-') => {
                return Err(Error::invalid_argument(format!("Unknown option: {}", args[i])));
            }
            _ => {
                // 位置引数（タスク名とメッセージ引数）
                config.task = Some(TaskName::new(args[i].clone()));
                i += 1;
                // 残りの引数はメッセージ引数として扱う
                while i < args.len() {
                    config.message_args.push(args[i].clone());
                    i += 1;
                }
                break;
            }
        }
    }
    
    Ok(config)
}

/// Config を AiCommand に変換する
pub fn config_to_command(config: Config) -> AiCommand {
    if config.help {
        return AiCommand::Help;
    }

    if config.continue_flag {
        return AiCommand::Resume {
            provider: config.provider,
            model: config.model,
            system: config.system,
        };
    }

    if let Some(task) = config.task {
        let args = config.message_args;
        let provider = config.provider;
        let model = config.model;
        let system = config.system;
        return AiCommand::Task {
            name: task,
            args,
            provider,
            model,
            system,
        };
    }

    let query = Query::new(config.message_args.join(" "));
    AiCommand::Query {
        provider: config.provider,
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
        assert!(!config.continue_flag);
        assert!(config.provider.is_none());
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
        assert_eq!(config.help, false);
        assert_eq!(config.task, None);
        assert_eq!(config.message_args.len(), 0);
    }

    #[test]
    fn test_parse_args_help_short() {
        let args = vec!["ai".to_string(), "-h".to_string()];
        let config = parse_args_from(&args).unwrap();
        assert_eq!(config.help, true);
        assert_eq!(config.task, None);
        assert_eq!(config.message_args.len(), 0);
    }

    #[test]
    fn test_parse_args_help_long() {
        let args = vec!["ai".to_string(), "--help".to_string()];
        let config = parse_args_from(&args).unwrap();
        assert_eq!(config.help, true);
        assert_eq!(config.task, None);
        assert_eq!(config.message_args.len(), 0);
    }

    #[test]
    fn test_parse_args_unknown_option() {
        let args = vec!["ai".to_string(), "--unknown".to_string()];
        let result = parse_args_from(&args);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Unknown option"));
        assert_eq!(err.exit_code(), 64);
    }

    #[test]
    fn test_parse_args_unknown_option_short() {
        let args = vec!["ai".to_string(), "-x".to_string()];
        let result = parse_args_from(&args);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Unknown option"));
        assert_eq!(err.exit_code(), 64);
    }

    #[test]
    fn test_parse_args_task_only() {
        let args = vec!["ai".to_string(), "agent".to_string()];
        let config = parse_args_from(&args).unwrap();
        assert_eq!(config.help, false);
        assert_eq!(config.task.as_ref().map(|t| t.as_ref()), Some("agent"));
        assert_eq!(config.message_args.len(), 0);
    }

    #[test]
    fn test_parse_args_task_with_message() {
        let args = vec!["ai".to_string(), "agent".to_string(), "hello".to_string()];
        let config = parse_args_from(&args).unwrap();
        assert_eq!(config.help, false);
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
        assert_eq!(config.help, false);
        assert_eq!(config.task.as_ref().map(|t| t.as_ref()), Some("agent"));
        assert_eq!(config.message_args.len(), 3);
        assert_eq!(config.message_args[0], "hello");
        assert_eq!(config.message_args[1], "world");
        assert_eq!(config.message_args[2], "test");
    }

    #[test]
    fn test_parse_args_help_with_task() {
        // ヘルプが指定された場合、タスクは無視される（実装による）
        let args = vec!["ai".to_string(), "-h".to_string(), "agent".to_string()];
        let config = parse_args_from(&args).unwrap();
        assert_eq!(config.help, true);
        // 現在の実装では、ヘルプの後にタスクが来ても処理される
        // これは実装の仕様による
    }

    #[test]
    fn test_parse_args_provider() {
        let args = vec!["ai".to_string(), "-p".to_string(), "gemini".to_string()];
        let config = parse_args_from(&args).unwrap();
        assert_eq!(config.provider.as_ref().map(|p| p.as_ref()), Some("gemini"));
    }

    #[test]
    fn test_parse_args_provider_long() {
        let args = vec!["ai".to_string(), "--provider".to_string(), "gpt".to_string()];
        let config = parse_args_from(&args).unwrap();
        assert_eq!(config.provider.as_ref().map(|p| p.as_ref()), Some("gpt"));
    }

    #[test]
    fn test_parse_args_provider_requires_arg() {
        let args = vec!["ai".to_string(), "-p".to_string()];
        let result = parse_args_from(&args);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("requires an argument"));
        assert_eq!(err.exit_code(), 64);
    }

    #[test]
    fn test_parse_args_provider_with_message() {
        let args = vec![
            "ai".to_string(),
            "-p".to_string(), "echo".to_string(),
            "Hello".to_string(),
        ];
        let config = parse_args_from(&args).unwrap();
        assert_eq!(config.provider.as_ref().map(|p| p.as_ref()), Some("echo"));
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
        assert!(err.to_string().contains("requires an argument"));
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
        assert!(err.to_string().contains("requires an argument"));
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
}

