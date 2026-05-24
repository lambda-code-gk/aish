//! LLM エラーを protocol レスポンスへ写す。

use crate::ports::outbound::LlmError;
use crate::protocol::{ClientResponse, ErrorCode};

pub fn client_response_for_llm_error(id: String, err: LlmError) -> ClientResponse {
    match err {
        LlmError::Provider(msg) => ClientResponse::error(id, ErrorCode::ProviderError, msg),
        LlmError::UnknownTool(name) => ClientResponse::error(
            id,
            ErrorCode::ToolNotAllowed,
            format!("unknown tool: {name}"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_tool_maps_to_tool_not_allowed() {
        let res = client_response_for_llm_error(
            "turn-1".into(),
            LlmError::UnknownTool("delete_everything".into()),
        );
        match res {
            ClientResponse::Error { code, message, .. } => {
                assert_eq!(code, ErrorCode::ToolNotAllowed);
                assert_eq!(message, "unknown tool: delete_everything");
            }
            _ => panic!("expected error response"),
        }
    }
}
