//! 0055 minimal human handoff acceptance tests (aish).

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn set_dir_mode_0700(path: &Path) {
    let mut perms = std::fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o700);
    std::fs::set_permissions(path, perms).unwrap();
}

fn private_temp_home() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    set_dir_mode_0700(home.path());
    home
}

fn resolve_zsh_binary() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("ZSH") {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    if let Ok(paths) = std::env::var("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join("zsh");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    std::process::Command::new("sh")
        .args(["-c", "command -v zsh"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8(o.stdout)
                .ok()
                .map(|s| PathBuf::from(s.trim()))
        })
        .filter(|p| p.is_file())
}

fn run_human_shell_with(
    input: &[u8],
    home: &Path,
    suggested_command: &str,
    extra_env: &[(&str, &str)],
) -> (std::process::Output, aish::human_shell::HumanShellResult) {
    if !Path::new("/bin/bash").is_file() {
        panic!("human-shell tests require /bin/bash");
    }
    let result_file = home.join("result.json");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_aish"));
    cmd.args(["human-shell", "--result-file"])
        .arg(&result_file)
        .env("HOME", home)
        .env("SHELL", "/bin/bash")
        .env("AISH_CONTROL_MODE", "human-shell")
        .env("AISH_HANDOFF_PARENT_REQUEST", "create marker file")
        .env("AISH_HANDOFF_SUGGESTED_COMMAND", suggested_command);
    for (key, value) in extra_env {
        cmd.env(key, value);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let _ = child.stdin.take().unwrap().write_all(input);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let output = rx
        .recv_timeout(Duration::from_secs(12))
        .expect("human shell hung")
        .unwrap();
    let result = serde_json::from_str(&std::fs::read_to_string(result_file).unwrap()).unwrap();
    (output, result)
}

fn run_human_shell(input: &[u8]) -> (std::process::Output, aish::human_shell::HumanShellResult) {
    let home = private_temp_home();
    let marker = home.path().join("must-not-exist-before-human-input");
    let suggested = format!("touch {}", shell_quote(marker.to_str().unwrap()));
    run_human_shell_with(input, home.path(), &suggested, &[])
}

#[test]
fn human_shell_ctrl_d_returns_control_to_parent() {
    let (output, actual) = run_human_shell(b"\x04");
    assert!(output.status.success());
    assert!(actual.normal_return);
}

#[test]
fn human_shell_exit_returns_control_regardless_of_code() {
    for input in [b"exit\n".as_slice(), b"exit 1\n".as_slice()] {
        let (output, actual) = run_human_shell(input);
        assert!(output.status.success());
        assert!(actual.normal_return);
        assert!(actual.exit_code.is_some());
    }
}

#[test]
fn suggested_command_is_not_auto_executed() {
    let home = private_temp_home();
    let marker = home.path().join("must-not-exist-before-human-input");
    let suggested = format!("touch {}", shell_quote(marker.to_str().unwrap()));
    assert!(
        !marker.exists(),
        "marker must not exist before human shell starts"
    );
    let (output, _) = run_human_shell_with(b"exit\n", home.path(), &suggested, &[]);
    assert!(output.status.success());
    assert!(
        !marker.exists(),
        "suggested command must not be auto-executed"
    );
}

#[test]
fn human_shell_startup_prints_parent_request_and_suggested_command() {
    let home = private_temp_home();
    let marker = home.path().join("marker");
    let suggested = format!("touch {}", shell_quote(marker.to_str().unwrap()));
    let (output, _) = run_human_shell_with(b"exit\n", home.path(), &suggested, &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("AISH Collaborative Mode"));
    assert!(stderr.contains("Human Task"));
    assert!(stderr.contains("create marker file"));
    assert!(stderr.contains(&aish::human_shell::escape_for_handoff_display(&suggested)));
    assert!(stderr.contains("Alt+. or Alt+,"));
    assert!(stderr.contains("Suggested first action:"));
    assert!(!stderr.contains("Human control requested by the parent agent."));
}

#[test]
fn bash_human_return_marker() {
    let (output, actual) = run_human_shell(b"exit\n");
    assert!(output.status.success());
    assert!(actual.normal_return);
}

#[test]
fn zsh_human_return_marker() {
    // verify.sh が AISH_0055_ZSH=1 で明示実行する。通常の cargo test では
    // zsh があっても skip し、ハング耐性のない二重実行を避ける。
    if std::env::var("AISH_0055_ZSH").is_err() {
        eprintln!("skipping zsh_human_return_marker: not under verify (set AISH_0055_ZSH=1)");
        return;
    }
    let zsh = resolve_zsh_binary().unwrap_or_else(|| {
        panic!("AISH_0055_ZSH=1 but zsh binary not found");
    });
    let home = private_temp_home();
    let result_file = home.path().join("result.json");
    let mut child = Command::new(env!("CARGO_BIN_EXE_aish"))
        .args(["human-shell", "--result-file"])
        .arg(&result_file)
        .env("HOME", home.path())
        .env("SHELL", &zsh)
        .env("AISH_CONTROL_MODE", "human-shell")
        .env("AISH_HANDOFF_PARENT_REQUEST", "zsh handoff")
        .env("AISH_HANDOFF_SUGGESTED_COMMAND", "true")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(400));
    let _ = child.stdin.take().unwrap().write_all(b"exit\n");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let output = rx
        .recv_timeout(Duration::from_secs(12))
        .expect("zsh human shell hung")
        .unwrap();
    assert!(
        output.status.success(),
        "status={:?} stderr={} stdout={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let actual: aish::human_shell::HumanShellResult =
        serde_json::from_str(&std::fs::read_to_string(result_file).unwrap()).unwrap();
    assert!(actual.normal_return);
}

#[test]
fn unsupported_shell_fails_before_interactive_launch() {
    let home = private_temp_home();
    let result_file = home.path().join("result.json");
    let output = Command::new(env!("CARGO_BIN_EXE_aish"))
        .args(["human-shell", "--result-file"])
        .arg(&result_file)
        .env("HOME", home.path())
        .env("SHELL", "/bin/dash")
        .env("AISH_CONTROL_MODE", "human-shell")
        .env("AISH_HANDOFF_PARENT_REQUEST", "test")
        .env("AISH_HANDOFF_SUGGESTED_COMMAND", "true")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("minimal human handoff currently supports bash and zsh only"),
        "stderr={stderr}"
    );
    assert!(!result_file.exists());
}

#[test]
fn human_shell_child_cannot_read_handoff_env_vars() {
    let home = private_temp_home();
    let script = format!(
        "printf 'MODE=%s\\nREQ=%s\\nSUG=%s\\nRT=%s\\n' \
         \"${{AISH_CONTROL_MODE:-}}\" \
         \"${{AISH_HANDOFF_PARENT_REQUEST:-}}\" \
         \"${{AISH_HANDOFF_SUGGESTED_COMMAND:-}}\" \
         \"${{AISH_HANDOFF_RUNTIME_DIR:-}}\"; exit\n"
    );
    let (output, _) = run_human_shell_with(script.as_bytes(), home.path(), "true", &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("MODE=\n") || stdout.contains("MODE=\r\n"));
    assert!(stdout.contains("REQ=\n") || stdout.contains("REQ=\r\n"));
    assert!(stdout.contains("SUG=\n") || stdout.contains("SUG=\r\n"));
    assert!(stdout.contains("RT=\n") || stdout.contains("RT=\r\n"));
}

#[test]
fn human_shell_ai_is_independent() {
    let home = private_temp_home();
    let stub = home.path().join("ai-stub");
    std::fs::write(
        &stub,
        r#"#!/bin/sh
printf 'COLLAB=%s\nCONV=%s\nCTX=%s\n' \
  "${AI_COLLABORATIVE:-}" \
  "${AI_CONVERSATION_ID:-}" \
  "${AI_PARENT_REQUEST:-}" >"$HOME/ai-invocation.txt"
exit 0
"#,
    )
    .unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let input = format!("{}\nexit\n", stub.display());
    let (output, _) = run_human_shell_with(input.as_bytes(), home.path(), "true", &[]);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let record_path = home.path().join("ai-invocation.txt");
    assert!(record_path.is_file(), "ai stub must run inside human shell");
    let record = std::fs::read_to_string(record_path).unwrap();
    assert!(record.contains("COLLAB=\n"));
    assert!(record.contains("CONV=\n"));
    assert!(record.contains("CTX=\n"));
}

#[test]
fn human_shell_starts_in_requested_cwd() {
    let home = private_temp_home();
    let work = home.path().join("workdir");
    std::fs::create_dir_all(&work).unwrap();
    let marker = home.path().join("cwd-marker");
    let canonical = work.canonicalize().unwrap();
    let input = format!("pwd > {}\nexit\n", shell_quote(marker.to_str().unwrap()));
    let result_file = home.path().join("result.json");
    let mut child = Command::new(env!("CARGO_BIN_EXE_aish"))
        .args(["human-shell", "--result-file"])
        .arg(&result_file)
        .current_dir(&work)
        .env("HOME", home.path())
        .env("SHELL", "/bin/bash")
        .env("AISH_CONTROL_MODE", "human-shell")
        .env("AISH_HANDOFF_PARENT_REQUEST", "cwd test")
        .env("AISH_HANDOFF_SUGGESTED_COMMAND", "true")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let _ = child.stdin.take().unwrap().write_all(input.as_bytes());
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let written = std::fs::read_to_string(&marker).unwrap();
    assert_eq!(written.trim(), canonical.to_string_lossy());
}

#[test]
fn handoff_result_file_is_0600() {
    let home = private_temp_home();
    let runtime = home.path().join("runtime");
    std::fs::create_dir_all(&runtime).unwrap();
    set_dir_mode_0700(&runtime);
    let result_file = runtime.join("result.json");
    let (output, _) = {
        let mut child = Command::new(env!("CARGO_BIN_EXE_aish"))
            .args(["human-shell", "--result-file"])
            .arg(&result_file)
            .env("HOME", home.path())
            .env("SHELL", "/bin/bash")
            .env("AISH_CONTROL_MODE", "human-shell")
            .env("AISH_HANDOFF_PARENT_REQUEST", "perm test")
            .env("AISH_HANDOFF_SUGGESTED_COMMAND", "true")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let _ = child.stdin.take().unwrap().write_all(b"exit\n");
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(child.wait_with_output());
        });
        let output = rx
            .recv_timeout(Duration::from_secs(8))
            .expect("human shell hung")
            .unwrap();
        (output, ())
    };
    assert!(output.status.success());
    assert!(result_file.is_file());
    let mode = std::fs::metadata(&result_file)
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "result.json must be 0600");
}

#[test]
fn terminal_disconnect_does_not_create_normal_return_marker() {
    let home = private_temp_home();
    let result_file = home.path().join("result.json");
    let mut child = Command::new(env!("CARGO_BIN_EXE_aish"))
        .args(["human-shell", "--result-file"])
        .arg(&result_file)
        .env("HOME", home.path())
        .env("SHELL", "/bin/bash")
        .env("AISH_CONTROL_MODE", "human-shell")
        .env("AISH_HANDOFF_PARENT_REQUEST", "disconnect test")
        .env("AISH_HANDOFF_SUGGESTED_COMMAND", "true")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    drop(child.stdin.take());
    let deadline = Instant::now() + Duration::from_secs(2);
    let output: Option<std::process::Output> = loop {
        match child.try_wait() {
            Ok(Some(_)) => break Some(child.wait_with_output().unwrap()),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Err(err) => panic!("wait failed: {err}"),
        }
    };
    if let Some(output) = output {
        if output.status.success() && result_file.is_file() {
            let result: aish::human_shell::HumanShellResult =
                serde_json::from_str(&std::fs::read_to_string(&result_file).unwrap()).unwrap();
            assert!(
                !result.normal_return,
                "stdin EOF must not synthesize Ctrl+D normal return"
            );
        } else {
            assert!(
                !result_file.is_file(),
                "disconnect must not write a normal-return result file"
            );
        }
    } else {
        assert!(
            !result_file.is_file(),
            "hung shell must not have written normal-return marker"
        );
    }
}
