use std::ffi::CString;
use std::fs;
use std::io::{BufRead, BufReader};
use std::process::Command;

use aish::adapters::outbound::{accept_human_terminal_control, prepare_interactive_rc};

#[test]
fn human_task_suspend_function_is_ephemeral() {
    let home = tempfile::tempdir().unwrap();
    let bashrc = home.path().join(".bashrc");
    let zshrc = home.path().join(".zshrc");
    fs::write(&bashrc, "export USER_RC_SENTINEL=bash\n").unwrap();
    fs::write(&zshrc, "export USER_RC_SENTINEL=zsh\n").unwrap();
    let path_before = std::env::var_os("PATH");
    std::env::set_var("HOME", home.path());
    let bash = prepare_interactive_rc("/bin/bash").unwrap().unwrap();
    let bash_path = bash.bash_rcfile.as_ref().unwrap();
    let text = fs::read_to_string(bash_path).unwrap();
    assert!(text.contains("human-task()"));
    assert!(text.contains("_AISH_EXPLICIT_HUMAN_TASK"));
    assert!(text.contains("4096"));
    assert!(text.contains("__human-task-suspend"));
    assert!(!text.contains("\"event\":\"human_suspend\""));
    assert!(Command::new("bash")
        .arg("-n")
        .arg(bash_path)
        .status()
        .unwrap()
        .success());
    assert_eq!(
        fs::read_to_string(&bashrc).unwrap(),
        "export USER_RC_SENTINEL=bash\n"
    );
    assert_eq!(std::env::var_os("PATH"), path_before);
}

#[test]
fn human_task_suspend_helper_validates_and_emits_versioned_event() {
    let dir = tempfile::tempdir().unwrap();
    let fifo = dir.path().join("control.fifo");
    let fifo_c = CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) }, 0);
    let reader_path = fifo.clone();
    let reader = std::thread::spawn(move || {
        let mut line = String::new();
        BufReader::new(fs::File::open(reader_path).unwrap())
            .read_line(&mut line)
            .unwrap();
        line
    });
    let output = Command::new(env!("CARGO_BIN_EXE_aish"))
        .env("AISH_CONTROL_FIFO", &fifo)
        .args([
            "__human-task-suspend",
            "--reason",
            "approval needed",
            "--cwd",
            "/tmp/final",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let line = reader.join().unwrap();
    let event: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(event["version"], 1);
    assert_eq!(event["event"], "human_suspend");

    let rejected = Command::new(env!("CARGO_BIN_EXE_aish"))
        .env("AISH_CONTROL_FIFO", &fifo)
        .args([
            "__human-task-suspend",
            "--reason",
            "unsafe\u{85}reason",
            "--cwd",
            "/tmp/final",
        ])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(!String::from_utf8_lossy(&rejected.stderr).contains("unsafe"));
}

#[test]
fn human_task_suspend_first_terminal_event_wins() {
    let mut marker = None;
    accept_human_terminal_control(
        &mut marker,
        r#"{"version":1,"event":"human_suspend","exit_code":0,"cwd":"/tmp/final","reason":"approval needed"}"#,
    );
    accept_human_terminal_control(
        &mut marker,
        r#"{"event":"human_return","exit_code":0,"cwd":"/wrong"}"#,
    );
    let marker = marker.unwrap();
    assert!(marker.suspended);
    assert_eq!(marker.final_cwd, "/tmp/final");
    assert_eq!(marker.suspend_reason.as_deref(), Some("approval needed"));

    let mut invalid = None;
    accept_human_terminal_control(
        &mut invalid,
        "{\"version\":1,\"event\":\"human_suspend\",\"exit_code\":0,\"cwd\":\"/tmp\",\"reason\":\"unsafe\\u0085reason\"}",
    );
    assert!(invalid.is_none());
}

#[test]
fn human_task_suspend_preserves_bash_zsh_and_prior_stages() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("HOME", home.path());
    let bash = prepare_interactive_rc("bash").unwrap().unwrap();
    assert!(Command::new("bash")
        .arg("-n")
        .arg(bash.bash_rcfile.unwrap())
        .status()
        .unwrap()
        .success());
    if Command::new("zsh").arg("--version").output().is_ok() {
        let zsh = prepare_interactive_rc("zsh").unwrap().unwrap();
        let path = zsh.zdotdir.unwrap().join(".zshrc");
        assert!(Command::new("zsh")
            .arg("-n")
            .arg(path)
            .status()
            .unwrap()
            .success());
    }
    let mut marker = None;
    accept_human_terminal_control(
        &mut marker,
        r#"{"event":"human_return","exit_code":0,"cwd":"/tmp"}"#,
    );
    assert!(!marker.unwrap().suspended);
}
