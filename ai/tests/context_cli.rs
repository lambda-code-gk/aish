//! `ai context current/use/new` の統合テスト（0035）。
#![cfg(unix)]

use std::fs;
use std::process::Command;

fn ai_context(config_path: &std::path::Path, home: &std::path::Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ai"));
    cmd.env("AI_CONFIG", config_path)
        .env("HOME", home)
        .env_remove("AIBE_CONTEXT_ID")
        .arg("context")
        .args(args);
    cmd
}

#[test]
fn context_use_updates_config_and_current_reads_it() {
    let home = tempfile::tempdir().expect("home");
    let config_path = home.path().join("config.toml");

    let out = ai_context(&config_path, home.path(), &["use", "ctx_a"])
        .output()
        .expect("run context use");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let config = fs::read_to_string(&config_path).expect("config written");
    assert!(config.contains("[context]"), "config={config}");
    assert!(config.contains(r#"current = "ctx_a""#), "config={config}");

    let out = ai_context(&config_path, home.path(), &["current"])
        .output()
        .expect("run context current");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("memory_space_id: ctx_a"), "stdout={stdout}");
    assert!(stdout.contains("source: config"), "stdout={stdout}");
    assert!(
        stdout.contains("session_id (provenance):"),
        "stdout={stdout}"
    );
}

#[test]
fn context_new_sets_current() {
    let home = tempfile::tempdir().expect("home");
    let config_path = home.path().join("config.toml");

    let out = ai_context(&config_path, home.path(), &["new", "ctx_b"])
        .output()
        .expect("run context new");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let config = fs::read_to_string(&config_path).expect("config written");
    assert!(config.contains(r#"current = "ctx_b""#), "config={config}");
}

#[test]
fn env_context_overrides_config_in_current() {
    let home = tempfile::tempdir().expect("home");
    let config_path = home.path().join("config.toml");
    fs::write(&config_path, "[context]\ncurrent = \"ctx_cfg\"\n").expect("seed config");

    let out = ai_context(&config_path, home.path(), &["current"])
        .env("AIBE_CONTEXT_ID", "ctx_env")
        .output()
        .expect("run context current");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("memory_space_id: ctx_env"),
        "stdout={stdout}"
    );
    assert!(
        stdout.contains("source: AIBE_CONTEXT_ID"),
        "stdout={stdout}"
    );
}

#[test]
fn context_use_rejects_path_unsafe_names() {
    let home = tempfile::tempdir().expect("home");
    let config_path = home.path().join("config.toml");

    for name in ["../evil", "has/slash", "..", "."] {
        let out = ai_context(&config_path, home.path(), &["use", name])
            .output()
            .expect("run context use");
        assert!(!out.status.success(), "name {name} must be rejected");
        assert!(
            !config_path.exists(),
            "config must not be written for {name}"
        );
    }
}

#[test]
fn context_use_does_not_destroy_broken_config() {
    let home = tempfile::tempdir().expect("home");
    let config_path = home.path().join("config.toml");
    fs::write(&config_path, "this is not [valid toml").expect("seed broken config");

    let out = ai_context(&config_path, home.path(), &["use", "ctx_a"])
        .output()
        .expect("run context use");
    assert!(
        !out.status.success(),
        "broken config must not be overwritten"
    );
    let config = fs::read_to_string(&config_path).expect("config still present");
    assert_eq!(config, "this is not [valid toml");
}
