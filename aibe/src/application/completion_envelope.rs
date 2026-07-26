//! Assistant content 内の Task Completion control envelope。

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::application::tool_defs::SET_TASK_CONTRACT;
use crate::domain::{
    validate_contract_covers_request, AgentTaskRequest, CompletionEvaluation,
    DelegatedVerificationAction, TaskCompletionEligibility, TaskContract, ToolCall, AGENT_TASK,
};

pub const TASK_COMPLETION_MARKER: &str = "aish_task_completion";

/// Inactive turn fixture用の空assistant text。Active Contractはcontrol callで提出する。
pub const MINIMAL_CONTRACT_BEFORE_TOOLS: &str = "";

/// テスト用: 最小 Contract + Done evaluation。
pub fn minimal_done_envelope(deliverable: &str, evidence_ids: &[&str]) -> String {
    let ids = evidence_ids
        .iter()
        .map(|id| format!("\"{id}\""))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"aish_task_completion":{{"evaluation":{{"criteria":[{{"criterion_id":"c1","status":"satisfied","evidence_ids":[{ids}],"required_evidence":[]}}]}}}},"deliverable":{}}}"#,
        serde_json::to_string(deliverable).unwrap_or_else(|_| "\"ok\"".into())
    )
}

/// テスト用: 最小 Contract + Blocked 終端（tool 成否に依存せず評価を閉じる）。
pub fn minimal_blocked_envelope(deliverable: &str) -> String {
    format!(
        r#"{{"aish_task_completion":{{"evaluation":{{"criteria":[{{"criterion_id":"c1","status":"unsatisfied","evidence_ids":[],"required_evidence":["fixture"]}}],"blocked":"test fixture closed"}}}},"deliverable":{}}}"#,
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
    pub evaluation: CompletionEvaluation,
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
    envelope
        .aish_task_completion
        .evaluation
        .validate_bounded()?;
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
    delegated_policy: Option<DelegatedVerificationPolicy>,
}

#[derive(Debug)]
struct RequestBinding {
    eligibility: TaskCompletionEligibility,
    user_request: String,
}

#[derive(Debug)]
struct DelegatedVerificationPolicy {
    allowed_tools: std::collections::BTreeSet<String>,
    allowed_commands: Vec<String>,
}

impl Default for ContractGate {
    fn default() -> Self {
        Self::permissive()
    }
}

impl ContractGate {
    pub fn requires_contract_control(&self) -> bool {
        self.require_contract_before_tools
    }

    /// functionCall args を唯一の Contract 固定入力として検査する。
    pub fn fix_contract_from_control_args(
        &self,
        arguments: &serde_json::Value,
    ) -> Result<(), String> {
        let encoded =
            serde_json::to_vec(arguments).map_err(|_| "invalid task contract".to_string())?;
        if encoded.len() > ENVELOPE_MAX_BYTES {
            return Err("task contract exceeds 65536 bytes".into());
        }
        if !arguments.is_object() {
            return Err("task contract arguments must be an object".into());
        }
        let contract: TaskContract = serde_json::from_value(arguments.clone())
            .map_err(|_| "task contract schema is invalid".to_string())?;
        contract.validate()?;
        let binding = self
            .request_binding
            .as_ref()
            .ok_or_else(|| "task contract control is inactive".to_string())?;
        if binding.user_request.trim().is_empty() {
            return Err("task contract source request is empty".into());
        }
        validate_contract_covers_request(&contract, binding.eligibility, &binding.user_request)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| "task contract state unavailable".to_string())?;
        if state.fixed.is_some() {
            return Err("task contract already fixed".into());
        }
        if state.tool_execution_started {
            return Err("task contract cannot be fixed after tool execution".into());
        }
        state.fixed = Some(contract);
        Ok(())
    }

    pub fn mark_tool_execution_started(&self) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "task contract state unavailable".to_string())?;
        state.tool_execution_started = true;
        Ok(())
    }

    pub fn text_contract_forbidden(content: &str) -> bool {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(content) else {
            return false;
        };
        value
            .get(TASK_COMPLETION_MARKER)
            .and_then(serde_json::Value::as_object)
            .is_some_and(|body| body.contains_key("contract"))
    }

    pub fn control_count(calls: &[ToolCall]) -> usize {
        calls
            .iter()
            .filter(|call| call.name == SET_TASK_CONTRACT)
            .count()
    }

    pub fn is_fixed(&self) -> Result<bool, String> {
        self.state
            .lock()
            .map(|state| state.fixed.is_some())
            .map_err(|_| "task contract state unavailable".to_string())
    }

    /// Task Completion 非対象 turn: Contract なしでも tool を許可する。
    pub fn permissive() -> Self {
        Self {
            state: Mutex::new(ContractGateState::default()),
            require_contract_before_tools: false,
            request_binding: None,
            delegated_policy: None,
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
            delegated_policy: None,
        }
    }

    pub fn strict_with_delegated_policy(
        eligibility: TaskCompletionEligibility,
        user_request: impl Into<String>,
        allowed_tools: impl IntoIterator<Item = String>,
        allowed_commands: Vec<String>,
    ) -> Self {
        Self {
            state: Mutex::new(ContractGateState::default()),
            require_contract_before_tools: true,
            request_binding: Some(RequestBinding {
                eligibility,
                user_request: user_request.into(),
            }),
            delegated_policy: Some(DelegatedVerificationPolicy {
                allowed_tools: allowed_tools.into_iter().collect(),
                allowed_commands,
            }),
        }
    }
}

#[derive(Debug, Default)]
struct ContractGateState {
    fixed: Option<TaskContract>,
    tool_execution_started: bool,
    initial_agent_request: Option<AgentTaskRequest>,
    agent_task_calls: u8,
    expected_follow_up: Option<AgentTaskRequest>,
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
        if Self::text_contract_forbidden(content) {
            return Err("text_task_contract_forbidden".into());
        }
        if will_execute_tools {
            if state.fixed.is_none() {
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

    pub fn expect_follow_up(&self, request: AgentTaskRequest) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|error| error.to_string())?;
        if state.initial_agent_request.is_none() || state.agent_task_calls != 1 {
            return Err("follow-up can only be fixed after one initial agent_task".into());
        }
        if state.expected_follow_up.replace(request).is_some() {
            return Err("follow-up request was already fixed".into());
        }
        Ok(())
    }

    /// Agent Task の schema と親固定 plan の coverage/cwd を Worker spawn 前に検査する。
    pub fn inspect_tool_calls(
        &self,
        calls: &[ToolCall],
        base_dir: &std::path::Path,
    ) -> Result<(), String> {
        if !self.require_contract_before_tools {
            return Ok(());
        }
        let mut state = self.state.lock().map_err(|error| error.to_string())?;
        let agent_calls = calls.iter().filter(|call| call.name == AGENT_TASK).count();
        if agent_calls > 1 {
            return Err("only one agent_task is allowed in a delegated verification step".into());
        }
        for call in calls.iter().filter(|call| call.name == AGENT_TASK) {
            let contract = state
                .fixed
                .as_ref()
                .ok_or_else(|| "task contract required before agent_task".to_string())?;
            let plan = contract
                .delegated_verification
                .as_ref()
                .ok_or_else(|| "agent_task requires delegated_verification plan".to_string())?;
            if let Some(policy) = &self.delegated_policy {
                for item in &plan.items {
                    match &item.action {
                        DelegatedVerificationAction::Observation { tool, .. } => {
                            if !policy.allowed_tools.contains(tool) {
                                return Err(format!(
                                    "delegated observation tool is not allowed: {tool}"
                                ));
                            }
                        }
                        DelegatedVerificationAction::Command { command, .. } => {
                            if !policy.allowed_tools.contains(aibe_protocol::SHELL_EXEC)
                                || !command_is_allowed(command, &policy.allowed_commands)
                            {
                                return Err("delegated verification command is not allowed".into());
                            }
                        }
                    }
                }
            }
            let request: AgentTaskRequest = serde_json::from_value(call.arguments.clone())
                .map_err(|error| format!("invalid agent_task request before spawn: {error}"))?;
            let delegated_ids = request
                .completion_criteria
                .iter()
                .map(|criterion| criterion.id.clone())
                .collect::<std::collections::BTreeSet<_>>();
            if !delegated_ids.is_subset(&contract.ids()) || !plan.covers(&delegated_ids) {
                return Err(
                    "agent_task criteria must be covered by parent verification plan".into(),
                );
            }
            let effective_cwd = request
                .cwd
                .as_deref()
                .map(std::path::Path::new)
                .map(|path| {
                    if path.is_absolute() {
                        path.to_path_buf()
                    } else {
                        base_dir.join(path)
                    }
                })
                .unwrap_or_else(|| base_dir.to_path_buf());
            if effective_cwd
                .components()
                .any(|component| component == std::path::Component::ParentDir)
            {
                return Err("agent_task cwd contains parent traversal".into());
            }
            if effective_cwd != base_dir {
                return Err(
                    "delegated verification command requires agent_task cwd to equal context cwd"
                        .into(),
                );
            }
            for item in &plan.items {
                if let DelegatedVerificationAction::Command { cwd, .. } = &item.action {
                    let plan_cwd = std::path::Path::new(cwd);
                    if !plan_cwd.is_absolute()
                        || plan_cwd
                            .components()
                            .any(|component| component == std::path::Component::ParentDir)
                        || plan_cwd != effective_cwd
                    {
                        return Err("verification command cwd differs from agent_task cwd".into());
                    }
                }
            }
            if state.agent_task_calls >= 2 {
                return Err("delegated verification permits only one follow-up".into());
            }
            if let Some(initial) = &state.initial_agent_request {
                let expected = state.expected_follow_up.as_ref().ok_or_else(|| {
                    "follow-up agent_task was not fixed from the evaluated Gap".to_string()
                })?;
                if &request != expected {
                    return Err("follow-up agent_task differs from the fixed Gap request".into());
                }
                if initial.worker != request.worker
                    || initial.cwd != request.cwd
                    || initial.timeout_secs != request.timeout_secs
                {
                    return Err("follow-up changed worker/cwd/timeout".into());
                }
            } else {
                state.initial_agent_request = Some(request.clone());
            }
            state.agent_task_calls += 1;
        }
        Ok(())
    }
}

fn command_is_allowed(command: &str, allowed_commands: &[String]) -> bool {
    let normalized = command.trim().strip_prefix("./").unwrap_or(command.trim());
    let basename = std::path::Path::new(normalized)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(normalized);
    allowed_commands.iter().any(|allowed| {
        let allowed = allowed.trim();
        !allowed.is_empty()
            && if allowed.contains('/') {
                allowed == normalized
            } else {
                allowed == basename
            }
    })
}
