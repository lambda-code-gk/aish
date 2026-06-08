//! 質問 → aibe → 表示ユースケース。

use aibe_protocol::{ClientResponse, SHELL_LOG_TAIL_MAX_BYTES};
use std::path::PathBuf;

use crate::domain::{AskInput, AskRequestError, ResolvedTools};
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
            llm_profile: options.llm_profile,
            ai_session_id: options.ai_session_id,
            conversation_id: options.conversation_id,
        };
        let request = input.into_request()?;

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
