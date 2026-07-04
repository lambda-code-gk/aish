// RED stubs for 0055 Collaborative Human Handoff.
// #[ignore] until the corresponding phase lands (see scripts/spec-acceptance.toml).

use std::process::Command;

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

#[test]
fn docs_architecture_mentions_collaborative_handoff() {
    let path = workspace_root().join("docs/architecture.md");
    let text = std::fs::read_to_string(path).expect("architecture.md");
    assert!(text.contains("Collaborative human handoff"));
    assert!(text.contains("human shell"));
    assert!(text.contains("side agent"));
}

#[test]
fn manual_collaborative_handoff_checklist_exists() {
    let path = workspace_root().join("docs/manual/collaborative-handoff.md");
    let text = std::fs::read_to_string(path).expect("manual checklist");
    for needle in [
        "ai --collaborative",
        "Alt+.",
        "Ctrl+D",
        "ai resume",
        "ai status",
    ] {
        assert!(text.contains(needle), "missing manual item: {needle}");
    }
}

#[test]
fn normal_ai_entry_unchanged_regression() {
    let output = Command::new(env!("CARGO_BIN_EXE_ai"))
        .arg("--help")
        .env("AIBE_SOCKET_PATH", "/nonexistent/aibe.sock")
        .output()
        .expect("spawn ai --help");
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(help.contains("ask"));
    let dry = Command::new(env!("CARGO_BIN_EXE_ai"))
        .args(["ask", "--dry-run", "hello"])
        .env("AIBE_SOCKET_PATH", "/nonexistent/aibe.sock")
        .output()
        .expect("spawn ai ask --dry-run");
    assert!(dry.status.success());
    let stderr = String::from_utf8_lossy(&dry.stderr);
    assert!(!stderr.contains("collaborative handoff is not enabled"));
}
