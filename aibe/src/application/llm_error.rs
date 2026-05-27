//! LLM エラーを protocol レスポンスへ写す。

use crate::ports::outbound::LlmError;
use aibe_protocol::{ClientResponse, ErrorCode};

pub fn client_response_for_llm_error(id: String, err: LlmError) -> ClientResponse {
    match err {
        LlmError::Provider(msg) => ClientResponse::error(id, ErrorCode::ProviderError, msg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_error_maps_to_provider_error() {
        let res = client_response_for_llm_error(
            "turn-1".into(),
            LlmError::Provider("rate limited".into()),
        );
        match res {
            ClientResponse::Error { code, message, .. } => {
                assert_eq!(code, ErrorCode::ProviderError);
                assert_eq!(message, "rate limited");
            }
            _ => panic!("expected error response"),
        }
    }
}
