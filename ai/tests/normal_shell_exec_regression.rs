#![cfg(unix)]
//! 通常 shell_exec 経路の回帰（0055 minimal）。

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::{Child, Command};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use aibe_protocol::{
    AgentTurnStatus, ClientRequest, ClientResponse, ProtocolMessageOut, ShellExecApprovalOrigin,
};

fn regression_timeout() -> Duration {
    let secs = std::env::var("AISH_0055_E2E_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(15);
    Duration::from_secs(secs.min(60))
}

struct NormalShellExecMock {
    socket_path: std::path::PathBuf,
    _dir: tempfile::TempDir,
    handle: Option<JoinHandle<()>>,
}

impl NormalShellExecMock {
    fn spawn() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("aibe.sock");
        let _ = fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path).expect("bind");
        let deadline = Instant::now() + regression_timeout();
        let handle = thread::spawn(move || {
            listener.set_nonblocking(true).expect("set_nonblocking");
            let (stream, _) = loop {
                if Instant::now() >= deadline {
                    panic!("mock server accept timed out");
                }
                match listener.accept() {
                    Ok(conn) => break conn,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(20));
                    }
                    Err(e) => panic!("accept failed: {e}"),
                }
            };
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
                approved: _approval_approved,
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
        }
    }

    fn join_with_timeout(mut self) {
        if let Some(handle) = self.handle.take() {
            let deadline = Instant::now() + regression_timeout();
            while Instant::now() < deadline {
                if handle.is_finished() {
                    let _ = handle.join();
                    return;
                }
                thread::sleep(Duration::from_millis(20));
            }
            panic!("normal shell regression mock server join timed out");
        }
    }
}

fn wait_child_with_timeout(mut child: Child, deadline: Instant) -> std::process::Output {
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().expect("wait output"),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("ai child timed out");
            }
            Err(e) => panic!("wait child failed: {e}"),
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
fn non_collaborative_ai_uses_normal_approval_protocol() {
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

    let deadline = Instant::now() + regression_timeout();
    let output = wait_child_with_timeout(
        Command::new(env!("CARGO_BIN_EXE_ai"))
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
            .spawn()
            .expect("spawn ai"),
        deadline,
    );
    server.join_with_timeout();

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}
