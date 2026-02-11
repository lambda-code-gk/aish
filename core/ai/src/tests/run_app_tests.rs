use crate::cli::Config;
use crate::domain::TaskName;
use crate::ports::inbound::UseCaseRunner;
use crate::wiring;
use common::domain::ProviderName;
use common::error::Error;

/// 標準アダプターで App を組み立て、Runner で run する（テスト用の入口）
fn run_app(config: Config) -> Result<i32, Error> {
    let app = wiring::wire_ai(config.non_interactive, config.verbose);
    let runner = crate::Runner { app };
    runner.run(config)
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
    // 引数なしの ai → クエリ未指定エラー（-c を促す）
    let config = Config::default();
    let result = run_app(config);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("No query provided"),
        "expected 'No query provided', got: {}",
        err
    );
    assert!(err.to_string().contains("--continue"));
    assert_eq!(err.exit_code(), 64);
}

#[test]
fn test_run_app_continue_without_state() {
    // ai -c で再開を要求したが保存状態がない場合はエラー
    let config = Config {
        continue_flag: true,
        ..Default::default()
    };
    let result = run_app(config);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("No continuation state"));
    assert_eq!(err.exit_code(), 64);
}

#[test]
fn test_run_app_with_message() {
    // echoプロファイルを使用してネットワーク不要で高速に実行
    // （profile未指定だとGeminiが使われ、APIキー欠如でHTTPタイムアウトまで数秒かかる）
    let config = Config {
        profile: Some(ProviderName::new("echo")),
        message_args: vec!["Hello".to_string()],
        ..Default::default()
    };
    let result = run_app(config);
    assert!(result.is_ok(), "echo profile should succeed without API key");
}

#[test]
fn test_run_app_with_task_and_message() {
    // echoプロファイルを使用（agentタスクは存在しない想定なのでLLMパスに入る）
    let config = Config {
        profile: Some(ProviderName::new("echo")),
        task: Some(TaskName::new("agent")),
        message_args: vec!["hello".to_string(), "world".to_string()],
        ..Default::default()
    };
    let result = run_app(config);
    assert!(result.is_ok(), "echo profile should succeed without API key");
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
fn test_run_app_with_profile() {
    // 環境変数が設定されていない場合はエラーになるが、基本的な構造はテストできる
    let config = Config {
        profile: Some(ProviderName::new("echo")),
        message_args: vec!["Hello".to_string()],
        ..Default::default()
    };
    // EchoプロファイルはAPIキーが不要なので成功する
    let result = run_app(config);
    assert!(result.is_ok());
}

#[test]
fn test_run_app_with_unknown_profile() {
    let config = Config {
        profile: Some(ProviderName::new("unknown")),
        message_args: vec!["Hello".to_string()],
        ..Default::default()
    };
    let result = run_app(config);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Unknown provider"));
    assert_eq!(err.exit_code(), 64);
}

#[test]
fn test_run_app_list_tools_echo() {
    // echo プロバイダで有効なツール一覧を表示（ネットワーク不要）
    let config = Config {
        list_tools: true,
        profile: Some(ProviderName::new("echo")),
        ..Default::default()
    };
    let result = run_app(config);
    assert!(result.is_ok(), "list-tools with profile echo should succeed");
    assert_eq!(result.unwrap(), 0);
}
