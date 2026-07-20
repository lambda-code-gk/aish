// RED stubs for 0054 Safe File Write Tools.
// Removed from #[ignore] when the corresponding phase lands.

use ai::domain::smart_preprocessor::{
    clamp_local_tools_to_allowlist, project_safe_local_tools, LocalToolHint, SmartToolHint,
};
use ai::domain::{resolve_tools, ConfigToolsTokens};
use aibe_protocol::{sanitize_readonly_advisory_tools, APPLY_PATCH, WRITE_FILE};

#[test]
fn full_tool_category_excludes_write_tools() {
    let resolved = resolve_tools(Some("@full"), &ConfigToolsTokens::default()).expect("resolve");
    for name in resolved.allowlist.names() {
        assert_ne!(name.as_str(), WRITE_FILE);
        assert_ne!(name.as_str(), APPLY_PATCH);
    }
}

#[test]
fn route_turn_does_not_recommend_write_tools() {
    let safe = sanitize_readonly_advisory_tools(&[
        "read_file".into(),
        WRITE_FILE.into(),
        APPLY_PATCH.into(),
        "grep".into(),
    ]);
    assert!(!safe.iter().any(|t| t == WRITE_FILE || t == APPLY_PATCH));

    for hint in [
        SmartToolHint::GitStatus,
        SmartToolHint::GitDiff,
        SmartToolHint::Grep,
        SmartToolHint::ReadFile,
        SmartToolHint::ListDir,
        SmartToolHint::ShellExecCandidate,
        SmartToolHint::MemorySearch,
        SmartToolHint::ConversationSearch,
    ] {
        if let Some(local) = LocalToolHint::from_smart_tool_hint(hint) {
            let runtime = local.runtime_tool_name();
            assert_ne!(runtime, WRITE_FILE);
            assert_ne!(runtime, APPLY_PATCH);
        }
    }

    let projected = project_safe_local_tools(&[
        SmartToolHint::ReadFile,
        SmartToolHint::Grep,
        SmartToolHint::ShellExecCandidate,
    ]);
    let allowlist = vec![
        "read_file".into(),
        WRITE_FILE.into(),
        APPLY_PATCH.into(),
        "grep".into(),
    ];
    let enabled = clamp_local_tools_to_allowlist(projected, &allowlist);
    assert!(enabled
        .iter()
        .all(|tool| tool.runtime_tool_name() != WRITE_FILE
            && tool.runtime_tool_name() != APPLY_PATCH));
}

#[test]
fn edit_tool_category_includes_write_tools() {
    let resolved = resolve_tools(Some("@edit"), &ConfigToolsTokens::default()).expect("resolve");
    let names: Vec<_> = resolved
        .allowlist
        .names()
        .iter()
        .map(|n| n.as_str())
        .collect();
    assert_eq!(
        names,
        vec![
            "read_file",
            "list_dir",
            "grep",
            "git_diff",
            "git_status",
            WRITE_FILE,
            APPLY_PATCH,
        ]
    );
    let full = resolve_tools(Some("@full"), &ConfigToolsTokens::default()).expect("resolve");
    for name in full.allowlist.names() {
        assert_ne!(name.as_str(), WRITE_FILE);
        assert_ne!(name.as_str(), APPLY_PATCH);
    }
}

#[test]
fn ai_warns_when_write_tools_enabled() {
    let resolved = resolve_tools(Some("@edit"), &ConfigToolsTokens::default()).expect("resolve");
    assert!(resolved.startup.warn_write);
    assert!(resolved.startup.enabled_list.contains(WRITE_FILE));
    assert!(resolved.startup.enabled_list.contains(APPLY_PATCH));
    assert_eq!(resolved.startup.source_hint.as_deref(), Some("@edit"));

    let literal =
        resolve_tools(Some("write_file"), &ConfigToolsTokens::default()).expect("resolve");
    assert!(literal.startup.warn_write);
    assert_eq!(literal.startup.enabled_list, WRITE_FILE);
}

#[cfg(test)]
mod phase8_file_write_approval_ui {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::thread;

    use ai::adapters::outbound::{
        escape_for_file_write_approval_display, file_write_approval_decision_from_input,
        file_write_approval_prompt_stderr_lines, format_tool_call_line,
        parse_file_write_approval_choice,
    };
    use aibe_client::{
        agent_turn_on_stream_with_callbacks, AgentTurnCallbacks, ShellExecApprovalDecision,
        ToolApprovalDecision, ToolApprovalPrompt,
    };
    use aibe_protocol::{
        AgentTurnStatus, ClientRequest, ClientResponse, ExecutedToolCall, ProtocolMessage,
        ProtocolMessageOut, ToolApprovalOrigin, ToolName, ToolRiskClass, WRITE_FILE,
    };
    use serde_json::json;

    fn sample_tool_prompt() -> ToolApprovalPrompt {
        ToolApprovalPrompt {
            prompt_id: "prompt-1".into(),
            turn_id: "turn-1".into(),
            tool_call_id: "call-write-1".into(),
            tool_name: WRITE_FILE.into(),
            risk_class: ToolRiskClass::WriteLike,
            summary: "create demo.txt (+1 -0, 0 -> 5 bytes)".into(),
            paths: vec!["demo.txt".into()],
            preview: "+hello\n".into(),
            preview_truncated: false,
        }
    }

    #[test]
    fn file_write_approval_ui_preserves_japanese_preview() {
        let mut prompt = sample_tool_prompt();
        prompt.preview =
            "--- a/test.txt\n+++ b/test.txt\n@@\n-Line 5: old\n+Line 5: ビルドの最適化\n".into();
        let lines = file_write_approval_prompt_stderr_lines(&prompt);
        let joined = lines.join("\n");
        assert!(joined.contains("ビルドの最適化"));
        assert!(!joined.contains("\\xe3"));
    }

    #[test]
    fn file_write_approval_ui_escapes_control_chars() {
        let mut prompt = sample_tool_prompt();
        prompt.preview = "\x1b[31msecret\x1b[0m\n".into();
        prompt.paths = vec!["src/\x07main.rs".into()];
        let lines = file_write_approval_prompt_stderr_lines(&prompt);
        let joined = lines.join("\n");
        assert!(joined.contains("\\x1b"));
        assert!(joined.contains("\\x07"));
        assert!(!joined.contains('\x1b'));
        let escaped = escape_for_file_write_approval_display(&prompt.preview);
        assert!(escaped.contains("\\x1b"));
    }

    #[test]
    fn file_write_approval_ui_shows_truncation_notice() {
        let mut prompt = sample_tool_prompt();
        prompt.preview_truncated = true;
        let lines = file_write_approval_prompt_stderr_lines(&prompt);
        let joined = lines.join("\n");
        assert!(joined.contains("preview truncated"));
    }

    #[test]
    fn file_write_approval_ui_rejects_non_tty() {
        let decision = file_write_approval_decision_from_input(false, "y\n");
        assert_eq!(decision, ToolApprovalDecision::Unavailable);
    }

    #[test]
    fn file_write_approval_ui_writes_stderr_only() {
        let lines = file_write_approval_prompt_stderr_lines(&sample_tool_prompt());
        let joined = lines.join("\n");
        assert!(joined.contains("ai: file write approval required:"));
        assert!(joined.contains("tool: write_file"));
        assert!(joined.contains("preview:"));
        // 承認 UI は行生成のみで stdout へは出さない（prompt 関数は eprintln! のみ）。
        assert!(!joined.is_empty());
    }

    #[test]
    fn file_write_approval_ui_yes_executes_write() {
        let (client, server) = UnixStream::pair().expect("pair");
        let handle = thread::spawn(move || run_tool_approval_mock_server(server, true));

        let mut seen = false;
        let resp = agent_turn_on_stream_with_callbacks(
            client,
            agent_turn_request(),
            AgentTurnCallbacks::new(
                |_| ShellExecApprovalDecision {
                    approved: false,
                    approval_origin: aibe_protocol::ShellExecApprovalOrigin::UiNo,
                    handoff_result: None,
                    handoff_error: None,
                },
                |prompt: ToolApprovalPrompt| {
                    seen = true;
                    assert_eq!(prompt.tool_name, WRITE_FILE);
                    file_write_approval_decision_from_input(true, "y\n")
                },
            ),
        )
        .expect("agent turn");

        handle.join().expect("server");
        assert!(seen);
        match resp {
            ClientResponse::AgentTurnResult {
                assistant_message, ..
            } => assert_eq!(assistant_message.content, "write approved"),
            other => panic!("expected agent_turn_result, got {other:?}"),
        }
    }

    #[test]
    fn file_write_approval_ui_no_continues_turn() {
        let (client, server) = UnixStream::pair().expect("pair");
        let handle = thread::spawn(move || run_tool_approval_mock_server(server, false));

        let resp = agent_turn_on_stream_with_callbacks(
            client,
            agent_turn_request(),
            AgentTurnCallbacks::new(
                |_| ShellExecApprovalDecision {
                    approved: false,
                    approval_origin: aibe_protocol::ShellExecApprovalOrigin::UiNo,
                    handoff_result: None,
                    handoff_error: None,
                },
                |_| file_write_approval_decision_from_input(true, "n\n"),
            ),
        )
        .expect("agent turn");

        handle.join().expect("server");
        match resp {
            ClientResponse::AgentTurnResult {
                assistant_message, ..
            } => assert_eq!(assistant_message.content, "write denied"),
            other => panic!("expected agent_turn_result, got {other:?}"),
        }
    }

    #[test]
    fn verbose_tools_shows_change_id() {
        let line = format_tool_call_line(&ExecutedToolCall::ok(
            "c1".into(),
            ToolName::write_file(),
            json!({"path": "demo.txt"}),
            "wrote 5 bytes (change_id=chg_test123)".into(),
        ));
        assert!(line.contains("change_id=chg_test123"));
    }

    fn agent_turn_request() -> ClientRequest {
        ClientRequest::AgentTurn {
            id: "turn-1".into(),
            messages: vec![ProtocolMessage {
                role: "user".into(),
                content: "write".into(),
            }],
            tools: vec![WRITE_FILE.into()],
            client_tools: vec![],
            context: Default::default(),
            llm_profile: None,
        }
    }

    fn run_tool_approval_mock_server(mut server: UnixStream, expect_approved: bool) {
        let mut reader = BufReader::new(server.try_clone().expect("clone"));
        let mut line = String::new();
        reader.read_line(&mut line).expect("read request");

        let prompt = ClientResponse::ToolApprovalPrompt {
            id: "prompt-1".into(),
            turn_id: "turn-1".into(),
            tool_call_id: "call-write-1".into(),
            tool_name: WRITE_FILE.into(),
            risk_class: ToolRiskClass::WriteLike,
            summary: "create demo.txt (+1 -0, 0 -> 5 bytes)".into(),
            paths: vec!["demo.txt".into()],
            preview: "+hello\n".into(),
            preview_truncated: false,
        };
        writeln!(
            server,
            "{}",
            serde_json::to_string(&prompt).expect("serialize prompt")
        )
        .expect("write prompt");
        server.flush().expect("flush");

        line.clear();
        reader.read_line(&mut line).expect("read approval");
        let approval: ClientRequest = serde_json::from_str(line.trim()).expect("parse approval");
        let ClientRequest::ToolApproval {
            approved,
            approval_origin,
            ..
        } = approval
        else {
            panic!("expected tool_approval");
        };
        assert_eq!(approved, expect_approved);
        assert_eq!(
            approval_origin,
            if expect_approved {
                ToolApprovalOrigin::UiYes
            } else {
                ToolApprovalOrigin::UiNo
            }
        );

        let final_resp = ClientResponse::AgentTurnResult {
            id: "turn-1".into(),
            status: AgentTurnStatus::Ok,
            assistant_message: ProtocolMessageOut {
                role: "assistant".into(),
                content: if expect_approved {
                    "write approved".into()
                } else {
                    "write denied".into()
                },
            },
            tool_calls: vec![],
            completion_report: None,
        };
        writeln!(
            server,
            "{}",
            serde_json::to_string(&final_resp).expect("serialize final")
        )
        .expect("write final");
        server.flush().expect("flush final");
    }

    #[test]
    fn parse_file_write_choice_contract() {
        assert_eq!(parse_file_write_approval_choice("y\n"), Some(true));
        assert_eq!(parse_file_write_approval_choice("\n"), Some(false));
        assert!(parse_file_write_approval_choice("maybe").is_none());
    }
}
