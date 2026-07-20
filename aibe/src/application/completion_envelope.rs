//! Assistant content 内の Task Completion control envelope。

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::domain::{CompletionEvaluation, TaskContract};

pub const TASK_COMPLETION_MARKER: &str = "aish_task_completion";

/// RequestService 経由で tool を伴う provider step に載せる最小 Contract（回帰テスト用）。
pub const MINIMAL_CONTRACT_BEFORE_TOOLS: &str = r#"{"aish_task_completion":{"contract":{"goal":"execute tools","criteria":[{"id":"c1","description":"tool step completes","deliverable_is_plan":false}],"constraints":[],"deliverables":["result"],"verification":["observe result"]}},"deliverable":""}"#;

pub const ENVELOPE_MAX_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletionEnvelope {
    pub aish_task_completion: CompletionEnvelopeBody,
    #[serde(default)]
    pub deliverable: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletionEnvelopeBody {
    pub contract: TaskContract,
    #[serde(default)]
    pub evaluation: Option<CompletionEvaluation>,
}

pub fn decode_completion_envelope(content: &str) -> Result<Option<CompletionEnvelope>, String> {
    if !content.contains(TASK_COMPLETION_MARKER) {
        return Ok(None);
    }
    if content.len() > ENVELOPE_MAX_BYTES {
        return Err(format!(
            "task completion envelope exceeds {ENVELOPE_MAX_BYTES} bytes"
        ));
    }
    let envelope: CompletionEnvelope = serde_json::from_str(content)
        .map_err(|error| format!("invalid task completion envelope: {error}"))?;
    envelope.aish_task_completion.contract.validate()?;
    if let Some(evaluation) = &envelope.aish_task_completion.evaluation {
        evaluation.validate_bounded()?;
    }
    if envelope.deliverable.len() > crate::domain::CONTRACT_TEXT_MAX_BYTES {
        return Err(format!(
            "deliverable exceeds {} bytes",
            crate::domain::CONTRACT_TEXT_MAX_BYTES
        ));
    }
    Ok(Some(envelope))
}

/// 一つの request 内で Contract を最初の tool 実行前に固定する gate。
#[derive(Debug)]
pub struct ContractGate {
    state: Mutex<ContractGateState>,
    require_contract_before_tools: bool,
}

impl Default for ContractGate {
    fn default() -> Self {
        Self::strict()
    }
}

impl ContractGate {
    pub fn strict() -> Self {
        Self {
            state: Mutex::new(ContractGateState::default()),
            require_contract_before_tools: true,
        }
    }
}

#[derive(Debug, Default)]
struct ContractGateState {
    fixed: Option<TaskContract>,
    tool_execution_started: bool,
}

impl ContractGate {
    pub fn inspect_before_tools(
        &self,
        content: &str,
        will_execute_tools: bool,
    ) -> Result<bool, String> {
        let envelope = decode_completion_envelope(content)?;
        let mut state = self.state.lock().map_err(|error| error.to_string())?;
        if let Some(envelope) = envelope {
            let contract = envelope.aish_task_completion.contract;
            match state.fixed.as_ref() {
                None if state.tool_execution_started => {
                    return Err("task contract appeared after tool execution started".into())
                }
                None => state.fixed = Some(contract),
                Some(existing) if existing == &contract => {}
                Some(_) => return Err("task contract changed after it was fixed".into()),
            }
        }
        if will_execute_tools {
            if state.fixed.is_none() && self.require_contract_before_tools {
                state.tool_execution_started = true;
                return Err("task contract required before tool execution".into());
            }
            state.tool_execution_started = true;
        }
        Ok(state.fixed.is_some())
    }

    pub fn fixed_contract(&self) -> Result<Option<TaskContract>, String> {
        self.state
            .lock()
            .map(|state| state.fixed.clone())
            .map_err(|error| error.to_string())
    }
}
