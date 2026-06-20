#![cfg(unix)]

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::thread;

use aibe_client::route_turn;
use aibe_protocol::{
    ClientRequest, ClientResponse, FeatureAction, RouteKind, RoutePlan, RouteTurnStatus,
};

#[test]
fn route_turn_roundtrip_over_socket() {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket_path = dir.path().join("aibe.sock");
    let _ = fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path).expect("bind");

    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accept");
        let mut writer = stream.try_clone().expect("clone");
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).expect("read request");
        let req: ClientRequest = serde_json::from_str(line.trim()).expect("parse request");
        let ClientRequest::RouteTurn {
            id,
            query,
            cwd,
            session,
            conversation,
            cli_overrides,
        } = req
        else {
            panic!("expected route_turn request");
        };
        assert_eq!(id, "route-1");
        assert_eq!(query, "hello");
        assert_eq!(cwd, "/tmp/proj");
        assert_eq!(session.ai_session_id, "session-1");
        assert_eq!(session.aish_session_dir.as_deref(), Some("/tmp/aish"));
        assert!(session.tty);
        assert_eq!(conversation.conversation_id.as_deref(), Some("conv-1"));
        assert_eq!(conversation.recent_summary.as_deref(), Some("summary"));
        assert!(!conversation.new_conversation);
        assert_eq!(cli_overrides.preset.as_deref(), Some("fast"));
        assert_eq!(cli_overrides.log_tail_bytes, Some(128));
        assert!(cli_overrides.yes_exec);

        let response = ClientResponse::RouteTurnResult {
            id: "route-1".into(),
            status: RouteTurnStatus::Ok,
            plan: RoutePlan {
                conversation_id: "conv-1".into(),
                new_conversation: false,
                route_kind: RouteKind::Continue,
                recommended_preset: Some("fast".into()),
                recommended_tools: Some(vec!["read_file".into()]),
                log_tail_bytes: Some(128),
                feature_actions: vec![FeatureAction::SetLogTailBytes { bytes: 256 }],
                require_shell_approval: false,
                log_tail_escalation: false,
                route_reason: "continue".into(),
                confidence: Some(0.8),
            },
        };
        let payload = serde_json::to_string(&response).expect("serialize response");
        writeln!(writer, "{payload}").expect("write response");
        writer.flush().expect("flush response");
    });

    let request = ClientRequest::RouteTurn {
        id: "route-1".into(),
        query: "hello".into(),
        cwd: "/tmp/proj".into(),
        session: aibe_protocol::RouteTurnSession {
            ai_session_id: "session-1".into(),
            aish_session_dir: Some("/tmp/aish".into()),
            tty: true,
        },
        conversation: aibe_protocol::RouteTurnConversation {
            conversation_id: Some("conv-1".into()),
            recent_summary: Some("summary".into()),
            new_conversation: false,
            preprocessor_hints: None,
        },
        cli_overrides: aibe_protocol::RouteTurnCliOverrides {
            preset: Some("fast".into()),
            tools: Some(vec!["read_file".into()]),
            log_tail_bytes: Some(128),
            yes_exec: true,
        },
    };

    let resp = route_turn(&socket_path, request).expect("route_turn");
    handle.join().expect("server");

    match resp {
        ClientResponse::RouteTurnResult { id, status, plan } => {
            assert_eq!(id, "route-1");
            assert_eq!(status, RouteTurnStatus::Ok);
            assert_eq!(plan.conversation_id, "conv-1");
            assert_eq!(plan.route_kind, RouteKind::Continue);
            assert_eq!(plan.recommended_tools.as_ref().map(Vec::len), Some(1));
            assert_eq!(plan.feature_actions.len(), 1);
        }
        other => panic!("unexpected response: {other:?}"),
    }
}
