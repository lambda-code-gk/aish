//! Smart Feature Plan（0041）用の機能実行器（MVP）。
//!
//! MVP では以下だけを実行対象とする。
//! - `MemoryQuery`
//! - `MemoryRecipeRun { apply: false }`
//! - `SetLogTailBytes`
//! - `SetRecommendedTools`（safe tool のみ）

use aibe_protocol::{
    ClientResponse, FeatureAction, MemoryContext, MemoryQueryDto, MemoryRecipeProposalDto,
    ProtocolMessage, GIT_DIFF, GIT_STATUS, GREP, LIST_DIR, READ_FILE, SHELL_EXEC,
    SYSTEM_INSTRUCTION_MAX_BYTES,
};

use crate::clap_cli::TurnOptions;
use crate::ports::outbound::MemoryClient;

/// feature action の適用結果。
#[derive(Debug, Clone)]
pub struct FeatureExecutionOutcome {
    pub turn: TurnOptions,
    pub extra_messages: Vec<ProtocolMessage>,
}

/// MVP: `RoutePlan.feature_actions` を安全に解釈して適用する。
///
/// - 読み取り系は自動適用（ただし失敗は致命傷にしない）
/// - `SetLogTailBytes` / `SetRecommendedTools` は CLI 明示値を上書きしない
/// - `MemoryRecipeRun { apply: true }` は無視する（副作用回避）
pub fn execute_feature_actions_mvp(
    actions: &[FeatureAction],
    user_input: &str,
    memory_context: Option<MemoryContext>,
    ai_session_id: &str,
    mut turn: TurnOptions,
    memory_client: &dyn MemoryClient,
    quiet: bool,
) -> FeatureExecutionOutcome {
    let mut extra_messages = Vec::new();

    let has_memory_actions = actions.iter().any(|a| {
        matches!(
            a,
            FeatureAction::MemoryQuery { .. } | FeatureAction::MemoryRecipeRun { .. }
        )
    });
    let memory_context: Option<MemoryContext> = if has_memory_actions {
        memory_context
    } else {
        None
    };

    for action in actions {
        match action {
            FeatureAction::MemoryQuery { query } => {
                let Some(ctx) = memory_context.as_ref() else {
                    continue;
                };
                let query = apply_query_defaults(query.clone());
                match memory_client.memory_query(ai_session_id, ctx, query.clone()) {
                    Ok(ClientResponse::MemoryQueryResult {
                        prompt_block,
                        entries,
                        ..
                    }) => {
                        let content = prompt_block
                            .or_else(|| {
                                (!entries.is_empty()).then(|| {
                                    entries
                                        .iter()
                                        .map(|e| e.text.as_str())
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                })
                            })
                            .unwrap_or_else(|| "(memory query: empty)".to_string());
                        let content =
                            truncate_system_message(content, SYSTEM_INSTRUCTION_MAX_BYTES);
                        extra_messages.push(ProtocolMessage {
                            role: "system".to_string(),
                            content: format!("[contextual memory query]\n{}", content.trim()),
                        });
                    }
                    Ok(other) => {
                        if !quiet {
                            eprintln!("ai: smart feature plan: memory_query unexpected response: {other:?}");
                        }
                    }
                    Err(e) => {
                        if !quiet {
                            eprintln!("ai: smart feature plan: memory_query failed: {e}");
                        }
                    }
                }
            }
            FeatureAction::MemoryRecipeRun { recipe_id, apply } => {
                if *apply {
                    // MVP では副作用禁止のため無視。
                    continue;
                }
                let Some(ctx) = memory_context.as_ref() else {
                    continue;
                };
                match memory_client.memory_recipe_run(
                    ai_session_id,
                    ctx,
                    recipe_id,
                    false,
                    Some(user_input.to_string()),
                ) {
                    Ok(ClientResponse::MemoryRecipeRunResult {
                        summary, proposals, ..
                    }) => {
                        let proposals = format_memory_recipe_proposals(proposals);
                        let summary =
                            truncate_system_message(summary, SYSTEM_INSTRUCTION_MAX_BYTES);
                        extra_messages.push(ProtocolMessage {
                            role: "system".to_string(),
                            content: format!(
                                "[memory recipe proposal: {recipe_id}]\nsummary: {summary}\nproposals:\n{proposals}"
                            ),
                        });
                    }
                    Ok(other) => {
                        if !quiet {
                            eprintln!(
                                "ai: smart feature plan: memory_recipe_run unexpected response: {other:?}"
                            );
                        }
                    }
                    Err(e) => {
                        if !quiet {
                            eprintln!("ai: smart feature plan: memory_recipe_run failed: {e}");
                        }
                    }
                }
            }
            FeatureAction::SetLogTailBytes { bytes } => {
                if turn.log_tail.is_none() {
                    turn.log_tail = Some(*bytes as usize);
                }
            }
            FeatureAction::SetRecommendedTools { tools } => {
                if turn.tools.is_some() {
                    continue;
                }
                let safe_tools = resolve_safe_tools(tools);
                turn.tools = Some(if safe_tools.is_empty() {
                    "none".to_string()
                } else {
                    safe_tools.join(",")
                });
            }
            FeatureAction::Unsupported => {
                // no-op
            }
        }
    }

    FeatureExecutionOutcome {
        turn,
        extra_messages,
    }
}

fn apply_query_defaults(mut query: MemoryQueryDto) -> MemoryQueryDto {
    query.include_prompt_block = true;
    query
}

fn truncate_system_message(raw: String, max_bytes: usize) -> String {
    let trimmed = raw.trim().to_string();
    if trimmed.len() <= max_bytes {
        return trimmed;
    }
    let end = trimmed.floor_char_boundary(max_bytes);
    trimmed[..end].to_string()
}

fn resolve_safe_tools(raw: &[String]) -> Vec<String> {
    const SAFE: &[&str] = &[READ_FILE, LIST_DIR, GREP, GIT_DIFF, GIT_STATUS];

    let mut out: Vec<String> = Vec::new();
    for t in raw {
        let norm = t.trim().to_ascii_lowercase().replace('-', "_");
        let mapped = match norm.as_str() {
            // common aliases
            "view_file" | "viewfile" | "read" | "cat" | "cat_file" => READ_FILE,
            "list_files" | "listdir" | "ls" | "dir" => LIST_DIR,
            "search" | "find" | "rg" => GREP,
            "diff" => GIT_DIFF,
            "git" | "status" | "git_status" => GIT_STATUS,
            other => other,
        };

        // unknown tool は safe にしない
        if mapped == SHELL_EXEC {
            continue;
        }
        if !SAFE.contains(&mapped) {
            // keep the behavior strict for MVP
            continue;
        }
        let s = mapped.to_string();
        if !out.iter().any(|x| x == &s) {
            out.push(s);
        }
    }
    out
}

fn format_memory_recipe_proposals(proposals: Vec<MemoryRecipeProposalDto>) -> String {
    if proposals.is_empty() {
        return "(none)".to_string();
    }
    let mut out = String::new();
    for p in &proposals {
        let op = format_memory_operation_line(&p.operation);
        out.push_str(&format!("- {op} — {}\n", p.rationale));
    }
    out.trim_end().to_string()
}

fn format_memory_operation_line(operation: &aibe_protocol::MemoryOperationDto) -> String {
    use aibe_protocol::MemoryOperationDto;
    match operation {
        MemoryOperationDto::Add(add) => format!("add {}: {}", add.kind, add.text),
        MemoryOperationDto::ClearKind(c) => format!("clear_kind {}", c.kind),
        MemoryOperationDto::Archive(a) => format!("archive {}", a.id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockMemoryClient;

    impl MemoryClient for MockMemoryClient {
        fn memory_apply(
            &self,
            _session_id: &str,
            _context: &MemoryContext,
            _operation: aibe_protocol::MemoryOperationDto,
        ) -> Result<ClientResponse, crate::ports::outbound::AgentError> {
            Err(crate::ports::outbound::AgentError::Request(
                "memory_apply not expected".to_string(),
            ))
        }

        fn memory_query(
            &self,
            _session_id: &str,
            _context: &MemoryContext,
            _query: MemoryQueryDto,
        ) -> Result<ClientResponse, crate::ports::outbound::AgentError> {
            Ok(ClientResponse::MemoryQueryResult {
                id: "q1".into(),
                status: aibe_protocol::MemoryQueryStatus::Ok,
                entries: vec![],
                prompt_block: Some("prompt block".into()),
            })
        }

        fn memory_kind_list(
            &self,
            _session_id: &str,
            _context: &MemoryContext,
        ) -> Result<ClientResponse, crate::ports::outbound::AgentError> {
            Err(crate::ports::outbound::AgentError::Request(
                "memory_kind_list not expected".to_string(),
            ))
        }

        fn memory_recipe_run(
            &self,
            _session_id: &str,
            _context: &MemoryContext,
            _recipe: &str,
            _apply: bool,
            _user_instruction: Option<String>,
        ) -> Result<ClientResponse, crate::ports::outbound::AgentError> {
            Ok(ClientResponse::MemoryRecipeRunResult {
                id: "r1".into(),
                status: aibe_protocol::MemoryRecipeStatus::Proposed,
                summary: "summary".into(),
                proposals: vec![aibe_protocol::MemoryRecipeProposalDto {
                    operation: aibe_protocol::MemoryOperationDto::Add(
                        aibe_protocol::MemoryOperationAdd {
                            kind: "goal".into(),
                            scope: None,
                            inject: None,
                            status: None,
                            text: "do it".into(),
                            make_active: None,
                        },
                    ),
                    rationale: "because".into(),
                }],
                applied_entries: vec![],
            })
        }
    }

    #[test]
    fn set_recommended_tools_filters_unsafe_and_unknown() {
        let actions = vec![FeatureAction::SetRecommendedTools {
            tools: vec![
                "read_file".into(),
                "shell_exec".into(),
                "unknown_tool".into(),
                "grep".into(),
            ],
        }];
        let client = MockMemoryClient;
        let turn = TurnOptions {
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
        };

        // memory actions are not present; memory_context not needed.
        let out = execute_feature_actions_mvp(
            &actions,
            "user",
            Some(MemoryContext {
                cwd: Some("/tmp".into()),
                memory_space_id: Some("ctx_test".into()),
            }),
            "sess_001",
            turn,
            &client,
            true,
        );
        assert_eq!(out.turn.tools.as_deref(), Some("read_file,grep"));
    }

    #[test]
    fn memory_actions_are_safe_and_best_effort() {
        let actions = vec![
            FeatureAction::MemoryQuery {
                query: MemoryQueryDto::default(),
            },
            FeatureAction::MemoryRecipeRun {
                recipe_id: "clarify-goal".into(),
                apply: false,
            },
        ];
        let client = MockMemoryClient;
        let turn = TurnOptions {
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
        };

        let out = execute_feature_actions_mvp(
            &actions,
            "user instruction",
            Some(MemoryContext {
                cwd: Some("/tmp".into()),
                memory_space_id: Some("ctx_test".into()),
            }),
            "sess_001",
            turn,
            &client,
            true,
        );

        assert_eq!(out.extra_messages.len(), 2);
        assert!(out
            .extra_messages
            .iter()
            .any(|m| m.content.contains("[contextual memory query]")));
        assert!(out
            .extra_messages
            .iter()
            .any(|m| m.content.contains("[memory recipe proposal: clarify-goal]")));
        assert!(out.turn.preset.is_none());
        assert!(out.turn.tools.is_none());
        assert!(out.turn.log_tail.is_none());
    }
}
