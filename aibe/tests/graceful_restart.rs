//! graceful restart / stop / status の統合テスト。
//!
//! 各テストは subprocess の env のみを使い、プロセス全体の `HOME` 等は変更しない。

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::Duration;

use aibe::daemon::{default_pid_file_path_for_home, write_pid_file, PidFileRecord};
use serde_json::Value;
use tempfile::tempdir;

const CHILD_EXIT_TIMEOUT: Duration = Duration::from_secs(5);
const SOCKET_DEAD_TIMEOUT: Duration = Duration::from_secs(5);

fn aibe_bin() -> PathBuf {
    if let Ok(p) = std::env::var("AIBE_BIN") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_BIN_EXE_aibe"))
}

struct TestEnv {
    home: PathBuf,
    config_path: PathBuf,
    socket_path: PathBuf,
}

impl TestEnv {
    fn new(dir: &Path) -> Self {
        let home = dir.join("home");
        fs::create_dir_all(&home).expect("home");
        let config_path = dir.join("aibe.toml");
        fs::write(&config_path, "[llm]\nprovider = \"mock\"\n").expect("config");
        let socket_path = home.join("run.sock");
        Self {
            home,
            config_path,
            socket_path,
        }
    }

    fn pid_file_path(&self) -> PathBuf {
        default_pid_file_path_for_home(&self.home)
    }

    fn spawn_daemon(&self) -> Child {
        Command::new(aibe_bin())
            .arg("-f")
            .env("HOME", &self.home)
            .env("AIBE_CONFIG", &self.config_path)
            .env("AIBE_SOCKET_PATH", &self.socket_path)
            .spawn()
            .expect("spawn aibe")
    }

    fn run_control(&self, args: &[&str]) -> std::process::Output {
        Command::new(aibe_bin())
            .args(args)
            .env("HOME", &self.home)
            .env("AIBE_CONFIG", &self.config_path)
            .env("AIBE_SOCKET_PATH", &self.socket_path)
            .env("AIBE_BIN", aibe_bin())
            .output()
            .expect("control command")
    }
}

fn wait_for_socket(socket_path: &Path) {
    for _ in 0..100 {
        if socket_path.exists() && aibe_client::ping(socket_path) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("socket not ready: {}", socket_path.display());
}

fn wait_for_child(child: &mut Child, timeout: Duration) {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if child.try_wait().expect("wait").is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    panic!("child did not exit within {:?}", timeout);
}

#[test]
fn status_json_exposes_required_fields() {
    let dir = tempdir().expect("tempdir");
    let env = TestEnv::new(dir.path());

    let mut child = env.spawn_daemon();
    wait_for_socket(&env.socket_path);

    let output = env.run_control(&["status", "--format", "json"]);
    assert!(output.status.success(), "status failed");
    let json: Value = serde_json::from_slice(&output.stdout).expect("json");
    for key in [
        "state",
        "pid_file_state",
        "pid_file_path",
        "config_path",
        "socket_path",
        "socket_ping",
    ] {
        assert!(json.get(key).is_some(), "missing key {key}");
    }
    assert_eq!(json["socket_ping"], Value::Bool(true));
    assert!(json.get("pid").is_some(), "pid key must always be present");

    let _ = env.run_control(&["stop"]);
    wait_for_child(&mut child, CHILD_EXIT_TIMEOUT);
}

#[test]
fn status_reports_stale_pid_but_live_socket_as_running() {
    let dir = tempdir().expect("tempdir");
    let env = TestEnv::new(dir.path());

    let mut child = env.spawn_daemon();
    wait_for_socket(&env.socket_path);

    let stale = PidFileRecord {
        pid: std::process::id(),
        config_path: env.config_path.clone(),
        socket_path: env.socket_path.clone(),
        process_start_jiffies: 0,
    };
    write_pid_file(&env.pid_file_path(), &stale).expect("write stale pid");

    let output = env.run_control(&["status", "--format", "json"]);
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(json["state"], "running");
    assert_eq!(json["pid_file_state"], "stale");

    let _ = child.kill();
    wait_for_child(&mut child, Duration::from_secs(5));
}

#[test]
fn already_running_short_circuits_on_live_socket() {
    let dir = tempdir().expect("tempdir");
    let env = TestEnv::new(dir.path());

    let mut child = env.spawn_daemon();
    wait_for_socket(&env.socket_path);

    let output = env.run_control(&["-f"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("already running"), "stderr={stderr}");

    let _ = env.run_control(&["stop"]);
    wait_for_child(&mut child, CHILD_EXIT_TIMEOUT);
}

#[test]
fn stop_signals_daemon_and_cleans_up_pid_file() {
    let dir = tempdir().expect("tempdir");
    let env = TestEnv::new(dir.path());

    let mut child = env.spawn_daemon();
    wait_for_socket(&env.socket_path);

    let output = env.run_control(&["stop"]);
    assert!(output.status.success(), "stop failed: {:?}", output.stderr);

    wait_for_child(&mut child, CHILD_EXIT_TIMEOUT);
    assert!(!env.pid_file_path().exists());
    assert!(!env.socket_path.exists());
}

#[test]
fn restart_waits_for_new_daemon_readiness_before_returning() {
    let dir = tempdir().expect("tempdir");
    let env = TestEnv::new(dir.path());

    let mut child = env.spawn_daemon();
    wait_for_socket(&env.socket_path);
    let old_pid = fs::read_to_string(&env.pid_file_path())
        .ok()
        .and_then(|raw| serde_json::from_str::<PidFileRecord>(raw.trim()).ok())
        .map(|r| r.pid)
        .expect("old pid");

    let output = env.run_control(&["restart"]);
    assert!(
        output.status.success(),
        "restart failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    wait_for_child(&mut child, CHILD_EXIT_TIMEOUT);
    wait_for_socket(&env.socket_path);

    let new_pid = fs::read_to_string(&env.pid_file_path())
        .ok()
        .and_then(|raw| serde_json::from_str::<PidFileRecord>(raw.trim()).ok())
        .map(|r| r.pid)
        .expect("new pid");
    assert_ne!(new_pid, old_pid);

    let stop = env.run_control(&["stop"]);
    assert!(stop.status.success());
    wait_for_socket_dead(&env.socket_path, SOCKET_DEAD_TIMEOUT);
}

fn wait_for_socket_dead(socket_path: &Path, timeout: Duration) {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if !aibe_client::ping(socket_path) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("socket still alive: {}", socket_path.display());
}

#[test]
fn shutdown_cancels_active_turn_and_closes_memory_subscribe() {
    use aibe_protocol::{ClientRequest, ClientResponse};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    let dir = tempdir().expect("tempdir");
    let env = TestEnv::new(dir.path());

    let mut child = env.spawn_daemon();
    wait_for_socket(&env.socket_path);

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let socket_path = env.socket_path.clone();
    let saw_terminal = rt.block_on(async {
        let stream = UnixStream::connect(&socket_path).await.expect("connect");
        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();
        let req = ClientRequest::AgentTurn {
            id: "turn-shutdown".into(),
            messages: vec![aibe_protocol::ProtocolMessage {
                role: "user".into(),
                content: "hello".into(),
            }],
            tools: vec![],
            client_tools: vec![],
            context: Default::default(),
            llm_profile: None,
        };
        let payload = serde_json::to_string(&req).expect("json") + "\n";
        writer.write_all(payload.as_bytes()).await.expect("write");
        writer.flush().await.expect("flush");

        let _ = env.run_control(&["stop"]);

        let mut saw_terminal = false;
        for _ in 0..100 {
            if let Ok(Some(line)) = lines.next_line().await {
                if let Ok(ClientResponse::AgentTurnResult { .. }) =
                    serde_json::from_str(line.trim())
                {
                    break;
                }
                if let Ok(ClientResponse::Cancelled { .. }) = serde_json::from_str(line.trim()) {
                    saw_terminal = true;
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        saw_terminal || !socket_path.exists()
    });

    wait_for_child(&mut child, CHILD_EXIT_TIMEOUT);
    assert!(saw_terminal);
}
