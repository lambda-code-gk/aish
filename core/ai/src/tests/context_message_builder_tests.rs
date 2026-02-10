//! ContextMessageBuilder のテスト（QueryPlacement で query の有無を検証）

use crate::adapter::{PassThroughReducer, StdContextMessageBuilder};
use crate::domain::{ContextBudget, Query};
use crate::ports::outbound::{ContextMessageBuilder, QueryPlacement};
use common::llm::provider::Message as LlmMessage;
use common::msg::Msg;
use std::sync::Arc;

fn make_builder() -> StdContextMessageBuilder {
    StdContextMessageBuilder::new(
        Arc::new(PassThroughReducer),
        ContextBudget::legacy(),
    )
}

#[test]
fn test_query_placement_append_at_end_adds_query() {
    let builder = make_builder();
    let history = vec![LlmMessage::user("prior")];
    let query = Query::new("new query");
    let msgs = builder.build(
        &history,
        Some(&query),
        Some("sys"),
        QueryPlacement::AppendAtEnd,
    );
    assert!(msgs.len() >= 2);
    assert!(matches!(&msgs[0], Msg::System(_)));
    let last = msgs.last().unwrap();
    assert!(matches!(last, Msg::User(s) if s == "new query"));
}

#[test]
fn test_query_placement_already_in_history_does_not_duplicate_query() {
    let builder = make_builder();
    let history = vec![
        LlmMessage::user("prior"),
        LlmMessage::user("already in history"),
    ];
    let query = Query::new("already in history");
    let msgs = builder.build(
        &history,
        Some(&query),
        Some("sys"),
        QueryPlacement::AlreadyInHistory,
    );
    // query は末尾に追加されないので、User は2つだけ（prior, already in history）
    let user_count = msgs.iter().filter(|m| matches!(m, Msg::User(_))).count();
    assert_eq!(user_count, 2);
    let last = msgs.last().unwrap();
    assert!(matches!(last, Msg::User(s) if s == "already in history"));
}

#[test]
fn test_resume_no_query() {
    let builder = make_builder();
    let history = vec![LlmMessage::user("only one")];
    let msgs = builder.build(
        &history,
        None,
        Some("sys"),
        QueryPlacement::AlreadyInHistory,
    );
    assert_eq!(msgs.len(), 2); // System + User
    assert!(matches!(&msgs[1], Msg::User(s) if s == "only one"));
}
