//! Assistant content 内の Task Completion control envelope。

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::domain::{
    validate_contract_covers_request, CompletionEvaluation, TaskCompletionEligibility, TaskContract,
};

pub const TASK_COMPLETION_MARKER: &str = "aish_task_completion";

/// テスト用の最小 Contract envelope（tool 前固定）。本番の完了判定バイパスには使わない。
pub const MINIMAL_CONTRACT_BEFORE_TOOLS: &str = r#"{"aish_task_completion":{"contract":{"goal":"execute tools","task_kind":"execution","criteria":[{"id":"c1","description":"tool step completes","deliverable_is_plan":false,"observes_targets":[]}],"constraints":[],"deliverables":["result"],"verification":["observe result"],"verification_tools":["read_file"]}},"deliverable":""}"#;

/// テスト用: 最小 Contract + Done evaluation。
pub fn minimal_done_envelope(deliverable: &str, evidence_ids: &[&str]) -> String {
    let ids = evidence_ids
        .iter()
        .map(|id| format!("\"{id}\""))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"aish_task_completion":{{"contract":{{"goal":"execute tools","task_kind":"execution","criteria":[{{"id":"c1","description":"tool step completes","deliverable_is_plan":false,"observes_targets":[]}}],"constraints":[],"deliverables":["result"],"verification":["observe result"],"verification_tools":["read_file"]}},"evaluation":{{"criteria":[{{"criterion_id":"c1","status":"satisfied","evidence_ids":[{ids}],"required_evidence":[]}}]}}}},"deliverable":{}}}"#,
        serde_json::to_string(deliverable).unwrap_or_else(|_| "\"ok\"".into())
    )
}

/// テスト用: 最小 Contract + Blocked 終端（tool 成否に依存せず評価を閉じる）。
pub fn minimal_blocked_envelope(deliverable: &str) -> String {
    format!(
        r#"{{"aish_task_completion":{{"contract":{{"goal":"execute tools","task_kind":"execution","criteria":[{{"id":"c1","description":"tool step completes","deliverable_is_plan":false,"observes_targets":[]}}],"constraints":[],"deliverables":["result"],"verification":["observe result"],"verification_tools":["read_file"]}},"evaluation":{{"criteria":[{{"criterion_id":"c1","status":"unsatisfied","evidence_ids":[],"required_evidence":["fixture"]}}],"blocked":"test fixture closed"}}}},"deliverable":{}}}"#,
        serde_json::to_string(deliverable).unwrap_or_else(|_| "\"ok\"".into())
    )
}

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
    request_binding: Option<RequestBinding>,
}

#[derive(Debug)]
struct RequestBinding {
    eligibility: TaskCompletionEligibility,
    user_request: String,
}

impl Default for ContractGate {
    fn default() -> Self {
        Self::permissive()
    }
}

impl ContractGate {
    /// Task Completion 非対象 turn: Contract なしでも tool を許可する。
    pub fn permissive() -> Self {
        Self {
            state: Mutex::new(ContractGateState::default()),
            require_contract_before_tools: false,
            request_binding: None,
        }
    }

    /// Task Completion 対象 turn: 最初の tool 前に Contract 必須。
    pub fn strict(eligibility: TaskCompletionEligibility, user_request: impl Into<String>) -> Self {
        Self {
            state: Mutex::new(ContractGateState::default()),
            require_contract_before_tools: true,
            request_binding: Some(RequestBinding {
                eligibility,
                user_request: user_request.into(),
            }),
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
        let mut state = self.state.lock().map_err(|error| error.to_string())?;
        if !self.require_contract_before_tools {
            // Inactive turn: never decode envelope. Plain explanations may mention
            // `aish_task_completion` without being control JSON.
            if will_execute_tools {
                state.tool_execution_started = true;
            }
            return Ok(false);
        }
        let envelope = decode_completion_envelope(content)?;
        if let Some(envelope) = envelope {
            let contract = envelope.aish_task_completion.contract;
            if let Some(binding) = &self.request_binding {
                validate_contract_covers_request(
                    &contract,
                    binding.eligibility,
                    &binding.user_request,
                )?;
            }
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
