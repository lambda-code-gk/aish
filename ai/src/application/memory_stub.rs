//! memory feature 無効時の fail-closed stub。

use crate::application::memory_cli_context::MemoryCliContext;
use crate::application::memory_cli_pack::{MemoryCliPack, MemoryCommandPolicy};
use crate::domain::OutputFormat;
use crate::ports::outbound::{AgentError, MemoryClient};

pub const MEMORY_FEATURE_DISABLED_MESSAGE: &str =
    "contextual memory CLI is not available (build without memory feature)";

fn disabled_err() -> AgentError {
    AgentError::Request(MEMORY_FEATURE_DISABLED_MESSAGE.to_string())
}

pub fn run_dedicated_set(
    _pack: &MemoryCliPack<'_>,
    _kind: &str,
    _text: &str,
    _ok_line: &str,
) -> Result<String, AgentError> {
    Err(disabled_err())
}

pub fn run_dedicated_show(_pack: &MemoryCliPack<'_>, _kind: &str) -> Result<String, AgentError> {
    Err(disabled_err())
}

pub fn run_dedicated_list(_pack: &MemoryCliPack<'_>, _kind: &str) -> Result<String, AgentError> {
    Err(disabled_err())
}

pub fn run_dedicated_clear(
    _pack: &MemoryCliPack<'_>,
    _kind: &str,
    _ok_line: &str,
) -> Result<String, AgentError> {
    Err(disabled_err())
}

pub fn run_mem_add(
    _pack: &MemoryCliPack<'_>,
    _kind: &str,
    _text: &str,
) -> Result<String, AgentError> {
    Err(disabled_err())
}

pub fn run_mem_list(
    _client: &dyn MemoryClient,
    _ctx: &MemoryCliContext,
    _kind: Option<&str>,
) -> Result<String, AgentError> {
    Err(disabled_err())
}

pub fn run_mem_show(
    _client: &dyn MemoryClient,
    _ctx: &MemoryCliContext,
    _query: Option<&str>,
) -> Result<String, AgentError> {
    Err(disabled_err())
}

pub fn run_mem_clear(_pack: &MemoryCliPack<'_>, _kind: &str) -> Result<String, AgentError> {
    Err(disabled_err())
}

pub fn run_mem_kinds(
    _policy: &MemoryCommandPolicy,
    _format: OutputFormat,
) -> Result<String, AgentError> {
    Err(disabled_err())
}

pub fn run_mem_recipe_clarify_goal<F>(
    _client: &dyn MemoryClient,
    _ctx: &MemoryCliContext,
    _apply: bool,
    _user_instruction: Option<&str>,
    _prompt_apply: F,
) -> Result<String, AgentError>
where
    F: FnOnce() -> bool,
{
    Err(disabled_err())
}
