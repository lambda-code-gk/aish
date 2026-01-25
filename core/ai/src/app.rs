use crate::args::Config;
use common::llm::{ProviderType, create_driver};
use std::io::{self, Write};

pub fn run_app(config: Config) -> Result<i32, (String, i32)> {
    if config.help {
        print_help();
        return Ok(0);
    }
    
    // クエリを構築（taskとmessage_argsを結合）
    let mut query_parts = Vec::new();
    if let Some(ref task) = config.task {
        query_parts.push(task.clone());
    }
    query_parts.extend_from_slice(&config.message_args);
    
    if query_parts.is_empty() {
        return Err(("No query provided. Please provide a message to send to the LLM.".to_string(), 64));
    }
    
    let query = query_parts.join(" ");
    
    // プロバイダタイプを決定
    let provider_type = if let Some(ref provider_str) = config.provider {
        ProviderType::from_str(provider_str)
            .ok_or_else(|| (format!("Unknown provider: {}. Supported providers: gemini, gpt, echo", provider_str), 64))?
    } else {
        // デフォルトはGemini
        ProviderType::Gemini
    };
    
    // ドライバーを作成（デフォルトモデルを使用）
    let driver = create_driver(provider_type, None)?;
    
    // LLMにクエリを送信（ストリーミング表示）
    driver.query_streaming(
        &query,
        None,
        &[],
        Box::new(|chunk| {
            print!("{}", chunk);
            io::stdout().flush().map_err(|e| (format!("Failed to flush stdout: {}", e), 74))?;
            Ok(())
        }),
    )?;
    
    // 最後に改行を出力
    println!();
    
    Ok(0)
}

fn print_help() {
    println!("Usage: ai [options] [message...]");
    println!("Options:");
    println!("  -h, --help                    Show this help message");
    println!("  -p, --provider <provider>      Specify LLM provider (gemini, gpt, echo). Default: gemini");
    println!("");
    println!("Description:");
    println!("  Send a message to the LLM and display the response.");
    println!("");
    println!("Examples:");
    println!("  ai Hello, how are you?");
    println!("  ai -p gpt What is Rust programming language?");
    println!("  ai --provider echo Explain quantum computing");
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
    fn test_run_app_without_query() {
        let config = Config::default();
        let result = run_app(config);
        // クエリがない場合はエラー
        assert!(result.is_err());
        let (msg, code) = result.unwrap_err();
        assert!(msg.contains("No query provided"));
        assert_eq!(code, 64);
    }

    #[test]
    fn test_run_app_with_message() {
        // 環境変数が設定されていない場合はエラーになるが、基本的な構造はテストできる
        let config = Config {
            message_args: vec!["Hello".to_string()],
            ..Default::default()
        };
        // APIキーがない場合はエラーになるが、引数解析は正常
        let _result = run_app(config);
        // 環境変数がない場合はエラーになる
        // 実際の動作確認は統合テストで行う
    }

    #[test]
    fn test_run_app_with_task_and_message() {
        // 環境変数が設定されていない場合はエラーになるが、基本的な構造はテストできる
        let config = Config {
            task: Some("agent".to_string()),
            message_args: vec!["hello".to_string(), "world".to_string()],
            ..Default::default()
        };
        // APIキーがない場合はエラーになるが、引数解析は正常
        let _result = run_app(config);
        // 環境変数がない場合はエラーになる
        // 実際の動作確認は統合テストで行う
    }

    #[test]
    fn test_run_app_help_takes_precedence() {
        let config = Config {
            help: true,
            task: Some("agent".to_string()),
            message_args: vec!["hello".to_string()],
            ..Default::default()
        };
        let result = run_app(config);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[test]
    fn test_run_app_with_provider() {
        // 環境変数が設定されていない場合はエラーになるが、基本的な構造はテストできる
        let config = Config {
            provider: Some("echo".to_string()),
            message_args: vec!["Hello".to_string()],
            ..Default::default()
        };
        // EchoプロバイダはAPIキーが不要なので成功する
        let result = run_app(config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_run_app_with_unknown_provider() {
        let config = Config {
            provider: Some("unknown".to_string()),
            message_args: vec!["Hello".to_string()],
            ..Default::default()
        };
        let result = run_app(config);
        assert!(result.is_err());
        let (msg, code) = result.unwrap_err();
        assert!(msg.contains("Unknown provider"));
        assert_eq!(code, 64);
    }

}

