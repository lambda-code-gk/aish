//! Spec 0068: production CLI opt-in for Task Completion.

use ai::clap_cli::{AiCli, AiCommand};
use clap::Parser;

#[test]
fn ai_ask_parses_task_completion_flag() {
    let cli = AiCli::try_parse_from(["ai", "ask", "--task-completion", "hello"]).expect("parse");
    let AiCommand::Ask { turn, .. } = cli.command else {
        panic!("expected ask");
    };
    assert!(turn.task_completion);
}

#[test]
fn ai_ask_defaults_task_completion_to_false() {
    let cli = AiCli::try_parse_from(["ai", "ask", "hello"]).expect("parse");
    let AiCommand::Ask { turn, .. } = cli.command else {
        panic!("expected ask");
    };
    assert!(!turn.task_completion);
}
