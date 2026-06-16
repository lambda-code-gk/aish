//! contextual memory CLI ハンドラ。

use aibe_protocol::{
    ClientResponse, MemoryEntryDto, MemoryInjectPolicyDto, MemoryKindDefinitionDto,
    MemoryOperationClearKind, MemoryOperationDto, MemoryQueryDto, MemoryRecipeProposalDto,
    MemoryScopeDto, MemoryStatusDto,
};

use super::api::append_env_line;
use super::api::MemoryCliContext;
use super::api::OutputFormat;
use super::api::{AgentError, MemoryClient};
use super::memory_cli_pack::MemoryCliPack;
use super::memory_command_policy::MemoryCommandPolicy;

pub fn run_dedicated_set(
    pack: &MemoryCliPack<'_>,
    kind: &str,
    text: &str,
    ok_line: &str,
) -> Result<String, AgentError> {
    let op = pack.policy.add_operation(kind, text);
    apply_and_summarize(pack.client, pack.ctx, op, ok_line)
}

pub fn run_dedicated_show(pack: &MemoryCliPack<'_>, kind: &str) -> Result<String, AgentError> {
    query_show(pack, kind)
}

pub fn run_dedicated_list(pack: &MemoryCliPack<'_>, kind: &str) -> Result<String, AgentError> {
    query_list(pack, kind)
}

pub fn run_dedicated_clear(
    pack: &MemoryCliPack<'_>,
    kind: &str,
    ok_line: &str,
) -> Result<String, AgentError> {
    let scope = pack.policy.clear_scope(kind);
    clear_active(pack.client, pack.ctx, kind, scope, ok_line)
}

pub fn run_mem_add(pack: &MemoryCliPack<'_>, kind: &str, text: &str) -> Result<String, AgentError> {
    if let Some(message) = pack.policy.mem_add_dedicated_hint(kind) {
        return Err(AgentError::Request(message));
    }
    let op = pack.policy.generic_add_operation(kind, text);
    apply_and_summarize(
        pack.client,
        pack.ctx,
        op,
        &format!("mem add {kind}: {text}"),
    )
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

pub fn run_mem_clear(pack: &MemoryCliPack<'_>, kind: &str) -> Result<String, AgentError> {
    let scope = pack.policy.clear_scope(kind);
    clear_active(
        pack.client,
        pack.ctx,
        kind,
        scope,
        &format!("mem clear {kind}"),
    )
}

pub fn run_mem_kinds(
    policy: &MemoryCommandPolicy,
    format: OutputFormat,
) -> Result<String, AgentError> {
    Ok(match format {
        OutputFormat::Json => format_kind_definitions(policy.kinds(), format),
        OutputFormat::Tsv | OutputFormat::Env => {
            format_kind_definitions(&policy.ordered_kinds(), format)
        }
    })
}

/// `ai mem run <recipe>` — LLM 提案を表示し、任意で apply する。
pub fn run_mem_recipe(
    client: &dyn MemoryClient,
    ctx: &MemoryCliContext,
    recipe: &str,
    apply: bool,
    user_instruction: Option<&str>,
    confirm_apply: impl FnOnce() -> bool,
) -> Result<String, AgentError> {
    let response = client.memory_recipe_run(
        &ctx.session_id,
        &ctx.memory_context,
        recipe,
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

fn query_show(pack: &MemoryCliPack<'_>, kind: &str) -> Result<String, AgentError> {
    let scope = pack.policy.show_query_scope(kind);
    let status = pack.policy.show_query_status(kind);
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
    let response =
        pack.client
            .memory_query(&pack.ctx.session_id, &pack.ctx.memory_context, query)?;
    match response {
        ClientResponse::MemoryQueryResult { entries, .. } => {
            if entries.is_empty() {
                Ok(format!("{kind}: (none)"))
            } else {
                Ok(format_entries(&entries, pack.ctx.format))
            }
        }
        ClientResponse::Error { message, .. } => Err(AgentError::Request(message)),
        other => Err(AgentError::Request(format!(
            "unexpected response: {other:?}"
        ))),
    }
}

fn query_list(pack: &MemoryCliPack<'_>, kind: &str) -> Result<String, AgentError> {
    let scope = pack.policy.list_query_scope(kind);
    let status = pack.policy.list_query_status(kind);
    let query = MemoryQueryDto {
        kind: Some(kind.to_string()),
        scope: Some(scope),
        status: Some(status),
        active_only: false,
        include_archived: false,
        limit: None,
        include_prompt_block: false,
        user_query: None,
    };
    let response =
        pack.client
            .memory_query(&pack.ctx.session_id, &pack.ctx.memory_context, query)?;
    match response {
        ClientResponse::MemoryQueryResult { entries, .. } => {
            Ok(format_entries(&entries, pack.ctx.format))
        }
        ClientResponse::Error { message, .. } => Err(AgentError::Request(message)),
        other => Err(AgentError::Request(format!(
            "unexpected response: {other:?}"
        ))),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_memory::api::OutputFormat;
    use crate::plugin_memory::api::{AgentError, MemoryClient};
    use crate::plugin_memory::memory_cli_pack::MemoryCliPack;
    use crate::plugin_memory::memory_command_policy::MemoryCommandPolicy;
    use aibe_protocol::{
        ClientResponse, MemoryApplyStatus, MemoryInjectPolicyDto, MemoryKindDefinitionDto,
        MemoryOperationDto, MemoryScopeDto, MemoryStatusDto,
    };
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

    fn goal_policy() -> MemoryCommandPolicy {
        MemoryCommandPolicy::from_kinds(vec![MemoryKindDefinitionDto {
            id: "goal".into(),
            description: "goal".into(),
            default_scope: MemoryScopeDto::Project,
            default_inject: MemoryInjectPolicyDto::Pinned,
            default_status: MemoryStatusDto::Active,
            lifecycle: "active_inactive".into(),
            cardinality: "single_effective".into(),
            clear_from: MemoryStatusDto::Active,
            clear_to: MemoryStatusDto::Inactive,
            auto_inject: true,
            on_demand: false,
            priority: 10,
            keywords: vec![],
            max_entries: Some(1),
            aliases: vec!["goal".into()],
            builtin: true,
            dedicated_cli: Some("ai goal set".into()),
        }])
    }

    fn test_pack<'a>(
        client: &'a dyn MemoryClient,
        ctx: &'a MemoryCliContext,
        policy: &'a MemoryCommandPolicy,
    ) -> MemoryCliPack<'a> {
        MemoryCliPack::new(client, ctx, policy)
    }

    #[test]
    fn mem_add_goal_returns_standard_kind_hint() {
        let ctx = test_ctx();
        let policy = goal_policy();
        let pack = test_pack(&PanicMemoryClient, &ctx, &policy);
        let err = run_mem_add(&pack, "goal", "ship").expect_err("expected error");
        assert!(matches!(err, AgentError::Request(ref m) if m.contains("ai goal set")));
    }

    #[test]
    fn mem_add_generic_operation_sends_kind_and_text_only() {
        let policy = MemoryCommandPolicy::from_kinds(vec![]);
        match policy.generic_add_operation("rule", "no fat aish") {
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
    fn mem_add_delegates_defaulting_to_server() {
        let client = RecordingMemAddClient::new();
        let ctx = test_ctx();
        let policy = MemoryCommandPolicy::from_kinds(vec![]);
        let pack = test_pack(&client, &ctx, &policy);
        run_mem_add(&pack, "custom", "memo").expect("ok");
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

    #[test]
    fn mem_kinds_json_preserves_input_snapshot_order() {
        let policy = MemoryCommandPolicy::from_kinds(vec![
            MemoryKindDefinitionDto {
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
            },
            MemoryKindDefinitionDto {
                id: "goal".into(),
                description: "goal".into(),
                default_scope: MemoryScopeDto::Project,
                default_inject: MemoryInjectPolicyDto::Pinned,
                default_status: MemoryStatusDto::Active,
                lifecycle: "active_inactive".into(),
                cardinality: "single_effective".into(),
                clear_from: MemoryStatusDto::Active,
                clear_to: MemoryStatusDto::Inactive,
                auto_inject: true,
                on_demand: false,
                priority: 10,
                keywords: vec![],
                max_entries: Some(1),
                aliases: vec!["goal".into()],
                builtin: true,
                dedicated_cli: Some("ai goal set".into()),
            },
        ]);
        let json = format_kind_definitions(policy.kinds(), OutputFormat::Json);
        let parsed: Vec<MemoryKindDefinitionDto> = serde_json::from_str(&json).expect("json");
        let ids = parsed
            .iter()
            .map(|kind| kind.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["note", "goal"]);
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
