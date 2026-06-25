//! `aish.replay_show` client tool の application 入口。

pub use crate::domain::client_tools::replay_show::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_tool_result_always_includes_untrusted_terminal_output_header() {
        let result = execute_replay_show(
            &aibe_client::ClientToolCallRequest {
                id: "id-1".into(),
                turn_id: "turn-1".into(),
                call_id: "call-1".into(),
                name: "aish.replay_show".into(),
                arguments: serde_json::json!({"index": 1, "tail_bytes": 8}),
            },
            &[
                aish_replay::LogEvent::command_start_span(
                    &aish_replay::CommandSpec {
                        program: "echo".into(),
                        args: vec!["hello".into()],
                    },
                    1,
                    "2026-01-01T00:00:00Z",
                    aish_replay::CommandKind::Exec,
                ),
                aish_replay::LogEvent::stdout_indexed("hello\n", 1),
                aish_replay::LogEvent::command_end(1, Some(0), "2026-01-01T00:00:01Z"),
            ],
        );
        assert!(result
            .expect("execute")
            .content
            .starts_with("[untrusted terminal output]"));
    }
}
