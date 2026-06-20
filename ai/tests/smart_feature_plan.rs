#![cfg(unix)]
//! Smart Feature Plan（0041 / 0042）の unit テスト。

use ai::application::execute_feature_actions_mvp;
use ai::clap_cli::TurnOptions;
use ai::ports::outbound::{AgentError, MemoryClient};
use aibe_protocol::{
    ClientResponse, FeatureAction, MemoryContext, MemoryQueryDto, MemoryQueryStatus,
    MemoryRecipeStatus, SHELL_LOG_TAIL_MAX_BYTES,
};

struct MockMemoryClient;

impl MemoryClient for MockMemoryClient {
    fn memory_apply(
        &self,
        _session_id: &str,
        _context: &MemoryContext,
        _operation: aibe_protocol::MemoryOperationDto,
    ) -> Result<ClientResponse, AgentError> {
        Err(AgentError::Request("unexpected".into()))
    }

    fn memory_query(
        &self,
        _session_id: &str,
        _context: &MemoryContext,
        _query: MemoryQueryDto,
    ) -> Result<ClientResponse, AgentError> {
        Ok(ClientResponse::MemoryQueryResult {
            id: "q1".into(),
            status: MemoryQueryStatus::Ok,
            entries: vec![aibe_protocol::MemoryEntryDto {
                id: "e1".into(),
                memory_space_id: "space".into(),
                created_session_id: "sess".into(),
                last_session_id: "sess".into(),
                kind: "goal".into(),
                scope: aibe_protocol::MemoryScopeDto::Session,
                inject: aibe_protocol::MemoryInjectPolicyDto::Pinned,
                status: aibe_protocol::MemoryStatusDto::Active,
                text: "secret goal text".into(),
                project_key: None,
                created_at_ms: 0,
                updated_at_ms: 0,
                version: 1,
            }],
            prompt_block: Some("full prompt block".into()),
        })
    }

    fn memory_kind_list(
        &self,
        _session_id: &str,
        _context: &MemoryContext,
    ) -> Result<ClientResponse, AgentError> {
        Err(AgentError::Request("unexpected".into()))
    }

    fn memory_recipe_run(
        &self,
        _session_id: &str,
        _context: &MemoryContext,
        _recipe: &str,
        _apply: bool,
        _user_instruction: Option<String>,
    ) -> Result<ClientResponse, AgentError> {
        Ok(ClientResponse::MemoryRecipeRunResult {
            id: "r1".into(),
            status: MemoryRecipeStatus::Proposed,
            summary: "summary".into(),
            proposals: vec![],
            applied_entries: vec![],
        })
    }
}

fn default_turn() -> TurnOptions {
    TurnOptions {
        quiet: true,
        format: None,
        dry_run: false,
        preset: None,
        log_tail: None,
        log: None,
        no_log: false,
        session: None,
        socket: None,
        no_start: true,
        tools: None,
        profile: None,
        new: false,
        verbose_tools: false,
        progress: false,
        no_progress: false,
        timeout: None,
        yes_exec: false,
        silent_exec: false,
        console_hint: false,
        no_console_hint: false,
        trace_route: false,
    }
}

#[test]
fn set_recommended_tools_excludes_shell_exec() {
    let actions = vec![FeatureAction::SetRecommendedTools {
        tools: vec!["read_file".into(), "shell_exec".into(), "grep".into()],
    }];
    let out = execute_feature_actions_mvp(
        &actions,
        "user",
        None,
        "sess",
        default_turn(),
        &MockMemoryClient,
        true,
    );
    assert_eq!(out.turn.tools.as_deref(), Some("read_file,grep"));
}

#[test]
fn set_log_tail_bytes_clamps_to_max() {
    let actions = vec![FeatureAction::SetLogTailBytes {
        bytes: (SHELL_LOG_TAIL_MAX_BYTES as u64) + 999_999,
    }];
    let out = execute_feature_actions_mvp(
        &actions,
        "user",
        None,
        "sess",
        default_turn(),
        &MockMemoryClient,
        true,
    );
    assert_eq!(out.turn.log_tail, Some(SHELL_LOG_TAIL_MAX_BYTES));
}

#[test]
fn memory_query_populates_agent_messages_and_history_summaries() {
    let actions = vec![FeatureAction::MemoryQuery {
        query: MemoryQueryDto::default(),
    }];
    let ctx = MemoryContext {
        cwd: Some("/tmp".into()),
        memory_space_id: Some("space".into()),
    };
    let out = execute_feature_actions_mvp(
        &actions,
        "user",
        Some(ctx),
        "sess",
        default_turn(),
        &MockMemoryClient,
        true,
    );
    assert_eq!(out.extra_messages.len(), 1);
    assert!(out.extra_messages[0].content.contains("full prompt block"));
    assert_eq!(out.history_summaries.len(), 1);
    assert!(out.history_summaries[0]
        .content
        .contains("[smart feature: memory_query applied entries=1]"));
    assert!(!out.history_summaries[0].content.contains("secret goal"));
}

#[test]
fn feature_actions_pipeline_produces_distinct_agent_and_history_content() {
    let actions = vec![
        FeatureAction::MemoryQuery {
            query: MemoryQueryDto::default(),
        },
        FeatureAction::SetRecommendedTools {
            tools: vec!["read_file".into(), "shell_exec".into()],
        },
    ];
    let ctx = MemoryContext {
        cwd: Some("/tmp".into()),
        memory_space_id: Some("space".into()),
    };
    let out = execute_feature_actions_mvp(
        &actions,
        "inspect repo",
        Some(ctx),
        "sess",
        default_turn(),
        &MockMemoryClient,
        true,
    );
    assert!(out
        .extra_messages
        .iter()
        .any(|m| m.content.contains("full prompt")));
    assert!(out
        .history_summaries
        .iter()
        .all(|m| !m.content.contains("full prompt")));
    assert_eq!(out.turn.tools.as_deref(), Some("read_file"));
}

#[test]
fn history_summaries_stay_redacted_while_extra_messages_keep_full_content() {
    let actions = vec![FeatureAction::MemoryQuery {
        query: MemoryQueryDto::default(),
    }];
    let ctx = MemoryContext {
        cwd: Some("/tmp".into()),
        memory_space_id: Some("space".into()),
    };
    let out = execute_feature_actions_mvp(
        &actions,
        "user question",
        Some(ctx),
        "sess",
        default_turn(),
        &MockMemoryClient,
        true,
    );
    assert!(out.extra_messages[0].content.contains("full prompt block"));
    assert!(out.history_summaries[0].content.contains("smart feature"));
    assert!(!out.history_summaries[0]
        .content
        .contains("full prompt block"));
}
