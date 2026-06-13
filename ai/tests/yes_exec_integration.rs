//! `--yes-exec` と shell_exec 承認の統合テスト。

#![cfg(unix)]

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::{self, JoinHandle};

use ai::adapters::outbound::YesExecCache;
use ai::domain::{
    exact_shell_exec_key, ShellExecApprovalChoice, ShellExecRememberScope, ShellExecTier,
};
use aibe_protocol::{
    AgentTurnStatus, ClientRequest, ClientResponse, ProtocolMessageOut, ShellExecApprovalOrigin,
};

const COMMAND: &str = "echo";
const ARGS: &[&str] = &["hi"];

struct ApprovalMockServer {
    socket_path: PathBuf,
    _dir: tempfile::TempDir,
    handle: Option<JoinHandle<()>>,
}

impl ApprovalMockServer {
    fn spawn(expect_approved: bool) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("aibe.sock");
        let _ = fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path).expect("bind");
        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            run_approval_flow(stream, expect_approved);
        });
        Self {
            socket_path,
            _dir: dir,
            handle: Some(handle),
        }
    }
}

impl Drop for ApprovalMockServer {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn run_approval_flow(stream: std::os::unix::net::UnixStream, expect_approved: bool) {
    let mut writer = stream.try_clone().expect("clone");
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).expect("read request");
    let req: ClientRequest = serde_json::from_str(line.trim()).expect("parse request");
    let ClientRequest::AgentTurn { id, .. } = req else {
        panic!("expected agent_turn");
    };

    let prompt = ClientResponse::ShellExecApprovalPrompt {
        id: "approval-prompt-1".into(),
        turn_id: id.clone(),
        tool_call_id: "call_exec_1".into(),
        command: COMMAND.into(),
        args: ARGS.iter().map(|s| (*s).to_string()).collect(),
    };
    writeln!(
        writer,
        "{}",
        serde_json::to_string(&prompt).expect("serialize prompt")
    )
    .expect("write prompt");

    line.clear();
    reader.read_line(&mut line).expect("read approval");
    let approval: ClientRequest = serde_json::from_str(line.trim()).expect("parse approval");
    let ClientRequest::ShellExecApproval {
        approved,
        approval_origin,
        ..
    } = approval
    else {
        panic!("expected shell_exec_approval");
    };
    assert_eq!(
        approved, expect_approved,
        "unexpected approval decision from ai client"
    );
    assert_eq!(
        approval_origin,
        if expect_approved {
            ShellExecApprovalOrigin::SessionCacheExactInvocation
        } else {
            ShellExecApprovalOrigin::UiNo
        }
    );

    let final_resp = ClientResponse::AgentTurnResult {
        id,
        status: AgentTurnStatus::Ok,
        assistant_message: ProtocolMessageOut {
            role: "assistant".into(),
            content: if expect_approved {
                "approved".into()
            } else {
                "denied".into()
            },
        },
        tool_calls: vec![],
    };
    writeln!(
        writer,
        "{}",
        serde_json::to_string(&final_resp).expect("serialize final")
    )
    .expect("write final");
}

fn write_aibe_config(dir: &Path) -> PathBuf {
    let path = dir.join("aibe.toml");
    fs::write(
        &path,
        r#"
[tools.shell_exec]
shell_exec_approval = "ask"
"#,
    )
    .expect("write aibe config");
    path
}

fn write_ai_config(home: &Path, socket_path: &Path, history_dir: &Path, extra: &str) -> PathBuf {
    let path = home.join("ai.toml");
    fs::write(
        &path,
        format!(
            r#"
socket_path = "{}"
history_dir = "{}"
history_max_entries = 0
{extra}
"#,
            socket_path.display(),
            history_dir.display(),
        ),
    )
    .expect("write ai config");
    path
}

fn seed_yes_exec_cache(history_dir: &Path) {
    let key = exact_shell_exec_key(
        COMMAND,
        &ARGS.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
    );
    let cache_dir = history_dir.join("yes-exec");
    fs::create_dir_all(&cache_dir).expect("cache dir");
    fs::write(
        cache_dir.join("global.json"),
        serde_json::to_string(&vec![key]).expect("serialize cache"),
    )
    .expect("write cache");
}

#[test]
fn yes_exec_with_seeded_cache_still_denies_before_session_allow() {
    let server = ApprovalMockServer::spawn(false);
    let home = tempfile::tempdir().expect("home");
    let history_dir = home.path().join("history");
    fs::create_dir_all(&history_dir).expect("history");
    seed_yes_exec_cache(&history_dir);
    let aibe_cfg = write_aibe_config(home.path());
    let ai_cfg = write_ai_config(home.path(), &server.socket_path, &history_dir, "");

    let out = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("AI_CONFIG", &ai_cfg)
        .env("AIBE_CONFIG", &aibe_cfg)
        .env("HOME", home.path())
        .args(["ask", "--quiet", "--no-start", "--yes-exec", "run echo"])
        .output()
        .expect("run ai");

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("non-interactive stdin"));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "denied");
}

#[test]
fn yes_exec_without_cache_denies_on_non_tty() {
    let server = ApprovalMockServer::spawn(false);
    let home = tempfile::tempdir().expect("home");
    let history_dir = home.path().join("history");
    fs::create_dir_all(&history_dir).expect("history");
    let aibe_cfg = write_aibe_config(home.path());
    let ai_cfg = write_ai_config(home.path(), &server.socket_path, &history_dir, "");

    let out = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("AI_CONFIG", &ai_cfg)
        .env("AIBE_CONFIG", &aibe_cfg)
        .env("HOME", home.path())
        .args(["ask", "--quiet", "--no-start", "--yes-exec", "run echo"])
        .output()
        .expect("run ai");

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("non-interactive stdin"));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "denied");
}

#[test]
fn yes_exec_with_preset_never_ignores_cache() {
    let server = ApprovalMockServer::spawn(false);
    let home = tempfile::tempdir().expect("home");
    let history_dir = home.path().join("history");
    fs::create_dir_all(&history_dir).expect("history");
    seed_yes_exec_cache(&history_dir);
    let aibe_cfg = write_aibe_config(home.path());
    let ai_cfg = write_ai_config(
        home.path(),
        &server.socket_path,
        &history_dir,
        r#"
[presets.blocked]
shell_exec_approval = "never"
"#,
    );

    let out = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("AI_CONFIG", &ai_cfg)
        .env("AIBE_CONFIG", &aibe_cfg)
        .env("HOME", home.path())
        .args([
            "ask",
            "--quiet",
            "--no-start",
            "--yes-exec",
            "--preset",
            "blocked",
            "run echo",
        ])
        .output()
        .expect("run ai");

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("non-interactive stdin"));
}

#[test]
fn yes_exec_cache_supports_exact_and_command_scopes() {
    let root = tempfile::tempdir().expect("root");
    let mut cache = YesExecCache::load(root.path(), Some("sess-1")).expect("load");
    let args = vec!["hi".to_string()];

    assert_eq!(
        cache.should_auto_approve(COMMAND, &args, ShellExecTier::ReadOnly),
        None
    );
    cache
        .remember_choice(
            COMMAND,
            &args,
            ShellExecTier::ReadOnly,
            ShellExecApprovalChoice::AlwaysThisSession,
        )
        .expect("remember exact");
    assert_eq!(
        cache.should_auto_approve(COMMAND, &args, ShellExecTier::ReadOnly),
        Some(ShellExecRememberScope::ExactInvocation)
    );
    assert_eq!(
        cache.should_auto_approve(COMMAND, &args, ShellExecTier::Mutating),
        Some(ShellExecRememberScope::ExactInvocation)
    );
    assert_eq!(
        cache.should_auto_approve(COMMAND, &args, ShellExecTier::Destructive),
        None
    );

    let mut cache = YesExecCache::load(root.path(), Some("sess-2")).expect("reload");
    cache
        .remember_choice(
            COMMAND,
            &args,
            ShellExecTier::Mutating,
            ShellExecApprovalChoice::CommandOnly,
        )
        .expect("remember command");
    assert_eq!(
        cache.should_auto_approve(COMMAND, &args, ShellExecTier::Mutating),
        Some(ShellExecRememberScope::CommandName)
    );
    assert_eq!(
        cache.should_auto_approve(COMMAND, &args, ShellExecTier::Destructive),
        None
    );
}
