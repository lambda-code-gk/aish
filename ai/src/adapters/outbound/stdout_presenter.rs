//! 標準出力プレゼンター。

use aibe::protocol::ClientResponse;

use crate::ports::outbound::Presenter;

pub struct StdoutPresenter;

impl Presenter for StdoutPresenter {
    fn show_response(&self, response: &ClientResponse) {
        match response {
            ClientResponse::AgentTurnResult {
                assistant_message, ..
            } => {
                println!("{}", assistant_message.content);
            }
            ClientResponse::Pong { id } => {
                println!("pong ({id})");
            }
            ClientResponse::Error { message, .. } => {
                eprintln!("aibe error: {message}");
            }
        }
    }

    fn show_error(&self, message: &str) {
        eprintln!("ai: {message}");
    }
}
