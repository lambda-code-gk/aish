//! contextual memory CLI ハンドラ。

use std::path::PathBuf;

use aibe_protocol::{
    ClientResponse, MemoryEntryDto, MemoryInjectPolicyDto, MemoryKindDefinitionDto,
    MemoryOperationAdd, MemoryOperationClearKind, MemoryOperationDto, MemoryQueryDto,
    MemoryRecipeProposalDto, MemoryScopeDto, MemoryStatusDto,
};

use crate::domain::{append_env_line, OutputFormat};
use crate::ports::outbound::{AgentError, MemoryClient};

pub struct MemoryCliContext {
    pub socket_path: PathBuf,
    pub session_id: String,
    pub memory_context: aibe_protocol::MemoryContext,
    pub cwd: PathBuf,
    pub format: OutputFormat,
}

pub fn run_goal_set(
    client: &dyn MemoryClient,
    ctx: &MemoryCliContext,
    text: &str,
) -> Result<String, AgentError> {
    let op = MemoryOperationDto::Add(MemoryOperationAdd {
        kind: "goal".into(),
        scope: Some(MemoryScopeDto::Project),
        inject: Some(MemoryInjectPolicyDto::Pinned),
        status: Some(MemoryStatusDto::Active),
        text: text.to_string(),
        make_active: Some(true),
    });
    apply_and_summarize(client, ctx, op, &format!("goal set: {text}"))
}

pub fn run_goal_show(
    client: &dyn MemoryClient,
    ctx: &MemoryCliContext,
) -> Result<String, AgentError> {
    query_active(client, ctx, "goal", MemoryScopeDto::Project)
}

pub fn run_goal_clear(
    client: &dyn MemoryClient,
    ctx: &MemoryCliContext,
) -> Result<String, AgentError> {
    clear_active(client, ctx, "goal", MemoryScopeDto::Project, "goal cleared")
}

pub fn run_now_set(
    client: &dyn MemoryClient,
    ctx: &MemoryCliContext,
    text: &str,
) -> Result<String, AgentError> {
    let op = MemoryOperationDto::Add(MemoryOperationAdd {
        kind: "now".into(),
        scope: Some(MemoryScopeDto::Session),
        inject: Some(MemoryInjectPolicyDto::Pinned),
        status: Some(MemoryStatusDto::Active),
        text: text.to_string(),
        make_active: Some(true),
    });
    apply_and_summarize(client, ctx, op, &format!("now set: {text}"))
}

pub fn run_now_show(
    client: &dyn MemoryClient,
    ctx: &MemoryCliContext,
) -> Result<String, AgentError> {
    query_active(client, ctx, "now", MemoryScopeDto::Session)
}

pub fn run_now_clear(
    client: &dyn MemoryClient,
    ctx: &MemoryCliContext,
) -> Result<String, AgentError> {
    clear_active(client, ctx, "now", MemoryScopeDto::Session, "now cleared")
}

pub fn run_idea_add(
    client: &dyn MemoryClient,
    ctx: &MemoryCliContext,
    text: &str,
) -> Result<String, AgentError> {
    let op = MemoryOperationDto::Add(MemoryOperationAdd {
        kind: "idea".into(),
        scope: Some(MemoryScopeDto::Project),
        inject: Some(MemoryInjectPolicyDto::OnDemand),
        status: Some(MemoryStatusDto::Open),
        text: text.to_string(),
        make_active: Some(false),
    });
    apply_and_summarize(client, ctx, op, &format!("idea added: {text}"))
}

pub fn run_idea_list(
    client: &dyn MemoryClient,
    ctx: &MemoryCliContext,
) -> Result<String, AgentError> {
    let query = MemoryQueryDto {
        kind: Some("idea".into()),
        scope: Some(MemoryScopeDto::Project),
        status: Some(MemoryStatusDto::Open),
        active_only: false,
        include_archived: false,
        limit: None,
        include_prompt_block: false,
        user_query: None,
    };
    let response = client.memory_query(&ctx.session_id, &ctx.memory_context, query)?;
    match response {
        ClientResponse::MemoryQueryResult { entries, .. } => {
            Ok(format_entries(&entries, ctx.format))
        }
        ClientResponse::Error { message, .. } => Err(AgentError::Request(message)),
        other => Err(AgentError::Request(format!(
            "unexpected response: {other:?}"
        ))),
    }
}

pub fn run_idea_clear(
    client: &dyn MemoryClient,
    ctx: &MemoryCliContext,
) -> Result<String, AgentError> {
    clear_active(client, ctx, "idea", MemoryScopeDto::Project, "idea cleared")
}

pub fn run_mem_add(
    client: &dyn MemoryClient,
    ctx: &MemoryCliContext,
    kind: &str,
    text: &str,
) -> Result<String, AgentError> {
    if let Some(message) = standard_kind_mem_add_hint(kind) {
        return Err(AgentError::Request(message));
    }
    let op = mem_add_operation(kind, text);
    apply_and_summarize(client, ctx, op, &format!("mem add {kind}: {text}"))
}

/// scope/inject/status/make_active は AIBE server が補完する（registered / unregistered とも）。
fn mem_add_operation(kind: &str, text: &str) -> MemoryOperationDto {
    MemoryOperationDto::Add(MemoryOperationAdd {
        kind: kind.to_string(),
        text: text.to_string(),
        scope: None,
        inject: None,
        status: None,
        make_active: None,
    })
}

pub fn run_mem_list(
    client: &dyn MemoryClient,
    ctx: &MemoryCliContext,
    kind: Option<&str>,
) -> Result<String, AgentError> {
    let query = MemoryQueryDto {
        kind: kind.map(str::to_string),
        scope: None,
        status: None,
        active_only: false,
        include_archived: false,
        limit: None,
        include_prompt_block: false,
        user_query: None,
    };
    let response = client.memory_query(&ctx.session_id, &ctx.memory_context, query)?;
    match response {
        ClientResponse::MemoryQueryResult { entries, .. } => {
            Ok(format_entries(&entries, ctx.format))
        }
        ClientResponse::Error { message, .. } => Err(AgentError::Request(message)),
        other => Err(AgentError::Request(format!(
            "unexpected response: {other:?}"
        ))),
    }
}

pub fn run_mem_show(
    client: &dyn MemoryClient,
    ctx: &MemoryCliContext,
    user_query: Option<&str>,
) -> Result<String, AgentError> {
    let query = MemoryQueryDto {
        kind: None,
        scope: None,
        status: None,
        active_only: false,
        include_archived: false,
        limit: None,
        include_prompt_block: true,
        user_query: user_query.map(str::to_string),
    };
    let response = client.memory_query(&ctx.session_id, &ctx.memory_context, query)?;
    match response {
        ClientResponse::MemoryQueryResult { prompt_block, .. } => format_prompt_block(
            prompt_block.as_deref(),
            ctx.memory_context.memory_space_id.as_deref(),
            ctx.format,
        ),
        ClientResponse::Error { message, .. } => Err(AgentError::Request(message)),
        other => Err(AgentError::Request(format!(
            "unexpected response: {other:?}"
        ))),
    }
}

pub fn run_mem_clear(
    client: &dyn MemoryClient,
    ctx: &MemoryCliContext,
    kind: &str,
) -> Result<String, AgentError> {
    clear_active(
        client,
        ctx,
        kind,
        clear_scope_for_kind(kind),
        &format!("mem clear {kind}"),
    )
}

pub fn run_mem_kinds(
    client: &dyn MemoryClient,
    ctx: &MemoryCliContext,
) -> Result<String, AgentError> {
    let response = client.memory_kind_list(&ctx.session_id, &ctx.memory_context)?;
    match response {
        ClientResponse::MemoryKindListResult { kinds, .. } => {
            Ok(format_kind_definitions(&kinds, ctx.format))
        }
        ClientResponse::Error { message, .. } => Err(AgentError::Request(message)),
        other => Err(AgentError::Request(format!(
            "unexpected response: {other:?}"
        ))),
    }
}

/// `ai mem run clarify-goal` — LLM 提案を表示し、任意で apply する。
pub fn run_mem_recipe_clarify_goal(
    client: &dyn MemoryClient,
    ctx: &MemoryCliContext,
    apply: bool,
    user_instruction: Option<&str>,
    confirm_apply: impl FnOnce() -> bool,
) -> Result<String, AgentError> {
    let response = client.memory_recipe_run(
        &ctx.session_id,
        &ctx.memory_context,
        "clarify-goal",
        false,
        user_instruction.map(str::to_string),
    )?;
    let (summary, proposals) = match response {
        ClientResponse::MemoryRecipeRunResult {
            summary, proposals, ..
        } => (summary, proposals),
        ClientResponse::Error { message, .. } => return Err(AgentError::Request(message)),
        other => {
            return Err(AgentError::Request(format!(
                "unexpected response: {other:?}"
            )))
        }
    };

    let mut out = format_recipe_proposals(&summary, &proposals, ctx.format);

    if apply {
        if !confirm_apply() {
            return Ok(out);
        }
        let mut applied = 0usize;
        for proposal in &proposals {
            match client.memory_apply(
                &ctx.session_id,
                &ctx.memory_context,
                proposal.operation.clone(),
            )? {
                ClientResponse::MemoryApplyResult { .. } => applied += 1,
                ClientResponse::Error { message, .. } => {
                    return Err(AgentError::Request(message));
                }
                other => {
                    return Err(AgentError::Request(format!(
                        "unexpected response: {other:?}"
                    )))
                }
            }
        }
        out.push_str(&format!("\napplied {applied} memory operation(s)\n"));
    }

    Ok(out)
}

fn format_recipe_proposals(
    summary: &str,
    proposals: &[MemoryRecipeProposalDto],
    format: OutputFormat,
) -> String {
    match format {
        OutputFormat::Json => {
            let value = serde_json::json!({
                "summary": summary,
                "proposals": proposals,
            });
            format!(
                "{}\n",
                serde_json::to_string_pretty(&value).unwrap_or_default()
            )
        }
        OutputFormat::Tsv | OutputFormat::Env => {
            let mut out = format!("summary: {summary}\n");
            if proposals.is_empty() {
                out.push_str("proposals: (none)\n");
            } else {
                out.push_str("proposals:\n");
                for (idx, p) in proposals.iter().enumerate() {
                    out.push_str(&format!(
                        "  {}. {} — {}\n",
                        idx + 1,
                        format_operation_line(&p.operation),
                        p.rationale
                    ));
                }
            }
            out
        }
    }
}

fn format_operation_line(operation: &MemoryOperationDto) -> String {
    match operation {
        MemoryOperationDto::Add(add) => format!("add {}: {}", add.kind, add.text),
        MemoryOperationDto::ClearKind(c) => format!("clear_kind {} ({:?})", c.kind, c.scope),
        MemoryOperationDto::Archive(a) => format!("archive {}", a.id),
    }
}

fn clear_scope_for_kind(kind: &str) -> MemoryScopeDto {
    match kind {
        "now" => MemoryScopeDto::Session,
        _ => MemoryScopeDto::Project,
    }
}

fn apply_and_summarize(
    client: &dyn MemoryClient,
    ctx: &MemoryCliContext,
    operation: MemoryOperationDto,
    ok_line: &str,
) -> Result<String, AgentError> {
    let response = client.memory_apply(&ctx.session_id, &ctx.memory_context, operation)?;
    match response {
        ClientResponse::MemoryApplyResult { .. } => Ok(ok_line.to_string()),
        ClientResponse::Error { message, .. } => Err(AgentError::Request(message)),
        other => Err(AgentError::Request(format!(
            "unexpected response: {other:?}"
        ))),
    }
}

fn query_active(
    client: &dyn MemoryClient,
    ctx: &MemoryCliContext,
    kind: &str,
    scope: MemoryScopeDto,
) -> Result<String, AgentError> {
    let status = if kind == "idea" {
        MemoryStatusDto::Open
    } else {
        MemoryStatusDto::Active
    };
    let query = MemoryQueryDto {
        kind: Some(kind.to_string()),
        scope: Some(scope),
        status: Some(status),
        active_only: true,
        include_archived: false,
        limit: Some(1),
        include_prompt_block: false,
        user_query: None,
    };
    let response = client.memory_query(&ctx.session_id, &ctx.memory_context, query)?;
    match response {
        ClientResponse::MemoryQueryResult { entries, .. } => {
            if entries.is_empty() {
                Ok(format!("{kind}: (none)"))
            } else {
                Ok(format_entries(&entries, ctx.format))
            }
        }
        ClientResponse::Error { message, .. } => Err(AgentError::Request(message)),
        other => Err(AgentError::Request(format!(
            "unexpected response: {other:?}"
        ))),
    }
}

fn clear_active(
    client: &dyn MemoryClient,
    ctx: &MemoryCliContext,
    kind: &str,
    scope: MemoryScopeDto,
    ok_line: &str,
) -> Result<String, AgentError> {
    let operation = MemoryOperationDto::ClearKind(MemoryOperationClearKind {
        kind: kind.to_string(),
        scope,
    });
    let response = client.memory_apply(&ctx.session_id, &ctx.memory_context, operation)?;
    match response {
        ClientResponse::MemoryApplyResult { .. } => Ok(ok_line.to_string()),
        ClientResponse::Error { message, .. } => Err(AgentError::Request(message)),
        other => Err(AgentError::Request(format!(
            "unexpected response: {other:?}"
        ))),
    }
}

fn standard_kind_mem_add_hint(kind: &str) -> Option<String> {
    match kind {
        "goal" => Some("goal is a standard memory kind; use `ai goal set ...`".into()),
        "now" => Some("now is a standard memory kind; use `ai now set ...`".into()),
        "idea" => Some("idea is a standard memory kind; use `ai idea add ...`".into()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::OutputFormat;
    use crate::ports::outbound::{AgentError, MemoryClient};
    use aibe_protocol::{ClientResponse, MemoryApplyStatus};
    use std::path::PathBuf;
    use std::sync::Mutex;

    struct PanicMemoryClient;

    impl MemoryClient for PanicMemoryClient {
        fn memory_apply(
            &self,
            _session_id: &str,
            _context: &aibe_protocol::MemoryContext,
            _operation: aibe_protocol::MemoryOperationDto,
        ) -> Result<ClientResponse, AgentError> {
            panic!("memory_apply must not be called");
        }

        fn memory_query(
            &self,
            _session_id: &str,
            _context: &aibe_protocol::MemoryContext,
            _query: aibe_protocol::MemoryQueryDto,
        ) -> Result<ClientResponse, AgentError> {
            panic!("memory_query must not be called");
        }

        fn memory_kind_list(
            &self,
            _session_id: &str,
            _context: &aibe_protocol::MemoryContext,
        ) -> Result<ClientResponse, AgentError> {
            panic!("memory_kind_list must not be called");
        }

        fn memory_recipe_run(
            &self,
            _session_id: &str,
            _context: &aibe_protocol::MemoryContext,
            _recipe: &str,
            _apply: bool,
            _user_instruction: Option<String>,
        ) -> Result<ClientResponse, AgentError> {
            panic!("memory_recipe_run must not be called");
        }
    }

    struct RecordingMemAddClient {
        applied: Mutex<Option<MemoryOperationDto>>,
    }

    impl RecordingMemAddClient {
        fn new() -> Self {
            Self {
                applied: Mutex::new(None),
            }
        }

        fn applied_operation(&self) -> Option<MemoryOperationDto> {
            self.applied.lock().ok().and_then(|g| g.clone())
        }
    }

    impl MemoryClient for RecordingMemAddClient {
        fn memory_apply(
            &self,
            _session_id: &str,
            _context: &aibe_protocol::MemoryContext,
            operation: aibe_protocol::MemoryOperationDto,
        ) -> Result<ClientResponse, AgentError> {
            if let Ok(mut guard) = self.applied.lock() {
                *guard = Some(operation);
            }
            Ok(ClientResponse::MemoryApplyResult {
                id: "m1".into(),
                status: MemoryApplyStatus::Ok,
                entries: vec![],
            })
        }

        fn memory_query(
            &self,
            _session_id: &str,
            _context: &aibe_protocol::MemoryContext,
            _query: aibe_protocol::MemoryQueryDto,
        ) -> Result<ClientResponse, AgentError> {
            panic!("memory_query must not be called");
        }

        fn memory_kind_list(
            &self,
            _session_id: &str,
            _context: &aibe_protocol::MemoryContext,
        ) -> Result<ClientResponse, AgentError> {
            panic!("memory_kind_list must not be called");
        }

        fn memory_recipe_run(
            &self,
            _session_id: &str,
            _context: &aibe_protocol::MemoryContext,
            _recipe: &str,
            _apply: bool,
            _user_instruction: Option<String>,
        ) -> Result<ClientResponse, AgentError> {
            panic!("memory_recipe_run must not be called");
        }
    }

    fn test_ctx() -> MemoryCliContext {
        MemoryCliContext {
            socket_path: PathBuf::from("/nonexistent"),
            session_id: "sess".into(),
            memory_context: aibe_protocol::MemoryContext {
                cwd: Some("/tmp".into()),
                memory_space_id: Some("ctx".into()),
            },
            cwd: PathBuf::from("/tmp"),
            format: OutputFormat::Tsv,
        }
    }

    #[test]
    fn mem_add_goal_returns_standard_kind_hint() {
        let err = run_mem_add(&PanicMemoryClient, &test_ctx(), "goal", "ship")
            .expect_err("expected error");
        assert!(matches!(err, AgentError::Request(ref m) if m.contains("ai goal set")));
    }

    #[test]
    fn mem_add_operation_sends_kind_and_text_only() {
        let op = mem_add_operation("rule", "no fat aish");
        match op {
            MemoryOperationDto::Add(add) => {
                assert_eq!(add.kind, "rule");
                assert_eq!(add.text, "no fat aish");
                assert!(add.scope.is_none());
                assert!(add.inject.is_none());
                assert!(add.status.is_none());
                assert!(add.make_active.is_none());
            }
            _ => panic!("expected add"),
        }
    }

    #[test]
    fn mem_add_delegates_defaulting_to_server_without_kind_list() {
        let client = RecordingMemAddClient::new();
        run_mem_add(&client, &test_ctx(), "custom", "memo").expect("ok");
        match client.applied_operation() {
            Some(MemoryOperationDto::Add(add)) => {
                assert_eq!(add.kind, "custom");
                assert!(add.scope.is_none());
                assert!(add.inject.is_none());
                assert!(add.status.is_none());
                assert!(add.make_active.is_none());
            }
            other => panic!("expected add operation: {other:?}"),
        }
    }

    #[test]
    fn format_kind_definitions_env_uses_shell_assignments() {
        let kinds = vec![MemoryKindDefinitionDto {
            id: "rule".into(),
            description: "rules".into(),
            default_scope: MemoryScopeDto::Project,
            default_inject: MemoryInjectPolicyDto::Pinned,
            default_status: MemoryStatusDto::Active,
            lifecycle: "active_archive".into(),
            cardinality: "multiple".into(),
            clear_from: MemoryStatusDto::Active,
            clear_to: MemoryStatusDto::Archived,
            auto_inject: true,
            on_demand: false,
            priority: 30,
            keywords: vec![],
            max_entries: Some(8),
            aliases: vec!["rule".into()],
            builtin: true,
            dedicated_cli: None,
        }];
        let out = format_kind_definitions(&kinds, OutputFormat::Env);
        assert!(out.contains("kinds[0].id='rule'\n"));
        assert!(out.contains("kinds[0].default_scope='project'\n"));
        assert!(out.contains("kinds[0].default_inject='pinned'\n"));
        assert!(!out.contains("Project"));
    }

    #[test]
    fn format_kind_definitions_tsv_uses_wire_enum_labels() {
        let kinds = vec![MemoryKindDefinitionDto {
            id: "note".into(),
            description: "memo".into(),
            default_scope: MemoryScopeDto::Project,
            default_inject: MemoryInjectPolicyDto::Manual,
            default_status: MemoryStatusDto::Open,
            lifecycle: "open_archive".into(),
            cardinality: "multiple".into(),
            clear_from: MemoryStatusDto::Open,
            clear_to: MemoryStatusDto::Archived,
            auto_inject: false,
            on_demand: false,
            priority: 100,
            keywords: vec![],
            max_entries: Some(0),
            aliases: vec!["note".into()],
            builtin: true,
            dedicated_cli: None,
        }];
        let out = format_kind_definitions(&kinds, OutputFormat::Tsv);
        assert!(out.contains("note\tmemo\tproject\tmanual\topen\t100\ttrue\t\n"));
    }
}

fn format_prompt_block(
    prompt_block: Option<&str>,
    memory_space_id: Option<&str>,
    format: OutputFormat,
) -> Result<String, AgentError> {
    let block = prompt_block.unwrap_or("");
    let space = memory_space_id.unwrap_or("(unresolved)");
    match format {
        OutputFormat::Json => {
            let payload = serde_json::json!({
                "memory_space_id": space,
                "prompt_block": block,
            });
            Ok(serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".into()))
        }
        OutputFormat::Tsv | OutputFormat::Env => {
            let body = if block.is_empty() { "(empty)" } else { block };
            Ok(format!("memory_space_id: {space}\n{body}"))
        }
    }
}

fn format_kind_definitions(kinds: &[MemoryKindDefinitionDto], format: OutputFormat) -> String {
    match format {
        OutputFormat::Json => serde_json::to_string_pretty(kinds).unwrap_or_else(|_| "[]".into()),
        OutputFormat::Tsv => {
            let mut out = String::from(
                "id\tdescription\tdefault_scope\tdefault_inject\tdefault_status\tpriority\tbuiltin\tdedicated_cli\n",
            );
            for k in kinds {
                out.push_str(&format_kind_row_tsv(k));
            }
            out
        }
        OutputFormat::Env => {
            let mut out = String::new();
            for (index, k) in kinds.iter().enumerate() {
                append_kind_definition_env(&mut out, index, k);
            }
            out
        }
    }
}

fn format_kind_row_tsv(k: &MemoryKindDefinitionDto) -> String {
    format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
        k.id,
        k.description,
        memory_scope_wire(k.default_scope),
        memory_inject_wire(k.default_inject),
        memory_status_wire(k.default_status),
        k.priority,
        k.builtin,
        k.dedicated_cli.as_deref().unwrap_or("")
    )
}

fn append_kind_definition_env(out: &mut String, index: usize, k: &MemoryKindDefinitionDto) {
    let prefix = format!("kinds[{index}]");
    append_env_line(out, &format!("{prefix}.id"), &k.id);
    append_env_line(out, &format!("{prefix}.description"), &k.description);
    append_env_line(
        out,
        &format!("{prefix}.default_scope"),
        memory_scope_wire(k.default_scope),
    );
    append_env_line(
        out,
        &format!("{prefix}.default_inject"),
        memory_inject_wire(k.default_inject),
    );
    append_env_line(
        out,
        &format!("{prefix}.default_status"),
        memory_status_wire(k.default_status),
    );
    append_env_line(out, &format!("{prefix}.priority"), &k.priority.to_string());
    append_env_line(out, &format!("{prefix}.builtin"), &k.builtin.to_string());
    append_env_line(
        out,
        &format!("{prefix}.dedicated_cli"),
        k.dedicated_cli.as_deref().unwrap_or(""),
    );
}

fn memory_scope_wire(scope: MemoryScopeDto) -> &'static str {
    match scope {
        MemoryScopeDto::Session => "session",
        MemoryScopeDto::Project => "project",
        MemoryScopeDto::Global => "global",
    }
}

fn memory_inject_wire(inject: MemoryInjectPolicyDto) -> &'static str {
    match inject {
        MemoryInjectPolicyDto::Pinned => "pinned",
        MemoryInjectPolicyDto::OnDemand => "on_demand",
        MemoryInjectPolicyDto::Manual => "manual",
        MemoryInjectPolicyDto::Never => "never",
    }
}

fn memory_status_wire(status: MemoryStatusDto) -> &'static str {
    match status {
        MemoryStatusDto::Active => "active",
        MemoryStatusDto::Inactive => "inactive",
        MemoryStatusDto::Open => "open",
        MemoryStatusDto::Archived => "archived",
    }
}

fn format_entries(entries: &[MemoryEntryDto], format: OutputFormat) -> String {
    match format {
        OutputFormat::Json => serde_json::to_string_pretty(entries).unwrap_or_else(|_| "[]".into()),
        OutputFormat::Tsv | OutputFormat::Env => {
            let mut out =
                String::from("id\tkind\tstatus\tscope\tmemory_space_id\tlast_session_id\ttext\n");
            for e in entries {
                out.push_str(&format!(
                    "{}\t{}\t{:?}\t{:?}\t{}\t{}\t{}\n",
                    e.id, e.kind, e.status, e.scope, e.memory_space_id, e.last_session_id, e.text
                ));
            }
            out
        }
    }
}
