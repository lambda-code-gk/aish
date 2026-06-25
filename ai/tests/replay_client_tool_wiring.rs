//! `aish.replay_show` client tool の main 経路配線回帰。

use ai::domain::client_tools::replay_show::replay_client_tool_callback;
use aibe_client::ClientToolCallRequest;
use aish_replay::LogEvent;

#[test]
fn agent_turn_request_stream_with_replay_client_tool_returns_real_replay_output() {
    let events = vec![
        LogEvent::command_start_span(
            &aish_replay::CommandSpec {
                program: "echo".into(),
                args: vec!["wired".into()],
            },
            1,
            "2026-01-01T00:00:00Z",
            aish_replay::CommandKind::Exec,
        ),
        LogEvent::stdout_indexed("wired\n", 1),
        LogEvent::command_end(1, Some(0), "2026-01-01T00:00:01Z"),
    ];
    let callback = replay_client_tool_callback(events);
    let result = callback(ClientToolCallRequest {
        id: "id-1".into(),
        turn_id: "turn-1".into(),
        call_id: "call-1".into(),
        name: "aish.replay_show".into(),
        arguments: serde_json::json!({"index": 1}),
    })
    .expect("main-path callback must execute replay_show");
    assert!(result.content.starts_with("[untrusted terminal output]"));
    assert!(result.content.contains("wired"));
    assert!(!result.content.contains("client tool unavailable"));
}
