#![cfg(unix)]

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::{Command, Output};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex, MutexGuard,
};
use std::thread;

use serde_json::Value;

const IDS: [&str; 6] = [
    "socket_reachable",
    "session_context",
    "shell_log_readable",
    "tools_configuration",
    "output_filter_configuration",
    "protocol_compatibility",
];
static SOCKET_FIXTURE_LOCK: Mutex<()> = Mutex::new(());

struct Fixture {
    _guard: MutexGuard<'static, ()>,
    _dir: tempfile::TempDir,
    socket: std::path::PathBuf,
    count: Arc<AtomicUsize>,
    handle: Option<thread::JoinHandle<()>>,
}
impl Fixture {
    fn server(response: &str) -> Self {
        let guard = SOCKET_FIXTURE_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("aibe.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let payload = response.to_string();
        let count = Arc::new(AtomicUsize::new(0));
        let seen = count.clone();
        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut line = String::new();
            BufReader::new(stream).read_line(&mut line).unwrap();
            seen.fetch_add(1, Ordering::SeqCst);
            writeln!(writer, "{payload}").unwrap();
        });
        Self {
            _guard: guard,
            _dir: dir,
            socket,
            count,
            handle: Some(handle),
        }
    }
    fn pong() -> Self {
        Self::server(r#"{"type":"pong","id":"health"}"#)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn config(home: &tempfile::TempDir, socket: &std::path::Path, extra: &str) -> std::path::PathBuf {
    let path = home.path().join("ai.toml");
    fs::write(
        &path,
        format!(
            "socket_path = {:?}\n[ask]\n{extra}\n",
            socket.display().to_string()
        ),
    )
    .unwrap();
    path
}
fn run(socket: &std::path::Path, args: &[&str], session: bool, extra: &str) -> Output {
    let home = tempfile::tempdir().unwrap();
    let cfg = config(&home, socket, extra);
    let mut command = Command::new(env!("CARGO_BIN_EXE_ai"));
    command
        .env("HOME", home.path())
        .env("AI_CONFIG", cfg)
        .env_remove("AI_FILTER")
        .env_remove("AI_ASK_LOG")
        .env_remove("AISH_SESSION_DIR")
        .arg("doctor")
        .arg("--quiet")
        .args(args);
    if session {
        let dir = home.path().join("session-1");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("log.jsonl"), "SECRET_LOG_BODY\n").unwrap();
        command.env("AISH_SESSION_DIR", dir);
    }
    command.output().unwrap()
}
fn json(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout).unwrap()
}
#[test]
fn doctor_health_checks_render_human_and_json() {
    let a = Fixture::pong();
    let human = run(&a.socket, &[], true, "");
    assert!(human.status.success());
    let text = String::from_utf8_lossy(&human.stdout);
    for id in IDS {
        assert!(text.contains(id));
    }
    drop(a);
    let b = Fixture::pong();
    let value = json(&run(&b.socket, &["--format", "json"], true, ""));
    let checks = value["checks"].as_array().unwrap();
    assert_eq!(checks.len(), 6);
    for (check, id) in checks.iter().zip(IDS) {
        assert_eq!(check["id"], id);
        let status = check["status"].as_str().unwrap().to_ascii_uppercase();
        assert!(
            text.contains(&format!("[{status}] {id}:")),
            "human and JSON must describe {id} with the same status"
        );
    }
}

#[test]
fn doctor_json_has_stable_check_schema() {
    let f = Fixture::pong();
    let v = json(&run(&f.socket, &["--format", "json"], true, ""));
    assert_eq!(v["command"], "doctor");
    assert!(v["status"].is_string());
    for c in v["checks"].as_array().unwrap() {
        for key in ["id", "status", "message", "suggestion"] {
            assert!(c.get(key).is_some(), "{key}");
        }
    }
}

#[test]
fn doctor_preflight_covers_locked_checks() {
    let f = Fixture::pong();
    let v = json(&run(&f.socket, &["--format", "json"], true, ""));
    let ids: Vec<_> = v["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, IDS);
}

#[test]
fn doctor_fail_exit_and_warn_success() {
    {
        let f = Fixture::pong();
        assert!(run(&f.socket, &["--format", "json"], false, "")
            .status
            .success());
    }
    let home = tempfile::tempdir().unwrap();
    let missing = home.path().join("missing.sock");
    assert_eq!(
        run(&missing, &["--format", "json"], false, "")
            .status
            .code(),
        Some(1)
    );
    let f = Fixture::pong();
    let home = tempfile::tempdir().unwrap();
    let cfg = home.path().join("bad.toml");
    fs::write(&cfg, "[ask]\nfilter = [\"broken\"]\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("HOME", home.path())
        .env("AI_CONFIG", &cfg)
        .env_remove("AI_FILTER")
        .args([
            "doctor",
            "--quiet",
            "--format",
            "json",
            "--socket",
            f.socket.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let v = json(&out);
    assert_eq!(v["checks"][4]["id"], "output_filter_configuration");
    assert_eq!(v["checks"][4]["status"], "fail");
    assert!(v["checks"][4]["suggestion"].is_string());
}

#[test]
fn doctor_does_not_call_external_llm_or_mutate() {
    let f = Fixture::pong();
    let before = fs::metadata(&f.socket).unwrap().modified().unwrap();
    let out = run(&f.socket, &["--format", "json"], true, "");
    assert!(out.status.success());
    assert_eq!(f.count.load(Ordering::SeqCst), 1);
    assert_eq!(fs::metadata(&f.socket).unwrap().modified().unwrap(), before);
}

#[test]
fn doctor_masks_filter_and_secret_values() {
    let f = Fixture::pong();
    let home = tempfile::tempdir().unwrap();
    let cfg = config(&home, &f.socket, "filter = \"SECRET_FILTER_COMMAND\"");
    let dir = home.path().join("session");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("log.jsonl"), "SECRET_LOG_BODY API_KEY_SECRET\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("HOME", home.path())
        .env("AI_CONFIG", cfg)
        .env("AISH_SESSION_DIR", dir)
        .env("AI_FILTER", "SECRET_FILTER_COMMAND")
        .args(["doctor", "--quiet", "--format", "json"])
        .output()
        .unwrap();
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    for secret in ["SECRET_FILTER_COMMAND", "SECRET_LOG_BODY", "API_KEY_SECRET"] {
        assert!(!all.contains(secret));
    }
}

#[test]
fn doctor_checks_continue_after_failure() {
    let home = tempfile::tempdir().unwrap();
    let out = run(
        &home.path().join("none.sock"),
        &["--format", "json"],
        false,
        "",
    );
    let v = json(&out);
    assert_eq!(v["checks"].as_array().unwrap().len(), 6);
    assert_eq!(v["checks"][0]["status"], "fail");
    assert_eq!(v["checks"][4]["status"], "ok");
}

#[test]
fn doctor_protocol_check_uses_existing_ping_contract() {
    let f = Fixture::server("not-json");
    let out = run(&f.socket, &["--format", "json"], false, "");
    let v = json(&out);
    assert_eq!(f.count.load(Ordering::SeqCst), 1);
    assert_eq!(v["checks"][0]["status"], "ok");
    assert_eq!(v["checks"][5]["status"], "fail");
}

#[test]
fn status_legacy_output_remains_compatible() {
    let home = tempfile::tempdir().unwrap();
    let cfg = config(&home, &home.path().join("none.sock"), "");
    for format in ["json", "tsv", "env"] {
        let out = Command::new(env!("CARGO_BIN_EXE_ai"))
            .env("HOME", home.path())
            .env("AI_CONFIG", &cfg)
            .args(["status", "--quiet", "--format", format])
            .output()
            .unwrap();
        assert!(out.status.success());
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.to_ascii_lowercase().contains("socket"));
        assert!(!text.contains("check.socket_reachable"));
    }
}

#[test]
fn doctor_tsv_env_remain_machine_readable() {
    for format in ["tsv", "env"] {
        let f = Fixture::pong();
        let out = run(&f.socket, &["--format", format], false, "");
        assert!(out.status.success());
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains(if format == "tsv" {
            "config.socket_path"
        } else {
            "AI_CONFIG_SOCKET_PATH"
        }));
        assert!(text.contains(if format == "tsv" {
            "check.socket_reachable.status"
        } else {
            "AI_DOCTOR_CHECK_SOCKET_REACHABLE_STATUS"
        }));
    }
}
