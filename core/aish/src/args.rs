#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub help: bool,
    pub session_dir: Option<String>,
    pub home_dir: Option<String>,
    pub command: Option<String>,
    pub command_args: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            help: false,
            session_dir: None,
            home_dir: None,
            command: None,
            command_args: Vec::new(),
        }
    }
}

pub fn parse_args() -> Result<Config, (String, i32)> {
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
                    return Err(("Option -s/--session-dir requires an argument".to_string(), 64));
                }
                config.session_dir = Some(args[i].clone());
                i += 1;
            }
            "-d" | "--home-dir" => {
                i += 1;
                if i >= args.len() {
                    return Err(("Option -d/--home-dir requires an argument".to_string(), 64));
                }
                config.home_dir = Some(args[i].clone());
                i += 1;
            }
            _ if args[i].starts_with('-') => {
                return Err((format!("Unknown option: {}", args[i]), 64));
            }
            _ => {
                // 位置引数（コマンドとその引数）
                config.command = Some(args[i].clone());
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
        assert_eq!(config.command, None);
        assert_eq!(config.command_args.len(), 0);
    }
}

