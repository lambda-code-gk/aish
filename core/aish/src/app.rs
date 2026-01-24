use crate::args::Config;
use crate::shell::run_shell;
use common::session::Session;

pub fn run_app(config: Config) -> Result<i32, (String, i32)> {
    if config.help {
        print_help();
        return Ok(0);
    }
    
    // セッションパスを決定（指定されていない場合は一時ディレクトリを使用）
    let session_path = if let Some(ref session_dir) = config.session_dir {
        session_dir.clone()
    } else {
        // デフォルトのセッションディレクトリ（将来的に改善予定）
        std::env::temp_dir().join("aish_session").to_string_lossy().to_string()
    };
    
    // ホームディレクトリが指定されていない場合はエラー
    let home_dir = config.home_dir.ok_or((
        "Home directory (-d/--home-dir) is required".to_string(),
        64
    ))?;
    
    // セッション管理を初期化（ホームディレクトリを指定）
    let session = Session::new(&session_path, &home_dir)?;
    
    // コマンドが指定されている場合は、将来的にコマンド処理を実装
    if let Some(ref _command) = config.command {
        // TODO: コマンド処理を実装
        return Ok(0);
    }
    
    // 引数なしの場合はシェルを起動
    run_shell(&session)
}

fn print_help() {
    println!("Usage: aish [-h] [-s|--session-dir directory] [-d|--home-dir directory] <command> [args...]");
    println!("  -h                    Display this help message.");
    println!("  -s, --session-dir      Specify a directory for the session.");
    println!("  -d, --home-dir         Specify a home directory (sets AISH_HOME environment variable).");
    println!("  <command>     Command to execute (e.g., ls, start, stop).");
    println!("  [args...]     Arguments for the command.");
    println!("");
    println!("Available commands:");
    println!("  resume [id]            Resume a session (default: latest).");
    println!("  sessions               List available sessions.");
    println!("  rollout                Write the terminal log to the part file.");
    println!("  clear                  Clear the console and part files.");
    println!("  ls                     List the part files.");
    println!("  rm_last                Remove the last part file.");
    println!("  memory                 Manage memories (--list, --show <id>, --revoke <id>).");
    println!("  models                 Manage models (--provider, --unsupported, --available).");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_app_with_help() {
        let config = Config {
            help: true,
            ..Default::default()
        };
        let result = run_app(config);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[test]
    fn test_run_app_with_command() {
        use std::fs;
        let temp_dir = std::env::temp_dir();
        let home_path = temp_dir.join("aish_test_home_command");
        
        // ホームディレクトリを作成
        if home_path.exists() {
            fs::remove_dir_all(&home_path).unwrap();
        }
        fs::create_dir_all(&home_path).unwrap();
        
        let config = Config {
            command: Some("sessions".to_string()),
            home_dir: Some(home_path.to_string_lossy().to_string()),
            ..Default::default()
        };
        let result = run_app(config);
        // コマンド処理は未実装のため、現時点では成功を返す
        assert!(result.is_ok());
        
        // クリーンアップ
        fs::remove_dir_all(&home_path).unwrap();
    }
}

