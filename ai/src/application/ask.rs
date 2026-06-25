//! 質問 → aibe → 表示ユースケース。

use aibe_protocol::ClientProvidedToolSpec;
use aibe_protocol::{ClientResponse, SHELL_LOG_TAIL_MAX_BYTES};
use aish_replay::LogEvent;
use std::path::PathBuf;

use crate::domain::{AskInput, AskRequestError, RequestContextInput, ResolvedTools};
use crate::ports::outbound::{AgentClient, AgentError, LogReadError, Presenter, ShellLogSource};

#[derive(Debug, thiserror::Error)]
pub enum AskError {
    #[error(transparent)]
    Agent(#[from] AgentError),
    #[error(transparent)]
    Log(#[from] LogReadError),
    #[error(transparent)]
    Request(#[from] AskRequestError),
}

pub struct AskRunOptions {
    pub resolved_tools: ResolvedTools,
    pub verbose_tools: bool,
    pub llm_profile: Option<String>,
    pub external_command_names: Vec<String>,
    pub shell_log_tail_bytes: usize,
    pub client_cwd: Option<PathBuf>,
    pub ai_session_id: Option<String>,
    pub conversation_id: Option<String>,
    pub client_tools: Vec<ClientProvidedToolSpec>,
    pub replay_events: Vec<LogEvent>,
    pub replay_manifest_block: Option<String>,
    pub request_context: RequestContextInput,
}

#[derive(Debug, Clone)]
pub struct AskOutcome {
    pub response: ClientResponse,
    pub response_error: Option<String>,
    pub shell_log_tail_bytes: usize,
}

pub struct Ask<'a, C, P, L> {
    client: &'a C,
    presenter: &'a P,
    log: Option<&'a L>,
}

impl<'a, C, P, L> Ask<'a, C, P, L>
where
    C: AgentClient,
    P: Presenter,
    L: ShellLogSource,
{
    pub fn new(client: &'a C, presenter: &'a P, log: Option<&'a L>) -> Self {
        Self {
            client,
            presenter,
            log,
        }
    }

    pub fn run(
        &self,
        user_message: String,
        options: AskRunOptions,
    ) -> Result<AskOutcome, AskError> {
        self.presenter
            .show_tools_startup(&options.resolved_tools.startup);
        self.presenter
            .show_external_commands(&options.external_command_names);

        let shell_log_tail = match self.log {
            Some(l) => {
                Some(l.tail_bytes(options.shell_log_tail_bytes.min(SHELL_LOG_TAIL_MAX_BYTES))?)
            }
            None => None,
        };

        let input = AskInput {
            user_message,
            shell_log_tail,
            client_cwd: options.client_cwd,
            tools: options.resolved_tools.allowlist.into_names(),
            client_tools: options.client_tools,
            replay_events: options.replay_events,
            replay_manifest_block: options.replay_manifest_block,
            llm_profile: options.llm_profile,
            ai_session_id: options.ai_session_id,
            conversation_id: options.conversation_id,
        };
        let mut request = input.into_request()?;
        request.request_context = options.request_context;

        match self.client.agent_turn(&request) {
            Ok(response) => {
                if let ClientResponse::Error { code, message, .. } = &response {
                    let code =
                        serde_json::to_string(code).unwrap_or_else(|_| "\"unknown\"".to_string());
                    let err = AgentError::Response {
                        code,
                        message: message.clone(),
                    };
                    self.presenter.show_error(&err.to_string());
                    return Ok(AskOutcome {
                        response,
                        response_error: Some(err.to_string()),
                        shell_log_tail_bytes: options.shell_log_tail_bytes,
                    });
                }
                self.presenter
                    .show_response(&response, options.verbose_tools, false);
                Ok(AskOutcome {
                    response,
                    response_error: None,
                    shell_log_tail_bytes: options.shell_log_tail_bytes,
                })
            }
            Err(e) => {
                self.presenter.show_error(&e.to_string());
                Err(e.into())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AskRequest, ResolvedTools, ToolAllowlist, ToolsStartupLine};
    use crate::ports::outbound::{AgentClient, LogReadError, Presenter, ShellLogSource};

    struct MockClient;

    impl AgentClient for MockClient {
        fn agent_turn(&self, request: &AskRequest) -> Result<ClientResponse, AgentError> {
            assert!(request.replay_manifest_block.is_none());
            Ok(ClientResponse::AgentTurnResult {
                id: "turn-1".into(),
                status: aibe_protocol::AgentTurnStatus::Ok,
                assistant_message: aibe_protocol::ProtocolMessageOut {
                    role: "assistant".into(),
                    content: "ok".into(),
                },
                tool_calls: vec![],
            })
        }
    }

    struct MockPresenter;

    impl Presenter for MockPresenter {
        fn show_tools_startup(&self, _line: &ToolsStartupLine) {}
        fn show_external_commands(&self, _lines: &[String]) {}
        fn show_progress(&self, _phase: &str, _message: Option<&str>) {}
        fn show_stream_chunk(&self, _chunk: &str) {}
        fn show_response(&self, _response: &ClientResponse, _verbose_tools: bool, _progress: bool) {
        }
        fn show_error(&self, _message: &str) {}
    }

    struct MockLog;

    impl ShellLogSource for MockLog {
        fn tail_bytes(&self, _max_bytes: usize) -> Result<String, LogReadError> {
            Ok("tail".into())
        }
    }

    #[test]
    fn shell_log_tail_fallback_keeps_turn_running_without_manifest() {
        let ask = Ask::new(&MockClient, &MockPresenter, Some(&MockLog));
        let outcome = ask
            .run(
                "hello".into(),
                AskRunOptions {
                    resolved_tools: ResolvedTools {
                        allowlist: ToolAllowlist::default(),
                        startup: ToolsStartupLine {
                            enabled_list: "none".into(),
                            source_hint: None,
                            warn_shell: false,
                        },
                    },
                    verbose_tools: false,
                    llm_profile: None,
                    external_command_names: vec![],
                    shell_log_tail_bytes: 4,
                    client_cwd: None,
                    ai_session_id: None,
                    conversation_id: None,
                    client_tools: vec![],
                    replay_events: vec![],
                    replay_manifest_block: None,
                    request_context: RequestContextInput::default(),
                },
            )
            .expect("ask");
        assert!(matches!(
            outcome.response,
            ClientResponse::AgentTurnResult { .. }
        ));
    }
}
