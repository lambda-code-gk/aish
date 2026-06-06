use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn shell_exports_ai_ask_log_and_aish_session_dir_to_child_shell() {
    let home = tempfile::tempdir().expect("home");
    let script = home.path().join("probe-shell.sh");
    fs::write(
        &script,
        "#!/bin/sh\nprintf 'AI_ASK_LOG=%s\\n' \"$AI_ASK_LOG\"\nprintf 'AISH_SESSION_DIR=%s\\n' \"$AISH_SESSION_DIR\"\nexit 0\n",
    )
    .expect("write script");
    let mut perms = fs::metadata(&script).expect("meta").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script, perms).expect("chmod");

    let out = Command::new(env!("CARGO_BIN_EXE_aish"))
        .env("HOME", home.path())
        .env("SHELL", &script)
        .arg("shell")
        .output()
        .expect("run shell");

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("utf8");
    assert!(stdout.contains("AI_ASK_LOG=session"));
    assert!(stdout.contains("AISH_SESSION_DIR="));
}
