use std::fs;
use std::os::unix::fs::symlink;
use std::process::Command;

fn setup_session_dir() -> (tempfile::TempDir, std::path::PathBuf) {
    let root = tempfile::tempdir().expect("tempdir");
    let session = root.path().join("002f15d02b54");
    fs::create_dir(&session).expect("mkdir");
    fs::write(session.join("log.jsonl"), "").expect("log");
    symlink("log.jsonl", session.join("current_log")).expect("symlink");
    (root, session)
}

#[test]
fn session_subcommand_default_tsv() {
    let (_root, session) = setup_session_dir();
    let out = Command::new(env!("CARGO_BIN_EXE_aish"))
        .env("AISH_SESSION_DIR", &session)
        .arg("session")
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("utf8");
    assert!(stdout.contains("session_id\t002f15d02b54"));
}

#[test]
fn session_subcommand_json_format() {
    let (_root, session) = setup_session_dir();
    let out = Command::new(env!("CARGO_BIN_EXE_aish"))
        .env("AISH_SESSION_DIR", &session)
        .args(["session", "--format", "json"])
        .output()
        .expect("run");
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(v["session_id"], "002f15d02b54");
}

#[test]
fn shell_rejects_extra_args() {
    let out = Command::new(env!("CARGO_BIN_EXE_aish"))
        .args(["shell", "bogus"])
        .output()
        .expect("run");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("bogus"));
}

#[test]
fn session_subcommand_env_not_set() {
    let out = Command::new(env!("CARGO_BIN_EXE_aish"))
        .env_remove("AISH_SESSION_DIR")
        .arg("session")
        .output()
        .expect("run");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("AISH_SESSION_DIR"));
}
