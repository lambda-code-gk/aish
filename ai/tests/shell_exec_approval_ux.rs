//! `shell_exec` 承認 UX の統合テスト。

#![cfg(unix)]

use ai::adapters::outbound::approval_prompt_stderr_lines;
use ai::domain::{
    canonical_shell_exec_invocation, classify_shell_exec_tier,
    match_shell_exec_auto_approve_pattern, parse_shell_exec_auto_approve_patterns, ShellExecTier,
};
use aibe_client::ShellExecApprovalPrompt;
use aibe_protocol::ShellExecApprovalOrigin;

#[test]
fn approval_prompt_lists_choices_and_tier() {
    let prompt = ShellExecApprovalPrompt {
        prompt_id: "p1".into(),
        turn_id: "t1".into(),
        tool_call_id: "c1".into(),
        command: "echo".into(),
        args: vec!["hello".into(), "world".into()],
    };
    let lines = approval_prompt_stderr_lines(&prompt, ShellExecTier::Mutating, true);
    let joined = lines.join("\n");
    assert!(joined.contains("choices: [y]es / [n]o / [a]lways-same-args / [c]ommand-only"));
    assert!(joined.contains("hint: [a] remembers this exact command+args"));
    assert!(joined.contains("tier: mutating"));
    assert!(joined.contains("session_shell_allowed: true"));
}

#[test]
fn tier_classifier_is_conservative() {
    assert_eq!(
        classify_shell_exec_tier("git", &["status".into()]),
        ShellExecTier::ReadOnly
    );
    assert_eq!(
        classify_shell_exec_tier("git", &["branch".into(), "-D".into(), "old".into()]),
        ShellExecTier::Destructive
    );
    assert_eq!(
        classify_shell_exec_tier("git", &["clean".into(), "-fd".into()]),
        ShellExecTier::Destructive
    );
    assert_eq!(
        classify_shell_exec_tier("git", &["checkout".into(), "-f".into(), "main".into()]),
        ShellExecTier::Destructive
    );
    assert_eq!(
        classify_shell_exec_tier("git", &["branch".into(), "-M".into(), "main".into()]),
        ShellExecTier::Destructive
    );
    assert_eq!(
        classify_shell_exec_tier("git", &["tag".into(), "-f".into(), "v1".into()]),
        ShellExecTier::Destructive
    );
    assert_eq!(
        classify_shell_exec_tier("cargo", &["test".into()]),
        ShellExecTier::Mutating
    );
    assert_eq!(
        classify_shell_exec_tier("rm", &["-rf".into(), "/tmp/x".into()]),
        ShellExecTier::Destructive
    );
}

#[test]
fn pattern_matcher_uses_canonical_invocation() {
    let patterns = parse_shell_exec_auto_approve_patterns(
        vec![r"^git status$".into()],
        vec![r"^cargo test --quiet$".into()],
    )
    .expect("patterns");

    let read_only_invocation = canonical_shell_exec_invocation("git", &["status".into()]);
    let mutating_invocation =
        canonical_shell_exec_invocation("cargo", &["test".into(), "--quiet".into()]);

    let read_only_match = match_shell_exec_auto_approve_pattern(
        &read_only_invocation,
        ShellExecTier::ReadOnly,
        &patterns,
    );
    assert!(matches!(
        read_only_match,
        Some((_, ShellExecApprovalOrigin::PatternReadOnly))
    ));

    let mutating_match = match_shell_exec_auto_approve_pattern(
        &mutating_invocation,
        ShellExecTier::Mutating,
        &patterns,
    );
    assert!(matches!(
        mutating_match,
        Some((_, ShellExecApprovalOrigin::PatternMutating))
    ));
}
