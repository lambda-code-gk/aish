use crate::cli::Config;
use crate::domain::TaskName;
use crate::ports::inbound::RunAiApp;
use crate::wiring;
use common::domain::ProviderName;
use common::error::Error;

/// 標準アダプターで AiUseCase を組み立てて run する（テスト用の入口）
fn run_app(config: Config) -> Result<i32, Error> {
    wiring::wire_ai().run(config)
}

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
    let err = result.unwrap_err();
    assert!(err.to_string().contains("No query provided"));
    assert_eq!(err.exit_code(), 64);
}

#[test]
fn test_run_app_with_message() {
    // echoプロバイダを使用してネットワーク不要で高速に実行
    // （provider未指定だとGeminiが使われ、APIキー欠如でHTTPタイムアウトまで数秒かかる）
    let config = Config {
        provider: Some(ProviderName::new("echo")),
        message_args: vec!["Hello".to_string()],
        ..Default::default()
    };
    let result = run_app(config);
    assert!(result.is_ok(), "echo provider should succeed without API key");
}

#[test]
fn test_run_app_with_task_and_message() {
    // echoプロバイダを使用（agentタスクは存在しない想定なのでLLMパスに入る）
    let config = Config {
        provider: Some(ProviderName::new("echo")),
        task: Some(TaskName::new("agent")),
        message_args: vec!["hello".to_string(), "world".to_string()],
        ..Default::default()
    };
    let result = run_app(config);
    assert!(result.is_ok(), "echo provider should succeed without API key");
}

#[test]
fn test_run_app_help_takes_precedence() {
    let config = Config {
        help: true,
        task: Some(TaskName::new("agent")),
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
        provider: Some(ProviderName::new("echo")),
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
        provider: Some(ProviderName::new("unknown")),
        message_args: vec!["Hello".to_string()],
        ..Default::default()
    };
    let result = run_app(config);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Unknown provider"));
    assert_eq!(err.exit_code(), 64);
}
