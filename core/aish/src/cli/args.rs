use crate::domain::command::Command;

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

use common::error::Error;

pub fn parse_args() -> Result<Config, Error> {
    let args: Vec<String> = std::env::args().collect();
    let mut config = Config::default();
    
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                config.help = true;
                i += 1;
            }
            "-s" | "--session-dir" => {
                i += 1;
                if i >= args.len() {
                    return Err(Error::invalid_argument("Option -s/--session-dir requires an argument"));
                }
                config.session_dir = Some(args[i].clone());
                i += 1;
            }
            "-d" | "--home-dir" => {
                i += 1;
                if i >= args.len() {
                    return Err(Error::invalid_argument("Option -d/--home-dir requires an argument"));
                }
                config.home_dir = Some(args[i].clone());
                i += 1;
            }
            _ if args[i].starts_with('-') => {
                return Err(Error::invalid_argument(format!("Unknown option: {}", args[i])));
            }
            _ => {
                // 位置引数（コマンドとその引数）
                config.command_name = Some(args[i].clone());
                i += 1;
                // 残りの引数はコマンドの引数として扱う
                while i < args.len() {
                    config.command_args.push(args[i].clone());
                    i += 1;
                }
                break;
            }
        }
    }

    Ok(config)
}

/// Config を Command に変換する
pub fn config_to_command(config: &Config) -> Command {
    if config.help {
        return Command::Help;
    }
    match &config.command_name {
        Some(name) => Command::parse(name),
        None => Command::Shell,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_help_flag() {
        // 環境変数を直接変更できないため、実際のテストは統合テストで行う
        // ここでは基本的な構造のテストのみ
        let config = Config::default();
        assert_eq!(config.help, false);
    }

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.help, false);
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

