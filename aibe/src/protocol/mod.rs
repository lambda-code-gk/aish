//! クライアント向けプロトコル型（`ai` が依存する公開 API）。

mod request;
mod response;

pub use request::{ClientRequest, ProtocolMessage, ProtocolMessageConversionError, RequestContext};
pub use response::{AgentTurnStatus, ClientResponse, ErrorCode, ProtocolMessageOut};
