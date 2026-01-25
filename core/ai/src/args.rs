#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub help: bool,
    pub provider: Option<String>,
    pub task: Option<String>,
    pub message_args: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            help: false,
            provider: None,
            task: None,
            message_args: Vec::new(),
        }
    }
}

pub fn parse_args() -> Result<Config, (String, i32)> {
    let args: Vec<String> = std::env::args().collect();
    parse_args_from(&args)
}

fn parse_args_from(args: &[String]) -> Result<Config, (String, i32)> {
    let mut config = Config::default();
    
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                config.help = true;
                i += 1;
            }
            "-p" | "--provider" => {
                i += 1;
                if i >= args.len() {
                    return Err(("Option -p/--provider requires an argument".to_string(), 64));
                }
                config.provider = Some(args[i].clone());
                i += 1;
            }
            _ if args[i].starts_with('-') => {
                return Err((format!("Unknown option: {}", args[i]), 64));
            }
            _ => {
                // 位置引数（タスク名とメッセージ引数）
                config.task = Some(args[i].clone());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.help, false);
        assert_eq!(config.provider, None);
        assert_eq!(config.task, None);
        assert_eq!(config.message_args.len(), 0);
    }

    #[test]
    fn test_config_with_task() {
        let mut config = Config::default();
        config.task = Some("agent".to_string());
        config.message_args.push("hello".to_string());
        assert_eq!(config.task, Some("agent".to_string()));
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
        let (msg, code) = result.unwrap_err();
        assert!(msg.contains("Unknown option"));
        assert_eq!(code, 64);
    }

    #[test]
    fn test_parse_args_unknown_option_short() {
        let args = vec!["ai".to_string(), "-x".to_string()];
        let result = parse_args_from(&args);
        assert!(result.is_err());
        let (msg, code) = result.unwrap_err();
        assert!(msg.contains("Unknown option"));
        assert_eq!(code, 64);
    }

    #[test]
    fn test_parse_args_task_only() {
        let args = vec!["ai".to_string(), "agent".to_string()];
        let config = parse_args_from(&args).unwrap();
        assert_eq!(config.help, false);
        assert_eq!(config.task, Some("agent".to_string()));
        assert_eq!(config.message_args.len(), 0);
    }

    #[test]
    fn test_parse_args_task_with_message() {
        let args = vec!["ai".to_string(), "agent".to_string(), "hello".to_string()];
        let config = parse_args_from(&args).unwrap();
        assert_eq!(config.help, false);
        assert_eq!(config.task, Some("agent".to_string()));
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
        assert_eq!(config.task, Some("agent".to_string()));
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
        assert_eq!(config.provider, Some("gemini".to_string()));
    }

    #[test]
    fn test_parse_args_provider_long() {
        let args = vec!["ai".to_string(), "--provider".to_string(), "gpt".to_string()];
        let config = parse_args_from(&args).unwrap();
        assert_eq!(config.provider, Some("gpt".to_string()));
    }

    #[test]
    fn test_parse_args_provider_requires_arg() {
        let args = vec!["ai".to_string(), "-p".to_string()];
        let result = parse_args_from(&args);
        assert!(result.is_err());
        let (msg, code) = result.unwrap_err();
        assert!(msg.contains("requires an argument"));
        assert_eq!(code, 64);
    }

    #[test]
    fn test_parse_args_provider_with_message() {
        let args = vec![
            "ai".to_string(),
            "-p".to_string(), "echo".to_string(),
            "Hello".to_string(),
        ];
        let config = parse_args_from(&args).unwrap();
        assert_eq!(config.provider, Some("echo".to_string()));
        assert_eq!(config.task, Some("Hello".to_string()));
    }
}

