//! memory feature 無効時の fail-closed stub。

use crate::application::memory_cli_context::MemoryCliContext;
use crate::application::memory_cli_pack::{MemoryCliPack, MemoryCommandPolicy};
use crate::domain::OutputFormat;
use crate::domain::WorkView;
use crate::ports::outbound::WorkClient;
use crate::ports::outbound::{AgentError, MemoryClient};
use aibe_protocol::WorkOperationDto;

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

pub fn run_mem_recipe<F>(
    _client: &dyn MemoryClient,
    _ctx: &MemoryCliContext,
    _recipe: &str,
    _apply: bool,
    _user_instruction: Option<&str>,
    _prompt_apply: F,
) -> Result<String, AgentError>
where
    F: FnOnce() -> bool,
{
    Err(disabled_err())
}

pub fn run_work_query(
    _client: &dyn WorkClient,
    _ctx: &MemoryCliContext,
    _view: WorkView,
) -> Result<String, AgentError> {
    Err(disabled_err())
}

pub fn run_work_apply(
    _client: &dyn WorkClient,
    _ctx: &MemoryCliContext,
    _operation: WorkOperationDto,
) -> Result<String, AgentError> {
    Err(disabled_err())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DisabledClient;

    impl WorkClient for DisabledClient {
        fn work_query(
            &self,
            _session_id: &str,
            _context: &aibe_protocol::MemoryContext,
        ) -> Result<aibe_protocol::ClientResponse, AgentError> {
            unreachable!()
        }

        fn work_apply(
            &self,
            _session_id: &str,
            _context: &aibe_protocol::MemoryContext,
            _operation: WorkOperationDto,
        ) -> Result<aibe_protocol::ClientResponse, AgentError> {
            unreachable!()
        }
    }

    #[test]
    fn work_cli_stub_rejects_when_memory_feature_is_disabled() {
        let ctx = MemoryCliContext {
            socket_path: "/tmp/aibe.sock".into(),
            session_id: "session".into(),
            memory_context: aibe_protocol::MemoryContext {
                cwd: None,
                memory_space_id: Some("project_test".into()),
            },
            cwd: "/tmp".into(),
            format: crate::domain::OutputFormat::Tsv,
        };
        let error = run_work_query(&DisabledClient, &ctx, WorkView::Dashboard)
            .expect_err("feature-off must reject");
        assert!(error.to_string().contains(MEMORY_FEATURE_DISABLED_MESSAGE));
    }
}
