//! 質問 → aibe → 表示ユースケース。

use aibe::protocol::ClientResponse;

use crate::domain::{print_tools_startup, AskInput, ResolvedTools};
use crate::ports::outbound::{AgentClient, AgentError, LogReadError, Presenter, ShellLogSource};

#[derive(Debug, thiserror::Error)]
pub enum AskError {
    #[error(transparent)]
    Agent(#[from] AgentError),
    #[error(transparent)]
    Log(#[from] LogReadError),
}

pub struct AskRunOptions {
    pub resolved_tools: ResolvedTools,
    pub verbose_tools: bool,
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
        print_tools_startup(&options.resolved_tools.startup);

        let shell_log_tail = match self.log {
            Some(l) => Some(l.tail_bytes(16 * 1024)?),
            None => None,
        };

        let input = AskInput {
            user_message,
            shell_log_tail,
            tools: options.resolved_tools.names,
        };

        match self.client.agent_turn(&input) {
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
