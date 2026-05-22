//! クライアント向けプロトコル型（`ai` が依存する公開 API）。

mod request;
mod response;

pub use request::{ClientRequest, ProtocolMessage, RequestContext};
pub use response::{ClientResponse, ErrorCode, ProtocolMessageOut};
