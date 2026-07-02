//! `ai` suggested command recall の統合テスト。

use std::process::Command;

use ai::adapters::outbound::FileSuggestedCommandRecallStore;
use ai::application::{
    persist_suggested_commands, recall_next_command, recall_prev_command, resolve_recall_gating,
    RecallGatingInput, RecallTurnContext,
};
use ai::domain::OutputFormat;
use tempfile::TempDir;

#[test]
fn recall_cli_next_prints_cached_command() {
    let home = TempDir::new().expect("home");
    let cache_dir = home.path().join(".local/share/ai/suggestions");
    std::fs::create_dir_all(&cache_dir).expect("mkdir");
    let cache_path = cache_dir.join("sess-1.json");
    let gating = resolve_recall_gating(RecallGatingInput {
        config_enabled: true,
        config_hint: false,
        max_items: 8,
        quiet: true,
        output_format: None,
        stdin_tty: true,
        stdout_tty: true,
        stderr_tty: true,
    });
    let store = FileSuggestedCommandRecallStore::new(cache_path.clone());
    let ctx = RecallTurnContext {
        gating,
        ai_session_id: "sess-1".into(),
        conversation_id: None,
        turn_id: "turn-1".into(),
        captured_at: "1".into(),
        shell: "bash".into(),
    };
    let content = "```bash\ngit status\n```";
    persist_suggested_commands(&store, &ctx, content).expect("persist");
    let out = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("HOME", home.path())
        .env("AI_SUGGESTION_CACHE", &cache_path)
        .args(["recall", "next"])
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "git status");
}

#[test]
fn recall_cli_prev_prints_cached_command() {
    let home = TempDir::new().expect("home");
    let cache_dir = home.path().join(".local/share/ai/suggestions");
    std::fs::create_dir_all(&cache_dir).expect("mkdir");
    let cache_path = cache_dir.join("sess-1.json");
    let gating = resolve_recall_gating(RecallGatingInput {
        config_enabled: true,
        config_hint: false,
        max_items: 8,
        quiet: true,
        output_format: None,
        stdin_tty: true,
        stdout_tty: true,
        stderr_tty: true,
    });
    let store = FileSuggestedCommandRecallStore::new(cache_path.clone());
    let ctx = RecallTurnContext {
        gating,
        ai_session_id: "sess-1".into(),
        conversation_id: None,
        turn_id: "turn-1".into(),
        captured_at: "1".into(),
        shell: "bash".into(),
    };
    let content = "```bash\ngit status\n```\n\n```sh\ngit add -A\n```";
    persist_suggested_commands(&store, &ctx, content).expect("persist");
    let out = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("HOME", home.path())
        .env("AI_SUGGESTION_CACHE", &cache_path)
        .args(["recall", "prev"])
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "git add -A");
}

#[test]
fn recall_next_command_wraps_after_last_candidate() {
    let dir = TempDir::new().expect("tempdir");
    let store = FileSuggestedCommandRecallStore::new(dir.path().join("cache.json"));
    let gating = resolve_recall_gating(RecallGatingInput {
        config_enabled: true,
        config_hint: false,
        max_items: 8,
        quiet: false,
        output_format: None,
        stdin_tty: true,
        stdout_tty: true,
        stderr_tty: true,
    });
    let ctx = RecallTurnContext {
        gating,
        ai_session_id: "sess".into(),
        conversation_id: None,
        turn_id: "t1".into(),
        captured_at: "1".into(),
        shell: "bash".into(),
    };
    let content = "```bash\nfirst\n```\n\n```sh\nsecond\n```";
    persist_suggested_commands(&store, &ctx, content).expect("persist");
    assert_eq!(
        recall_next_command(&store).expect("next"),
        Some("first".into())
    );
    assert_eq!(
        recall_next_command(&store).expect("next"),
        Some("second".into())
    );
    assert_eq!(
        recall_next_command(&store).expect("next"),
        Some("first".into())
    );
}

#[test]
fn recall_prev_command_wraps_before_first_candidate() {
    let dir = TempDir::new().expect("tempdir");
    let store = FileSuggestedCommandRecallStore::new(dir.path().join("cache.json"));
    let gating = resolve_recall_gating(RecallGatingInput {
        config_enabled: true,
        config_hint: false,
        max_items: 8,
        quiet: false,
        output_format: None,
        stdin_tty: true,
        stdout_tty: true,
        stderr_tty: true,
    });
    let ctx = RecallTurnContext {
        gating,
        ai_session_id: "sess".into(),
        conversation_id: None,
        turn_id: "t1".into(),
        captured_at: "1".into(),
        shell: "bash".into(),
    };
    let content = "```bash\nfirst\n```\n\n```sh\nsecond\n```";
    persist_suggested_commands(&store, &ctx, content).expect("persist");
    assert_eq!(
        recall_prev_command(&store).expect("prev"),
        Some("second".into())
    );
    assert_eq!(
        recall_prev_command(&store).expect("prev"),
        Some("first".into())
    );
}

#[test]
fn ai_complete_bash_includes_recall_hook() {
    let out = Command::new(env!("CARGO_BIN_EXE_ai"))
        .args(["complete", "bash"])
        .output()
        .expect("run");
    assert!(out.status.success());
    let script = String::from_utf8_lossy(&out.stdout);
    assert!(script.contains(r#"bind -x '"\e.": "_ai_recall_next"'"#));
    assert!(script.contains(r#"bind -x '"\e,": "_ai_recall_prev"'"#));
    assert!(script.contains("AI_SUGGESTION_CACHE"));
}

#[test]
fn ai_complete_zsh_includes_recall_hook() {
    let out = Command::new(env!("CARGO_BIN_EXE_ai"))
        .args(["complete", "zsh"])
        .output()
        .expect("run");
    assert!(out.status.success());
    let script = String::from_utf8_lossy(&out.stdout);
    assert!(script.contains(r#"bindkey '\e.' _ai_recall_next"#));
    assert!(script.contains(r#"bindkey '\e,' _ai_recall_prev"#));
}

#[test]
fn recall_next_command_advances_cursor() {
    let dir = TempDir::new().expect("tempdir");
    let cache_path = dir.path().join("cache.json");
    let store = FileSuggestedCommandRecallStore::new(cache_path);
    let gating = resolve_recall_gating(RecallGatingInput {
        config_enabled: true,
        config_hint: false,
        max_items: 8,
        quiet: false,
        output_format: None,
        stdin_tty: true,
        stdout_tty: true,
        stderr_tty: true,
    });
    let ctx = RecallTurnContext {
        gating,
        ai_session_id: "sess".into(),
        conversation_id: None,
        turn_id: "t1".into(),
        captured_at: "1".into(),
        shell: "bash".into(),
    };
    let content = "```bash\ngit status\n```\n\n```sh\ngit add -A\n```";
    persist_suggested_commands(&store, &ctx, content).expect("persist");
    assert_eq!(
        recall_next_command(&store).expect("next"),
        Some("git status".into())
    );
    assert_eq!(
        recall_next_command(&store).expect("next"),
        Some("git add -A".into())
    );
    assert_eq!(
        recall_next_command(&store).expect("next"),
        Some("git status".into())
    );
}

#[test]
fn recall_next_and_prev_stay_in_sync() {
    let dir = TempDir::new().expect("tempdir");
    let store = FileSuggestedCommandRecallStore::new(dir.path().join("cache.json"));
    let gating = resolve_recall_gating(RecallGatingInput {
        config_enabled: true,
        config_hint: false,
        max_items: 8,
        quiet: false,
        output_format: None,
        stdin_tty: true,
        stdout_tty: true,
        stderr_tty: true,
    });
    let ctx = RecallTurnContext {
        gating,
        ai_session_id: "sess".into(),
        conversation_id: None,
        turn_id: "t1".into(),
        captured_at: "1".into(),
        shell: "bash".into(),
    };
    let content = "```bash\na\n```\n\n```sh\nb\n```\n\n```bash\nc\n```";
    persist_suggested_commands(&store, &ctx, content).expect("persist");
    assert_eq!(recall_next_command(&store).expect("next"), Some("a".into()));
    assert_eq!(recall_next_command(&store).expect("next"), Some("b".into()));
    assert_eq!(recall_prev_command(&store).expect("prev"), Some("a".into()));
    assert_eq!(recall_next_command(&store).expect("next"), Some("b".into()));
}

#[test]
fn structured_output_disables_suggested_command_recall_integration() {
    let gating = resolve_recall_gating(RecallGatingInput {
        config_enabled: true,
        config_hint: true,
        max_items: 8,
        quiet: false,
        output_format: Some(OutputFormat::Json),
        stdin_tty: true,
        stdout_tty: true,
        stderr_tty: true,
    });
    assert!(!gating.enabled);
}
