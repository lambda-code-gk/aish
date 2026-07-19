//! 0060 Collab Mode Human Task Briefing acceptance tests.

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use aish::human_shell::{
    escape_for_handoff_display, render_human_task_briefing, HANDOFF_ENV_KEYS,
    HANDOFF_SUGGESTIONS_FILENAME,
};

#[test]
fn human_task_briefing_renders_collaborative_mode_header() {
    let rendered = render_human_task_briefing("Run the project tests", "cargo test");
    assert_eq!(
        rendered,
        concat!(
            "AISH Collaborative Mode\n",
            "=======================\n",
            "\n",
            "Human Task\n",
            "\n",
            "Objective:\n",
            "  Run the project tests\n",
            "\n",
            "Why this is a Human Task:\n",
            "  The parent agent requested a shell operation in Collab Mode.\n",
            "  AISH has not automatically executed the requested command.\n",
            "\n",
            "Suggested first action:\n",
            "  cargo test\n",
            "\n",
            "Done when:\n",
            "  Return control after you have completed the necessary work,\n",
            "  or when the parent agent should re-observe the environment\n",
            "  and decide the next step.\n",
            "\n",
            "You remain in control:\n",
            "  Edit, run, replace, or ignore the suggested command.\n",
            "  Alt+. or Alt+, inserts the suggested command.\n",
            "  Press Ctrl+D or run `exit` to return control.\n",
        )
    );
}

#[test]
fn human_task_briefing_renders_objective() {
    let rendered = render_human_task_briefing("Run the project tests", "cargo test");
    assert!(rendered.contains("Objective:\n  Run the project tests\n"));
    let empty = render_human_task_briefing("   \t  ", "cargo test");
    assert!(empty.contains("Objective:\n  No parent request summary is available.\n"));
}

#[test]
fn human_task_briefing_uses_fixed_reason() {
    let rendered = render_human_task_briefing(
        "somehow invent a custom reason from this text",
        "cargo test",
    );
    assert!(rendered.contains("Why this is a Human Task:"));
    assert!(rendered.contains("The parent agent requested a shell operation in Collab Mode."));
    // Objective には原文が出るが、理由欄は固定文のみで推測しない。
    let reason = rendered
        .split("Why this is a Human Task:")
        .nth(1)
        .unwrap()
        .split("Suggested first action:")
        .next()
        .unwrap();
    assert!(!reason.contains("somehow invent"));
    assert!(!reason.contains("custom reason"));
}

#[test]
fn human_task_briefing_renders_suggested_first_action() {
    let rendered = render_human_task_briefing("obj", "cargo test");
    assert!(rendered.contains("Suggested first action:\n  cargo test\n"));
    let empty = render_human_task_briefing("obj", " \n\t ");
    assert!(empty.contains("Suggested first action:\n  No command was provided.\n"));
}

#[test]
fn human_task_briefing_states_command_not_executed() {
    let rendered = render_human_task_briefing("Run the project tests", "cargo test");
    assert!(rendered.contains("AISH has not automatically executed the requested command."));
}

#[test]
fn human_task_briefing_preserves_user_control() {
    let rendered = render_human_task_briefing("obj", "cargo test");
    assert!(rendered.contains("You remain in control:"));
    assert!(rendered.contains("Edit, run, replace, or ignore the suggested command."));
    assert!(rendered.contains("Alt+. or Alt+, inserts the suggested command."));
}

#[test]
fn human_task_briefing_renders_done_when() {
    let rendered = render_human_task_briefing("obj", "cargo test");
    assert!(rendered.contains("Done when:"));
    assert!(rendered.contains("Return control after you have completed the necessary work,"));
    assert!(rendered.contains("or when the parent agent should re-observe the environment"));
}

#[test]
fn human_task_briefing_returns_with_ctrl_d_or_exit() {
    let rendered = render_human_task_briefing("obj", "cargo test");
    assert!(rendered.contains("Press Ctrl+D or run `exit` to return control."));

    if !Path::new("/bin/bash").is_file() {
        panic!("human-shell tests require /bin/bash");
    }
    let home = private_temp_home();
    let (output, result) = run_human_shell_with(b"exit\n", home.path(), "true", &[]);
    assert!(output.status.success());
    assert_eq!(result.outcome, aish::human_shell::HumanShellOutcome::Done);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("作業結果を選択してください"));
    assert!(!stderr.contains("サマリを入力"));
    assert!(!stderr.contains("実施した作業や結果を入力"));
}

#[test]
fn human_task_briefing_indents_multiline_objective() {
    let rendered = render_human_task_briefing("line one\nline two", "cmd\none");
    assert!(rendered.contains("Objective:\n  line one\n  line two\n"));
    assert!(rendered.contains("Suggested first action:\n  cmd\n  one\n"));
    assert!(!rendered.contains("\\n"));
}

#[test]
fn human_task_briefing_sanitizes_ansi_and_c0() {
    let objective = "keep\x1b[31mred\x07\tend";
    let suggested = "echo\x1b]0;evil\x07hi";
    let rendered = render_human_task_briefing(objective, suggested);
    assert!(!rendered.contains('\x1b'));
    assert!(!rendered.contains('\x07'));
    assert!(!rendered.contains('\t'));
    assert!(rendered.contains(&escape_for_handoff_display("keep")));
    assert!(rendered.contains("\\x1b"));
}

#[test]
fn human_task_briefing_renderer_is_pure() {
    let a = render_human_task_briefing("obj", "cmd");
    let b = render_human_task_briefing("obj", "cmd");
    assert_eq!(a, b);
}

#[test]
fn human_task_briefing_printer_only_reads_env_and_stderr() {
    if !Path::new("/bin/bash").is_file() {
        panic!("human-shell tests require /bin/bash");
    }
    let home = private_temp_home();
    let parent = "printer parent request";
    let suggested = "echo printer-suggested";
    let (output, _) = run_human_shell_with(
        b"exit\n",
        home.path(),
        suggested,
        &[("AISH_HANDOFF_PARENT_REQUEST", parent)],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let expected = render_human_task_briefing(parent, suggested);
    assert!(
        stderr.starts_with(&expected) || stderr.contains(&expected),
        "stderr must include rendered briefing"
    );
}

#[test]
fn human_task_briefing_has_no_outcome_selection() {
    let home = private_temp_home();
    let (output, _) = run_human_shell_with(b"exit\n", home.path(), "true", &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("作業結果を選択してください"));
    assert!(!stderr.contains("[d] done"));
    assert!(!stderr.contains("[b] blocked"));
    assert!(!stderr.contains("[c] cancelled"));
}

#[test]
fn human_task_briefing_has_no_summary_input() {
    let home = private_temp_home();
    let (output, _) = run_human_shell_with(b"exit\n", home.path(), "true", &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("サマリを入力"));
    assert!(!stderr.contains("実施した作業や結果を入力"));
    assert!(!stderr.contains("作業結果を選択してください"));
}

#[test]
fn human_task_briefing_uses_only_existing_env() {
    assert_eq!(HANDOFF_ENV_KEYS.len(), 5);
    assert!(HANDOFF_ENV_KEYS.contains(&"AISH_CONTROL_MODE"));
    assert!(HANDOFF_ENV_KEYS.contains(&"AISH_HANDOFF_PARENT_REQUEST"));
    assert!(HANDOFF_ENV_KEYS.contains(&"AISH_HANDOFF_SUGGESTED_COMMAND"));
    assert!(HANDOFF_ENV_KEYS.contains(&"AISH_HANDOFF_RUNTIME_DIR"));
    assert!(HANDOFF_ENV_KEYS.contains(&"AISH_HANDOFF_TASK_JSON"));
}

#[test]
fn explicit_human_task_seeds_alt_period_candidates_from_instructions() {
    if !Path::new("/bin/bash").is_file() {
        panic!("human-shell tests require /bin/bash");
    }
    let home = private_temp_home();
    let runtime = home.path().join("runtime");
    std::fs::create_dir_all(&runtime).unwrap();
    let mut perms = std::fs::metadata(&runtime).unwrap().permissions();
    perms.set_mode(0o700);
    std::fs::set_permissions(&runtime, perms).unwrap();
    let task_json =
        r#"{"version":1,"objective":"inspect","instructions":["cargo test","git status"]}"#;
    let (output, _) = run_human_shell_with(
        b"exit\n",
        home.path(),
        "",
        &[
            ("AISH_HANDOFF_TASK_JSON", task_json),
            (
                "AISH_HANDOFF_RUNTIME_DIR",
                runtime.to_str().expect("utf8 runtime"),
            ),
        ],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Suggested actions:"));
    assert!(stderr.contains("Alt+. or Alt+, inserts a suggested action."));
    let suggestions = std::fs::read(runtime.join(HANDOFF_SUGGESTIONS_FILENAME)).unwrap();
    assert_eq!(suggestions, b"cargo test\0git status\0");
}

#[test]
fn human_task_briefing_creates_no_persistent_state() {
    let home = private_temp_home();
    let before = snapshot_paths(home.path());
    let (_output, result) = run_human_shell_with(b"exit\n", home.path(), "true", &[]);
    assert_eq!(result.outcome, aish::human_shell::HumanShellOutcome::Done);
    let after = snapshot_paths(home.path());
    // human-shell may create session logs under HOME; ensure no outcome/task state files.
    for path in &after {
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        assert!(
            !name.contains("human_task") && !name.contains("briefing_state"),
            "unexpected persistent state path: {}",
            path.display()
        );
    }
    let _ = before;
}

#[test]
fn human_task_briefing_normal_shell_exec_regression() {
    // Human Task briefing is only for AISH_CONTROL_MODE=human-shell.
    // Non-handoff `aish exec` must not print Collaborative Mode briefing.
    let home = private_temp_home();
    let child = Command::new(env!("CARGO_BIN_EXE_aish"))
        .args(["exec", "--", "true"])
        .env("HOME", home.path())
        .env("SHELL", "/bin/bash")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let output = rx
        .recv_timeout(Duration::from_secs(12))
        .expect("aish exec hung")
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("AISH Collaborative Mode"));
    assert!(!stderr.contains("Human Task"));
    assert!(!stderr.contains("作業結果を選択してください"));
}

fn private_temp_home() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    let mut perms = std::fs::metadata(home.path()).unwrap().permissions();
    perms.set_mode(0o700);
    std::fs::set_permissions(home.path(), perms).unwrap();
    home
}

fn run_human_shell_with(
    input: &[u8],
    home: &Path,
    suggested_command: &str,
    extra_env: &[(&str, &str)],
) -> (std::process::Output, aish::human_shell::HumanShellResult) {
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
    let _ = child.stdin.take().unwrap().write_all(input);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let output = rx
        .recv_timeout(Duration::from_secs(12))
        .expect("human shell hung")
        .unwrap();
    let result_text = std::fs::read_to_string(&result_file).unwrap_or_else(|error| {
        panic!(
            "missing result file {}: status={:?} stderr={} stdout={} err={error}",
            result_file.display(),
            output.status.code(),
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        )
    });
    let result = serde_json::from_str(&result_text).unwrap();
    (output, result)
}

fn snapshot_paths(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path.clone());
            }
            out.push(path);
        }
    }
    out
}
