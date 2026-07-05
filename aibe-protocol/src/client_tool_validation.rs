//! client-provided tool の引数検証（wire 契約）。

use serde_json::Value;

use crate::ClientProvidedToolSpec;

pub fn validate_client_tool_arguments(
    spec: &ClientProvidedToolSpec,
    arguments: &Value,
) -> Result<(), String> {
    validate_client_tool_call(&spec.name, arguments)
}

pub fn validate_client_tool_call(tool_name: &str, arguments: &Value) -> Result<(), String> {
    match tool_name {
        "aish.replay_show" => validate_replay_show_arguments(arguments),
        "aish.request_human_action" => validate_request_human_action_arguments(arguments),
        other => Err(format!("unsupported client tool: {other}")),
    }
}

fn validate_replay_show_arguments(arguments: &Value) -> Result<(), String> {
    let obj = arguments
        .as_object()
        .ok_or_else(|| "arguments must be an object".to_string())?;

    let index = obj
        .get("index")
        .ok_or_else(|| "missing index".to_string())?;
    if !index.is_i64() && !index.is_u64() {
        return Err("index must be integer".into());
    }

    if let Some(stream) = obj.get("stream") {
        let value = stream
            .as_str()
            .ok_or_else(|| "stream must be string".to_string())?;
        if !matches!(value, "stdout" | "stderr" | "both") {
            return Err(format!("invalid stream: {value}"));
        }
    }

    if let Some(tail_bytes) = obj.get("tail_bytes") {
        let value = tail_bytes
            .as_u64()
            .ok_or_else(|| "tail_bytes must be integer".to_string())?;
        if !(1..=16_384).contains(&value) {
            return Err("tail_bytes out of range".into());
        }
    }

    for key in obj.keys() {
        if !matches!(key.as_str(), "index" | "stream" | "tail_bytes") {
            return Err(format!("unknown property: {key}"));
        }
    }

    Ok(())
}

fn validate_request_human_action_arguments(arguments: &Value) -> Result<(), String> {
    let obj = arguments
        .as_object()
        .ok_or_else(|| "arguments must be an object".to_string())?;

    for key in ["instruction", "reason", "expected_completion"] {
        let value = obj.get(key).ok_or_else(|| format!("missing {key}"))?;
        if !value.is_string() || value.as_str().is_some_and(str::is_empty) {
            return Err(format!("{key} must be a non-empty string"));
        }
    }

    if let Some(candidates) = obj.get("command_candidates") {
        let array = candidates
            .as_array()
            .ok_or_else(|| "command_candidates must be an array".to_string())?;
        for candidate in array {
            if !candidate.is_string() {
                return Err("command_candidates items must be strings".into());
            }
        }
    }

    for key in obj.keys() {
        if !matches!(
            key.as_str(),
            "instruction" | "reason" | "command_candidates" | "expected_completion"
        ) {
            return Err(format!("unknown property: {key}"));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClientProvidedToolSpec, ToolRiskClass};

    fn replay_show_spec() -> ClientProvidedToolSpec {
        ClientProvidedToolSpec {
            name: "aish.replay_show".into(),
            description: "show".into(),
            parameters: serde_json::json!({"type":"object"}),
            risk_class: ToolRiskClass::ReadOnly,
            max_output_bytes: 8192,
        }
    }

    #[test]
    fn validate_replay_show_arguments_accepts_minimal() {
        validate_client_tool_call("aish.replay_show", &serde_json::json!({"index": 1}))
            .expect("valid");
    }

    #[test]
    fn validate_replay_show_arguments_rejects_unknown_stream() {
        let err = validate_client_tool_call(
            "aish.replay_show",
            &serde_json::json!({"index": 1, "stream": "bogus"}),
        )
        .expect_err("stream");
        assert!(err.contains("invalid stream"));
    }

    #[test]
    fn validate_replay_show_arguments_rejects_out_of_range_tail_bytes() {
        let err = validate_client_tool_call(
            "aish.replay_show",
            &serde_json::json!({"index": 1, "tail_bytes": 99999}),
        )
        .expect_err("tail_bytes");
        assert!(err.contains("tail_bytes out of range"));
    }

    #[test]
    fn validate_client_tool_arguments_delegates_to_tool_name() {
        validate_client_tool_arguments(&replay_show_spec(), &serde_json::json!({"index": 1}))
            .expect("valid");
    }
}
