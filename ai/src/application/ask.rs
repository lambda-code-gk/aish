//! 質問 → aibe → 表示ユースケース。

use aibe_protocol::{ClientResponse, SHELL_LOG_TAIL_MAX_BYTES};

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

    pub fn run(&self, user_message: String, options: AskRunOptions) -> Result<(), AskError> {
        self.presenter
            .show_tools_startup(&options.resolved_tools.startup);

        let shell_log_tail = match self.log {
            Some(l) => Some(l.tail_bytes(SHELL_LOG_TAIL_MAX_BYTES)?),
            None => None,
        };

        let input = AskInput {
            user_message,
            shell_log_tail,
            client_cwd: std::env::current_dir().ok(),
            tools: options.resolved_tools.allowlist.into_names(),
            llm_profile: options.llm_profile,
        };
        let request = input.into_request()?;

        match self.client.agent_turn(&request) {
            Ok(response) => {
                if let ClientResponse::Error { code, message, .. } = &response {
                    let code =
                        serde_json::to_string(code).unwrap_or_else(|_| "\"unknown\"".to_string());
                    return Err(AgentError::Response {
                        code,
                        message: message.clone(),
                    }
                    .into());
                }
                self.presenter
                    .show_response(&response, options.verbose_tools);
                Ok(())
            }
            Err(e) => {
                self.presenter.show_error(&e.to_string());
                Err(e.into())
            }
        }
    }
}
