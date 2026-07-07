#![cfg(unix)]
//! 通常 shell_exec 経路の回帰（0055 minimal）。

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::Command;
use std::thread::{self, JoinHandle};

use aibe_protocol::{
    AgentTurnStatus, ClientRequest, ClientResponse, ProtocolMessageOut, ShellExecApprovalOrigin,
};

struct NormalShellExecMock {
    socket_path: std::path::PathBuf,
    _dir: tempfile::TempDir,
    handle: Option<JoinHandle<()>>,
    human_shell_called: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl NormalShellExecMock {
    fn spawn() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("aibe.sock");
        let _ = fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path).expect("bind");
        let human_shell_called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let called_flag = std::sync::Arc::clone(&human_shell_called);
        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let mut writer = stream.try_clone().expect("clone");
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).expect("read");
            let req: ClientRequest = serde_json::from_str(line.trim()).expect("parse");
            let ClientRequest::AgentTurn { id, context, .. } = req else {
                panic!("expected agent_turn");
            };
            assert!(
                !context.collaborative_handoff,
                "non-collaborative request must not set collaborative_handoff"
            );

            let prompt = ClientResponse::ShellExecApprovalPrompt {
                id: "prompt-1".into(),
                turn_id: id.clone(),
                tool_call_id: "call-1".into(),
                command: "echo".into(),
                args: vec!["ok".into()],
            };
            writeln!(writer, "{}", serde_json::to_string(&prompt).expect("json")).expect("write");

            line.clear();
            reader.read_line(&mut line).expect("read approval");
            let approval: ClientRequest = serde_json::from_str(line.trim()).expect("approval");
            let ClientRequest::ShellExecApproval {
                approved: _,
                approval_origin,
                handoff_result,
                handoff_error,
                ..
            } = approval
            else {
                panic!("expected approval");
            };
            assert_ne!(
                approval_origin,
                ShellExecApprovalOrigin::CollaborativeHandoff
            );
            assert!(handoff_result.is_none());
            assert!(handoff_error.is_none());
            called_flag.store(false, std::sync::atomic::Ordering::SeqCst);

            let result = ClientResponse::AgentTurnResult {
                id,
                status: AgentTurnStatus::Ok,
                assistant_message: ProtocolMessageOut {
                    role: "assistant".into(),
                    content: "echo ok".into(),
                },
                tool_calls: vec![],
            };
            writeln!(writer, "{}", serde_json::to_string(&result).expect("json")).expect("write");
        });
        Self {
            socket_path,
            _dir: dir,
            handle: Some(handle),
            human_shell_called,
        }
    }
}

impl Drop for NormalShellExecMock {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[test]
fn normal_shell_exec_tier_classification_unchanged() {
    use ai::domain::{classify_shell_exec_tier, ShellExecTier};
    assert_eq!(
        classify_shell_exec_tier("git", &["status".into()]),
        ShellExecTier::ReadOnly
    );
    assert_eq!(
        classify_shell_exec_tier("rm", &["-rf".into(), "/".into()]),
        ShellExecTier::Destructive
    );
}

#[test]
fn normal_shell_exec_path_uses_approval_without_human_shell() {
    let server = NormalShellExecMock::spawn();
    let home = tempfile::tempdir().expect("home");
    let history_dir = home.path().join("history");
    fs::create_dir_all(&history_dir).expect("history");
    let ai_cfg = home.path().join("ai.toml");
    fs::write(
        &ai_cfg,
        format!(
            r#"
socket_path = "{}"
history_dir = "{}"
history_max_entries = 0
[ask]
tools = "shell_exec"
progress = false
"#,
            server.socket_path.display(),
            history_dir.display(),
        ),
    )
    .expect("ai config");
    let aibe_cfg = home.path().join("aibe.toml");
    fs::write(
        &aibe_cfg,
        r#"
[tools.shell_exec]
shell_exec_approval = "ask"
"#,
    )
    .expect("aibe config");

    let out = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("AI_CONFIG", &ai_cfg)
        .env("AIBE_CONFIG", &aibe_cfg)
        .env("HOME", home.path())
        .env(
            "AISH_BIN",
            Path::new(env!("CARGO_BIN_EXE_ai"))
                .parent()
                .unwrap()
                .join("aish"),
        )
        .args(["ask", "--quiet", "--no-start", "run echo"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("run ai");

    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!server
        .human_shell_called
        .load(std::sync::atomic::Ordering::SeqCst));
}
