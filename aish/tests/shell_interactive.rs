use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// replay hook 有効時でも bash がプロンプトを出してコマンドを受け付ける（0049 デッドロック回帰）。
#[test]
fn shell_bash_accepts_commands_without_hanging() {
    if !std::path::Path::new("/bin/bash").is_file() {
        return;
    }

    let home = tempfile::tempdir().expect("home");
    let mut child = Command::new(env!("CARGO_BIN_EXE_aish"))
        .arg("shell")
        .env("HOME", home.path())
        .env("SHELL", "/bin/bash")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn aish shell");

    {
        let mut stdin = child.stdin.take().expect("stdin");
        stdin
            .write_all(b"echo aish-shell-ok\nexit\n")
            .expect("write commands");
    }

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });

    let output = rx
        .recv_timeout(Duration::from_secs(8))
        .expect("aish shell hung before completing")
        .expect("wait");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("aish-shell-ok"),
        "expected command output in stdout, got: {stdout}"
    );
}

/// PTY の CRLF 出力が span に記録され replay show で復元できること。
#[test]
fn shell_replay_show_includes_command_output() {
    if !std::path::Path::new("/bin/bash").is_file() {
        return;
    }

    let home = tempfile::tempdir().expect("home");
    let mut child = Command::new(env!("CARGO_BIN_EXE_aish"))
        .arg("shell")
        .env("HOME", home.path())
        .env("SHELL", "/bin/bash")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn aish shell");

    {
        let mut stdin = child.stdin.take().expect("stdin");
        stdin
            .write_all(b"echo aish-only-output\n")
            .expect("write echo");
        thread::sleep(Duration::from_millis(250));
        stdin.write_all(b"exit\n").expect("write exit");
    }

    let output = child.wait_with_output().expect("wait");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let log_path = stderr
        .lines()
        .find_map(|line| line.strip_prefix("aish: log written to "))
        .expect("session log path in stderr");

    let replay = Command::new(env!("CARGO_BIN_EXE_aish"))
        .args(["replay", "show", "1", "--log", log_path])
        .output()
        .expect("replay show");

    assert!(
        replay.status.success(),
        "replay stderr: {}",
        String::from_utf8_lossy(&replay.stderr)
    );
    let replay_stdout = String::from_utf8_lossy(&replay.stdout);
    assert_eq!(replay_stdout, "aish-only-output\n");
}
