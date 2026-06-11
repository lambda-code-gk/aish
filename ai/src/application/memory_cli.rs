//! contextual memory CLI ハンドラ。

use std::path::PathBuf;

use aibe_protocol::{
    ClientResponse, MemoryEntryDto, MemoryInjectPolicyDto, MemoryOperationDto, MemoryQueryDto,
    MemoryScopeDto, MemoryStatusDto,
};

use crate::domain::OutputFormat;
use crate::ports::outbound::{AgentError, MemoryClient};

pub struct MemoryCliContext {
    pub socket_path: PathBuf,
    pub session_id: String,
    pub cwd: PathBuf,
    pub format: OutputFormat,
}

pub fn run_goal_set(
    client: &dyn MemoryClient,
    ctx: &MemoryCliContext,
    text: &str,
) -> Result<String, AgentError> {
    let op = MemoryOperationDto::Add {
        kind: "goal".into(),
        scope: MemoryScopeDto::Project,
        inject: MemoryInjectPolicyDto::Pinned,
        status: MemoryStatusDto::Active,
        text: text.to_string(),
        make_active: true,
    };
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
    let op = MemoryOperationDto::Add {
        kind: "now".into(),
        scope: MemoryScopeDto::Session,
        inject: MemoryInjectPolicyDto::Pinned,
        status: MemoryStatusDto::Active,
        text: text.to_string(),
        make_active: true,
    };
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
    let op = MemoryOperationDto::Add {
        kind: "idea".into(),
        scope: MemoryScopeDto::Project,
        inject: MemoryInjectPolicyDto::OnDemand,
        status: MemoryStatusDto::Open,
        text: text.to_string(),
        make_active: false,
    };
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
    let response = client.memory_query(&ctx.session_id, &ctx.cwd.to_string_lossy(), query)?;
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
    let op = MemoryOperationDto::Add {
        kind: kind.to_string(),
        scope: MemoryScopeDto::Project,
        inject: MemoryInjectPolicyDto::Manual,
        status: MemoryStatusDto::Open,
        text: text.to_string(),
        make_active: false,
    };
    apply_and_summarize(client, ctx, op, &format!("mem add {kind}: {text}"))
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
    let response = client.memory_query(&ctx.session_id, &ctx.cwd.to_string_lossy(), query)?;
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
    let response = client.memory_query(&ctx.session_id, &ctx.cwd.to_string_lossy(), query)?;
    match response {
        ClientResponse::MemoryQueryResult { prompt_block, .. } => {
            format_prompt_block(prompt_block.as_deref(), ctx.format)
        }
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
    let response = client.memory_apply(&ctx.session_id, &ctx.cwd.to_string_lossy(), operation)?;
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
    let response = client.memory_query(&ctx.session_id, &ctx.cwd.to_string_lossy(), query)?;
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
    let operation = MemoryOperationDto::ClearActive {
        kind: kind.to_string(),
        scope,
    };
    let response = client.memory_apply(&ctx.session_id, &ctx.cwd.to_string_lossy(), operation)?;
    match response {
        ClientResponse::MemoryApplyResult { .. } => Ok(ok_line.to_string()),
        ClientResponse::Error { message, .. } => Err(AgentError::Request(message)),
        other => Err(AgentError::Request(format!(
            "unexpected response: {other:?}"
        ))),
    }
}

fn format_prompt_block(
    prompt_block: Option<&str>,
    format: OutputFormat,
) -> Result<String, AgentError> {
    let block = prompt_block.unwrap_or("");
    match format {
        OutputFormat::Json => {
            let payload = serde_json::json!({ "prompt_block": block });
            Ok(serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".into()))
        }
        OutputFormat::Tsv | OutputFormat::Env => {
            if block.is_empty() {
                Ok("(empty)".to_string())
            } else {
                Ok(block.to_string())
            }
        }
    }
}

fn format_entries(entries: &[MemoryEntryDto], format: OutputFormat) -> String {
    match format {
        OutputFormat::Json => serde_json::to_string_pretty(entries).unwrap_or_else(|_| "[]".into()),
        OutputFormat::Tsv | OutputFormat::Env => {
            let mut out = String::from("id\tkind\tstatus\tscope\ttext\n");
            for e in entries {
                out.push_str(&format!(
                    "{}\t{}\t{:?}\t{:?}\t{}\n",
                    e.id, e.kind, e.status, e.scope, e.text
                ));
            }
            out
        }
    }
}
