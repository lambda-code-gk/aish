use std::collections::HashSet;

use aibe_protocol::ShellExecApprovalOrigin;
use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellExecTier {
    ReadOnly,
    Mutating,
    Destructive,
}

impl ShellExecTier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::Mutating => "mutating",
            Self::Destructive => "destructive",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellExecRememberScope {
    ExactInvocation,
    CommandName,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellExecApprovalDecision {
    pub approved: bool,
    pub approval_origin: ShellExecApprovalOrigin,
    pub remember_scope: Option<ShellExecRememberScope>,
}

#[derive(Debug, Clone, Default)]
pub struct ShellExecSessionState {
    pub session_shell_allowed: bool,
    exact_invocations: HashSet<String>,
    command_names: HashSet<String>,
}

impl ShellExecSessionState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn session_shell_allowed(&self) -> bool {
        self.session_shell_allowed
    }

    pub fn allow_session_shell(&mut self) {
        self.session_shell_allowed = true;
    }

    pub fn remember_exact(&mut self, key: String) {
        self.exact_invocations.insert(key);
    }

    pub fn remember_command(&mut self, key: String) {
        self.command_names.insert(key);
    }

    pub fn has_exact(&self, key: &str) -> bool {
        self.exact_invocations.contains(key)
    }

    pub fn has_command(&self, key: &str) -> bool {
        self.command_names.contains(key)
    }
}

#[derive(Debug, Clone)]
pub struct ShellExecAutoApprovePatternSet {
    pub read_only: Vec<Regex>,
    pub mutating: Vec<Regex>,
}

impl ShellExecAutoApprovePatternSet {
    pub fn empty() -> Self {
        Self {
            read_only: Vec::new(),
            mutating: Vec::new(),
        }
    }
}

pub fn classify_shell_exec_tier(command: &str, args: &[String]) -> ShellExecTier {
    let command = command.trim();
    if command.is_empty() {
        return ShellExecTier::Mutating;
    }

    if matches!(
        command,
        "ls" | "pwd" | "cat" | "head" | "tail" | "find" | "grep"
    ) {
        return ShellExecTier::ReadOnly;
    }
    if command == "git" {
        return classify_git_tier(args);
    }
    if command == "cargo" {
        return classify_cargo_tier(args);
    }
    if matches!(
        command,
        "rm" | "rmdir" | "dd" | "mkfs" | "shutdown" | "reboot" | "poweroff" | "halt"
    ) {
        return ShellExecTier::Destructive;
    }
    if command == "sed" && args.iter().any(|arg| arg == "-i" || arg.starts_with("-i")) {
        return ShellExecTier::Mutating;
    }
    if matches!(
        command,
        "mv" | "cp" | "touch" | "mkdir" | "chmod" | "chown" | "truncate" | "tee"
    ) {
        return ShellExecTier::Mutating;
    }
    ShellExecTier::Mutating
}

pub fn canonical_shell_exec_invocation(command: &str, args: &[String]) -> String {
    let mut parts = vec![shell_quote(command)];
    parts.extend(args.iter().map(|arg| shell_quote(arg)));
    parts.join(" ")
}

pub fn exact_shell_exec_key(command: &str, args: &[String]) -> String {
    serde_json::to_string(&(command, args)).unwrap_or_else(|_| format!("{command}\u{0}{args:?}"))
}

pub fn command_shell_exec_key(command: &str, tier: ShellExecTier) -> String {
    format!("{command}\t{}", tier.as_str())
}

pub fn parse_shell_exec_auto_approve_patterns(
    read_only: Vec<String>,
    mutating: Vec<String>,
) -> Option<ShellExecAutoApprovePatternSet> {
    let compile = |values: Vec<String>| -> Option<Vec<Regex>> {
        let mut compiled = Vec::new();
        for value in values {
            let regex = Regex::new(&value).ok()?;
            compiled.push(regex);
        }
        Some(compiled)
    };
    Some(ShellExecAutoApprovePatternSet {
        read_only: compile(read_only)?,
        mutating: compile(mutating)?,
    })
}

pub fn match_shell_exec_auto_approve_pattern<'a>(
    invocation: &str,
    tier: ShellExecTier,
    patterns: &'a ShellExecAutoApprovePatternSet,
) -> Option<(&'a str, ShellExecApprovalOrigin)> {
    let haystack = invocation;
    match tier {
        ShellExecTier::ReadOnly => patterns.read_only.iter().find_map(|re| {
            if re.is_match(haystack) {
                Some((re.as_str(), ShellExecApprovalOrigin::PatternReadOnly))
            } else {
                None
            }
        }),
        ShellExecTier::Mutating => patterns.mutating.iter().find_map(|re| {
            if re.is_match(haystack) {
                Some((re.as_str(), ShellExecApprovalOrigin::PatternMutating))
            } else {
                None
            }
        }),
        ShellExecTier::Destructive => None,
    }
}

pub fn shell_exec_approval_origin_for_choice(
    choice: ShellExecApprovalChoice,
) -> ShellExecApprovalOrigin {
    match choice {
        ShellExecApprovalChoice::Yes => ShellExecApprovalOrigin::UiYes,
        ShellExecApprovalChoice::No => ShellExecApprovalOrigin::UiNo,
        ShellExecApprovalChoice::AlwaysThisSession => {
            ShellExecApprovalOrigin::UiAlwaysThisSessionExactInvocation
        }
        ShellExecApprovalChoice::CommandOnly => ShellExecApprovalOrigin::UiCommandOnly,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellExecApprovalChoice {
    Yes,
    No,
    AlwaysThisSession,
    CommandOnly,
}

fn classify_git_tier(args: &[String]) -> ShellExecTier {
    if git_invocation_is_destructive(args) {
        return ShellExecTier::Destructive;
    }
    match args.first().map(String::as_str) {
        Some("status" | "diff" | "log" | "show" | "rev-parse" | "branch") => {
            ShellExecTier::ReadOnly
        }
        Some("add" | "commit" | "merge" | "rebase" | "cherry-pick" | "stash" | "checkout") => {
            ShellExecTier::Mutating
        }
        _ => ShellExecTier::Mutating,
    }
}

/// `git` の argv から破壊的操作を検出する。曖昧なら true（上位 tier）に倒す。
fn git_invocation_is_destructive(args: &[String]) -> bool {
    let sub = args.first().map(String::as_str);
    let rest = args.get(1..).unwrap_or(&[]);

    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "--hard" | "--force" | "--force-with-lease"))
    {
        return true;
    }

    match sub {
        Some("branch") => rest
            .iter()
            .any(|arg| matches!(arg.as_str(), "-D" | "-d" | "--delete" | "-M" | "-m")),
        Some("clean") => args.iter().any(|arg| {
            arg == "-f"
                || arg == "-x"
                || arg == "-fd"
                || arg == "-df"
                || arg == "-ffd"
                || (arg.starts_with('-') && arg.contains('f'))
        }),
        Some("checkout") => rest
            .iter()
            .any(|arg| matches!(arg.as_str(), "-f" | "--force" | "-B")),
        Some("reset") => args.iter().any(|arg| arg == "--hard"),
        Some("push") => args
            .iter()
            .any(|arg| arg.contains("force") || arg == "-f" || arg.starts_with("-f")),
        Some("tag") => rest
            .iter()
            .any(|arg| matches!(arg.as_str(), "-f" | "--force" | "-F")),
        Some("stash") => matches!(rest.first().map(String::as_str), Some("drop" | "clear")),
        Some("rebase") => rest.iter().any(|arg| arg.contains("force")),
        Some("worktree") => matches!(rest.first().map(String::as_str), Some("remove")),
        _ => false,
    }
}

fn classify_cargo_tier(args: &[String]) -> ShellExecTier {
    match args.first().map(String::as_str) {
        Some("metadata") | Some("tree") => ShellExecTier::ReadOnly,
        Some("check" | "test" | "build" | "clippy" | "fmt" | "run") => ShellExecTier::Mutating,
        _ => ShellExecTier::Mutating,
    }
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '=' | '+'))
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_destructive_flags_are_conservative() {
        assert_eq!(
            classify_shell_exec_tier("git", &["branch".into(), "-D".into(), "main".into()]),
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
            classify_shell_exec_tier("git", &["push".into(), "--force".into()]),
            ShellExecTier::Destructive
        );
        assert_eq!(
            classify_shell_exec_tier("git", &["branch".into(), "-M".into(), "main".into()]),
            ShellExecTier::Destructive
        );
        assert_eq!(
            classify_shell_exec_tier("git", &["checkout".into(), "-B".into(), "main".into()]),
            ShellExecTier::Destructive
        );
        assert_eq!(
            classify_shell_exec_tier("git", &["tag".into(), "-f".into(), "v1".into()]),
            ShellExecTier::Destructive
        );
    }

    #[test]
    fn git_read_only_subcommands_stay_read_only() {
        assert_eq!(
            classify_shell_exec_tier("git", &["status".into()]),
            ShellExecTier::ReadOnly
        );
        assert_eq!(
            classify_shell_exec_tier("git", &["branch".into()]),
            ShellExecTier::ReadOnly
        );
    }

    #[test]
    fn git_mutating_without_destructive_flags() {
        assert_eq!(
            classify_shell_exec_tier("git", &["checkout".into(), "main".into()]),
            ShellExecTier::Mutating
        );
        assert_eq!(
            classify_shell_exec_tier("git", &["add".into(), ".".into()]),
            ShellExecTier::Mutating
        );
    }

    #[test]
    fn session_exact_remembers_only_identical_invocation() {
        let mut session = ShellExecSessionState::new();
        session.allow_session_shell();
        session.remember_exact(exact_shell_exec_key(
            "git",
            &["grep".into(), "-n".into(), "second_ask".into()],
        ));

        assert!(session.has_exact(&exact_shell_exec_key(
            "git",
            &["grep".into(), "-n".into(), "second_ask".into()],
        )));
        assert!(!session.has_exact(&exact_shell_exec_key(
            "git",
            &["grep".into(), "-n".into(), "mod tests".into()],
        )));
    }

    #[test]
    fn session_command_remembers_git_mutating_across_args() {
        let mut session = ShellExecSessionState::new();
        session.allow_session_shell();
        session.remember_command(command_shell_exec_key("git", ShellExecTier::Mutating));

        assert!(session.has_command(&command_shell_exec_key("git", ShellExecTier::Mutating)));
        assert!(!session.has_command(&command_shell_exec_key(
            "git",
            ShellExecTier::Destructive,
        )));
    }
}
