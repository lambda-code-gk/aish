//! `aish.request_human_action` client tool の実行と callback 生成。

use std::sync::{Arc, Mutex};

use aibe_client::ClientToolCallRequest;
use aibe_protocol::{
    validate_client_tool_call, ClientProvidedToolSpec, ClientToolErrorKind, ClientToolResult,
    ClientToolResultStatus, ToolRiskClass,
};
use serde_json::Value;

use crate::domain::RequestHumanAction;

#[derive(Debug, thiserror::Error)]
pub enum RequestHumanActionToolError {
    #[error("invalid arguments: {0}")]
    InvalidArguments(String),
    #[error("failed to persist human action request: {0}")]
    Persist(String),
}

pub fn side_agent_request_human_action_client_tool() -> ClientProvidedToolSpec {
    ClientProvidedToolSpec {
        name: "aish.request_human_action".into(),
        description: "Request human action in the human shell.".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "instruction": { "type": "string" },
                "reason": { "type": "string" },
                "command_candidates": {
                    "type": "array",
                    "items": { "type": "string" }
                },
                "expected_completion": { "type": "string" }
            },
            "required": ["instruction", "reason", "expected_completion"]
        }),
        risk_class: ToolRiskClass::ReadOnly,
        max_output_bytes: 4_096,
    }
}

pub fn parse_request_human_action_arguments(
    arguments: &Value,
) -> Result<RequestHumanAction, RequestHumanActionToolError> {
    validate_client_tool_call("aish.request_human_action", arguments)
        .map_err(RequestHumanActionToolError::InvalidArguments)?;
    let instruction = arguments
        .get("instruction")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let reason = arguments
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let expected_completion = arguments
        .get("expected_completion")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let command_candidates = arguments
        .get("command_candidates")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    Ok(RequestHumanAction {
        instruction,
        reason,
        command_candidates,
        expected_completion,
    })
}

pub fn execute_request_human_action(
    request: &ClientToolCallRequest,
    capture: &Arc<Mutex<Option<RequestHumanAction>>>,
    on_persist: &mut Option<impl FnMut(&str, RequestHumanAction) -> Result<(), String>>,
    on_tool_running: &mut Option<impl FnMut(&str, &str) -> Result<(), String>>,
) -> Result<ClientToolResult, RequestHumanActionToolError> {
    if let Some(record) = on_tool_running {
        record(&request.call_id, "aish.request_human_action")
            .map_err(RequestHumanActionToolError::Persist)?;
    }
    let parsed = parse_request_human_action_arguments(&request.arguments)?;
    if let Some(persist) = on_persist {
        persist(&request.call_id, parsed.clone()).map_err(RequestHumanActionToolError::Persist)?;
    }
    *capture.lock().expect("request_human_action capture") = Some(parsed);
    Ok(ClientToolResult {
        id: request.id.clone(),
        turn_id: request.turn_id.clone(),
        call_id: request.call_id.clone(),
        status: ClientToolResultStatus::Ok,
        error_kind: None,
        content: "Human action request recorded durably. Ending side agent turn.".into(),
    })
}

pub fn request_human_action_error_kind(err: &RequestHumanActionToolError) -> ClientToolErrorKind {
    match err {
        RequestHumanActionToolError::InvalidArguments(_) => ClientToolErrorKind::InvalidArguments,
        RequestHumanActionToolError::Persist(_) => ClientToolErrorKind::ToolNotAllowed,
    }
}

pub fn request_human_action_tool_error_to_result(
    request: &ClientToolCallRequest,
    kind: ClientToolErrorKind,
    message: impl Into<String>,
) -> ClientToolResult {
    ClientToolResult {
        id: request.id.clone(),
        turn_id: request.turn_id.clone(),
        call_id: request.call_id.clone(),
        status: ClientToolResultStatus::Error,
        error_kind: Some(kind),
        content: message.into(),
    }
}

/// side agent turn 用: replay callback に request_human_action を合成する。
pub fn side_agent_client_tool_callback<R, P, T>(
    mut replay_callback: R,
    capture: Arc<Mutex<Option<RequestHumanAction>>>,
    mut on_persist: Option<P>,
    mut on_tool_running: Option<T>,
) -> impl FnMut(ClientToolCallRequest) -> Option<ClientToolResult>
where
    R: FnMut(ClientToolCallRequest) -> Option<ClientToolResult>,
    P: FnMut(&str, RequestHumanAction) -> Result<(), String>,
    T: FnMut(&str, &str) -> Result<(), String>,
{
    move |prompt| {
        if prompt.name == "aish.request_human_action" {
            match execute_request_human_action(
                &prompt,
                &capture,
                &mut on_persist,
                &mut on_tool_running,
            ) {
                Ok(result) => Some(result),
                Err(err) => Some(request_human_action_tool_error_to_result(
                    &prompt,
                    request_human_action_error_kind(&err),
                    err.to_string(),
                )),
            }
        } else {
            if let Some(record) = on_tool_running.as_mut() {
                if let Err(error) = record(&prompt.call_id, &prompt.name) {
                    return Some(request_human_action_tool_error_to_result(
                        &prompt,
                        ClientToolErrorKind::ToolNotAllowed,
                        error,
                    ));
                }
            }
            replay_callback(prompt)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_request_human_action_arguments_accepts_candidates() {
        let parsed = parse_request_human_action_arguments(&serde_json::json!({
            "instruction": "Run tests",
            "reason": "Need TTY",
            "command_candidates": ["cargo test"],
            "expected_completion": "tests pass"
        }))
        .expect("valid");
        assert_eq!(parsed.instruction, "Run tests");
        assert_eq!(parsed.command_candidates, vec!["cargo test".to_string()]);
    }

    #[test]
    fn request_human_action_client_tool_persists_via_callback() {
        let capture = Arc::new(Mutex::new(None));
        let persisted = Arc::new(Mutex::new(false));
        let persisted_flag = Arc::clone(&persisted);
        let mut on_persist = Some(move |_call_id: &str, _request: RequestHumanAction| {
            *persisted_flag.lock().expect("flag") = true;
            Ok(())
        });
        execute_request_human_action(
            &ClientToolCallRequest {
                id: "id".into(),
                turn_id: "turn".into(),
                call_id: "call".into(),
                name: "aish.request_human_action".into(),
                arguments: serde_json::json!({
                    "instruction": "Run tests",
                    "reason": "Need TTY",
                    "command_candidates": ["cargo test"],
                    "expected_completion": "tests pass"
                }),
            },
            &capture,
            &mut on_persist,
            &mut None::<fn(&str, &str) -> Result<(), String>>,
        )
        .expect("tool ok");
        assert!(*persisted.lock().expect("flag"));
    }
}
