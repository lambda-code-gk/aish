#![cfg(unix)]
//! Smart feature plan の `ai ask` 導通（route_turn → feature_executor → agent_turn）。

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use aibe_protocol::{
    AgentTurnStatus, ClientRequest, ClientResponse, ProtocolMessageOut, RouteTurnStatus,
};

struct MockSocketServer {
    handle: Option<JoinHandle<()>>,
    _dir: tempfile::TempDir,
    socket_path: std::path::PathBuf,
}

impl MockSocketServer {
    fn smart_feature_ask() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("aibe.sock");
        let _ = fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path).expect("bind");
        let handle = thread::spawn(move || {
            let mut handled = 0usize;
            while handled < 2 {
                let Ok((stream, _)) = listener.accept() else {
                    break;
                };
                let mut writer = stream.try_clone().expect("clone");
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                line.clear();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                if line.trim().is_empty() {
                    continue;
                }
                let req: ClientRequest = serde_json::from_str(line.trim()).expect("parse request");
                let response = match req {
                    ClientRequest::RouteTurn { .. } => ClientResponse::RouteTurnResult {
                        id: "route-1".into(),
                        status: RouteTurnStatus::Ok,
                        plan: serde_json::from_str(
                            r#"{
                              "conversation_id": "conv-e2e",
                              "new_conversation": true,
                              "route_kind": "tool_assisted",
                              "feature_actions": [
                                {"type":"set_recommended_tools","tools":["read_file","grep"]}
                              ],
                              "require_shell_approval": false,
                              "log_tail_escalation": false,
                              "route_reason": "inspect repo"
                            }"#,
                        )
                        .expect("plan json"),
                    },
                    ClientRequest::AgentTurn {
                        tools, messages, ..
                    } => {
                        assert_eq!(
                            tools,
                            vec!["read_file".to_string(), "grep".to_string()],
                            "feature_executor should apply SetRecommendedTools"
                        );
                        assert!(
                            messages
                                .iter()
                                .any(|m| m.role == "user" && m.content.contains("エラー")),
                            "user message must reach agent_turn"
                        );
                        ClientResponse::AgentTurnResult {
                            id: "turn-1".into(),
                            status: AgentTurnStatus::Ok,
                            assistant_message: ProtocolMessageOut {
                                role: "assistant".into(),
                                content: "smart feature ok".into(),
                            },
                            tool_calls: vec![],
                        }
                    }
                    other => panic!("unexpected request: {other:?}"),
                };
                handled += 1;
                let payload = serde_json::to_string(&response).expect("serialize");
                writeln!(writer, "{payload}").expect("write");
                writer.flush().expect("flush");
            }
        });
        Self {
            handle: Some(handle),
            _dir: dir,
            socket_path,
        }
    }
}

impl Drop for MockSocketServer {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn write_ai_config(socket_path: &std::path::Path, dir: &tempfile::TempDir) -> std::path::PathBuf {
    let config_path = dir.path().join("ai.toml");
    fs::write(
        &config_path,
        format!(
            r#"
socket_path = "{}"
[ask]
default_profile = "fast"
"#,
            socket_path.display()
        ),
    )
    .expect("write config");
    config_path
}

fn script_available() -> bool {
    Command::new("script")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn tty_ask_applies_route_feature_actions_before_agent_turn() {
    if !script_available() {
        eprintln!("skip: script(1) not available for pseudo-tty");
        return;
    }

    let server = MockSocketServer::smart_feature_ask();
    let home = tempfile::tempdir().expect("home");
    let cfg = write_ai_config(&server.socket_path, &home);
    let ai_bin = env!("CARGO_BIN_EXE_ai");
    let inner = format!("{ai_bin} --no-start --no-progress '直近のエラーを調べて'");
    let transcript = home.path().join("typescript");

    let status = Command::new("script")
        .arg("-q")
        .arg("-c")
        .arg(&inner)
        .arg(&transcript)
        .env("AI_CONFIG", &cfg)
        .env("HOME", home.path())
        .status()
        .expect("run script+ai");

    assert!(status.success(), "ai exited with {status}");
    let captured = fs::read_to_string(&transcript).unwrap_or_default();
    assert!(
        captured.contains("smart feature ok"),
        "transcript: {captured}"
    );
}

/// `execute_feature_actions_mvp` が `MemoryQueryDto.user_query` に元入力を渡すこと（RPC 品質）。
#[test]
fn memory_query_forwards_user_input_as_user_query() {
    use ai::application::execute_feature_actions_mvp;
    use ai::clap_cli::TurnOptions;
    use ai::ports::outbound::{AgentError, MemoryClient};
    use aibe_protocol::{FeatureAction, MemoryContext, MemoryQueryDto, MemoryQueryStatus};

    struct CaptureClient {
        last_query: Arc<Mutex<Option<MemoryQueryDto>>>,
    }

    impl MemoryClient for CaptureClient {
        fn memory_apply(
            &self,
            _: &str,
            _: &MemoryContext,
            _: aibe_protocol::MemoryOperationDto,
        ) -> Result<ClientResponse, AgentError> {
            Err(AgentError::Request("unexpected".into()))
        }

        fn memory_query(
            &self,
            _: &str,
            _: &MemoryContext,
            query: MemoryQueryDto,
        ) -> Result<ClientResponse, AgentError> {
            *self.last_query.lock().expect("lock") = Some(query);
            Ok(ClientResponse::MemoryQueryResult {
                id: "q1".into(),
                status: MemoryQueryStatus::Ok,
                entries: vec![],
                prompt_block: Some("block".into()),
            })
        }

        fn memory_kind_list(
            &self,
            _: &str,
            _: &MemoryContext,
        ) -> Result<ClientResponse, AgentError> {
            Err(AgentError::Request("unexpected".into()))
        }

        fn memory_recipe_run(
            &self,
            _: &str,
            _: &MemoryContext,
            _: &str,
            _: bool,
            _: Option<String>,
        ) -> Result<ClientResponse, AgentError> {
            Err(AgentError::Request("unexpected".into()))
        }
    }

    let last_query = Arc::new(Mutex::new(None));
    let client = CaptureClient {
        last_query: Arc::clone(&last_query),
    };
    let actions = vec![FeatureAction::MemoryQuery {
        query: MemoryQueryDto::default(),
    }];
    let ctx = MemoryContext {
        cwd: Some("/tmp".into()),
        memory_space_id: Some("space".into()),
    };
    let turn = TurnOptions {
        quiet: true,
        format: None,
        dry_run: false,
        preset: None,
        log_tail: None,
        log: None,
        no_log: false,
        session: None,
        socket: None,
        no_start: true,
        tools: None,
        profile: None,
        new: false,
        verbose_tools: false,
        progress: false,
        no_progress: false,
        timeout: None,
        yes_exec: false,
        silent_exec: false,
        console_hint: false,
        no_console_hint: false,
    };
    let _ = execute_feature_actions_mvp(
        &actions,
        "プロジェクトのルールは？",
        Some(ctx),
        "sess",
        turn,
        &client,
        true,
    );
    let query = last_query.lock().expect("lock").take().expect("captured");
    assert_eq!(
        query.user_query.as_deref(),
        Some("プロジェクトのルールは？")
    );
    assert!(query.include_prompt_block);
}
