//! contextual memory ランタイムの有効/無効（Phase A）。

/// memory RPC / subscribe / CLI が拒否されるときのメッセージ。
pub const MEMORY_DISABLED_MESSAGE: &str =
    "contextual memory is disabled ([memory] enabled = false in aibe config)";

pub fn memory_disabled_response(id: String) -> aibe_protocol::ClientResponse {
    use aibe_protocol::{ClientResponse, ErrorCode};
    ClientResponse::error(id, ErrorCode::InvalidRequest, MEMORY_DISABLED_MESSAGE)
}
