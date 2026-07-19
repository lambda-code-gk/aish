//! Minimal human handoff wire DTOs（0055 / 0061）。

use serde::{Deserialize, Serialize};

/// human shell から制御が戻ったときの実行結果カテゴリ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffExecutionOutcome {
    HumanControlReturned,
    Done,
    Blocked,
    Cancelled,
    Suspended,
}

pub const HUMAN_TASK_BRIEFING_MAX_BYTES: usize = 64 * 1024;
pub const SUGGESTED_COMMAND_MAX_BYTES: usize = 4 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanTaskRequest {
    pub objective: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instructions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggested_commands: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub completion_criteria: Vec<String>,
}

impl HumanTaskRequest {
    pub fn normalized(mut self) -> Result<Self, &'static str> {
        self.objective = self.objective.trim().to_string();
        if self.objective.is_empty() {
            return Err("objective must not be empty");
        }
        self.reason = self.reason.and_then(|value| {
            let value = value.trim().to_string();
            (!value.is_empty()).then_some(value)
        });
        normalize_nonempty_items(&mut self.instructions)?;
        normalize_suggested_commands(&mut self.suggested_commands)?;
        normalize_nonempty_items(&mut self.completion_criteria)?;
        let encoded = serde_json::to_vec(&HumanTaskBriefing::from(&self))
            .map_err(|_| "briefing serialization failed")?;
        if encoded.len() > HUMAN_TASK_BRIEFING_MAX_BYTES {
            return Err("briefing exceeds 64 KiB");
        }
        Ok(self)
    }
}

fn normalize_suggested_commands(items: &mut [String]) -> Result<(), &'static str> {
    for item in items {
        *item = item.trim().to_string();
        if !is_safe_suggested_command(item) {
            return Err("suggested command must be nonempty, control-free, and at most 4 KiB");
        }
    }
    Ok(())
}

pub fn is_safe_suggested_command(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= SUGGESTED_COMMAND_MAX_BYTES
        && !value.chars().any(char::is_control)
}

fn normalize_nonempty_items(items: &mut [String]) -> Result<(), &'static str> {
    for item in items {
        *item = item.trim().to_string();
        if item.is_empty() {
            return Err("list elements must not be empty");
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanTaskBriefing {
    pub version: u8,
    pub objective: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instructions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggested_commands: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub completion_criteria: Vec<String>,
}

impl From<&HumanTaskRequest> for HumanTaskBriefing {
    fn from(value: &HumanTaskRequest) -> Self {
        Self {
            version: 1,
            objective: value.objective.clone(),
            reason: value.reason.clone(),
            instructions: value.instructions.clone(),
            suggested_commands: value.suggested_commands.clone(),
            completion_criteria: value.completion_criteria.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanTaskResult {
    pub status: HandoffExecutionOutcome,
    pub task: HumanTaskRequest,
    /// 0062: Human Task は自動検証しない。正常復帰の Done でも常に false。
    #[serde(default)]
    pub verified: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub human_shell_exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_shell_cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell_log_range: Option<ShellLogRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation: Option<PostHandoffObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<HumanHandoffFailure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suspend_reason: Option<String>,
}

impl HumanTaskResult {
    pub fn validate(&self) -> Result<(), &'static str> {
        match self.status {
            HandoffExecutionOutcome::Done
                if self.error.is_none()
                    && !self.verified
                    && self
                        .final_shell_cwd
                        .as_ref()
                        .is_some_and(|cwd| !cwd.trim().is_empty())
                    && self.shell_log_range.is_some()
                    && self.observation.is_some()
                    && self.task_id.is_none()
                    && self.suspend_reason.is_none() =>
            {
                Ok(())
            }
            HandoffExecutionOutcome::Blocked
                if !self.verified
                    && self.task_id.is_none()
                    && self.suspend_reason.is_none()
                    && self.error.as_ref().is_some_and(|e| {
                        !e.code.trim().is_empty() && !e.message.trim().is_empty()
                    }) =>
            {
                Ok(())
            }
            HandoffExecutionOutcome::Cancelled
                if self.error.is_none()
                    && !self.verified
                    && self.task_id.is_none()
                    && self.suspend_reason.is_none() =>
            {
                Ok(())
            }
            HandoffExecutionOutcome::Suspended
                if self.error.is_none()
                    && !self.verified
                    && self
                        .task_id
                        .as_ref()
                        .is_some_and(|id| valid_human_task_id(id))
                    && self
                        .suspend_reason
                        .as_ref()
                        .is_none_or(|reason| valid_suspend_reason(reason))
                    && self
                        .final_shell_cwd
                        .as_ref()
                        .is_some_and(|cwd| !cwd.trim().is_empty())
                    && self.shell_log_range.is_some()
                    && self.observation.is_some() =>
            {
                Ok(())
            }
            HandoffExecutionOutcome::HumanControlReturned => {
                Err("legacy outcome is not valid for HumanTaskResult")
            }
            _ => Err("status/error invariant violated"),
        }
    }
}

fn valid_human_task_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 18
        && bytes.starts_with(b"ht-")
        && bytes[3..11].iter().all(u8::is_ascii_digit)
        && bytes[11] == b'-'
        && bytes[12..]
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

fn valid_suspend_reason(reason: &str) -> bool {
    reason.len() <= 4096 && !reason.chars().any(char::is_control)
}

/// 親が要求したコマンドの完了可否（human shell 返却時は不明が既定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestedCommandCompletion {
    Unknown,
}

/// shell log 参照範囲。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellLogRange {
    pub start: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<u64>,
}

/// human handoff 失敗の structured error（user denial と区別する）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanHandoffFailure {
    pub code: String,
    pub message: String,
}

/// Human Shell 内で完了した 1 command の観測事実（0061）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanTaskCommandEvidence {
    pub index: u32,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

/// Human Shell handoff 範囲から収集した command Evidence（0061）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanTaskEvidence {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<HumanTaskCommandEvidence>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub truncated: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// human shell 終了後の軽量再観測。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostHandoffObservation {
    pub cwd_exists: bool,
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_head: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell_log_tail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell_log_truncated: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub observation_errors: Vec<String>,
    /// Human Task Evidence。欠落は旧 payload 互換。`None` は収集失敗。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub human_task_evidence: Option<HumanTaskEvidence>,
}

/// 親 `shell_exec` handoff 完了時の synthetic tool result 本文。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanHandoffResult {
    pub execution_outcome: HandoffExecutionOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_command: Option<String>,
    pub requested_command_completion: RequestedCommandCompletion,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub human_shell_exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_shell_cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell_log_range: Option<ShellLogRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation: Option<PostHandoffObservation>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn human_task_request_normalizes_and_rejects_unknown_fields() {
        let request: HumanTaskRequest =
            serde_json::from_value(json!({"objective":" x ","instructions":[" y "]})).unwrap();
        let request = request.normalized().unwrap();
        assert_eq!(request.objective, "x");
        assert_eq!(request.instructions, ["y"]);
        assert!(
            serde_json::from_value::<HumanTaskRequest>(json!({"objective":"x","extra":true}))
                .is_err()
        );
    }

    #[test]
    fn human_task_result_status_error_invariants() {
        let task = HumanTaskRequest {
            objective: "x".into(),
            reason: None,
            instructions: Vec::new(),
            suggested_commands: Vec::new(),
            completion_criteria: Vec::new(),
        };
        let blocked = HumanTaskResult {
            status: HandoffExecutionOutcome::Blocked,
            task: task.clone(),
            verified: false,
            human_shell_exit_code: None,
            final_shell_cwd: None,
            shell_log_range: None,
            observation: None,
            error: None,
            task_id: None,
            suspend_reason: None,
        };
        assert!(blocked.validate().is_err());

        let done_without_lifecycle = HumanTaskResult {
            status: HandoffExecutionOutcome::Done,
            task: task.clone(),
            verified: false,
            human_shell_exit_code: None,
            final_shell_cwd: None,
            shell_log_range: None,
            observation: None,
            error: None,
            task_id: None,
            suspend_reason: None,
        };
        assert!(done_without_lifecycle.validate().is_err());

        let done = HumanTaskResult {
            status: HandoffExecutionOutcome::Done,
            task: task.clone(),
            verified: false,
            human_shell_exit_code: Some(0),
            final_shell_cwd: Some("/tmp".into()),
            shell_log_range: Some(ShellLogRange {
                start: 1,
                end: Some(2),
            }),
            observation: Some(PostHandoffObservation {
                cwd_exists: true,
                cwd: "/tmp".into(),
                git_head: None,
                git_branch: None,
                git_status: None,
                shell_log_tail: None,
                shell_log_truncated: None,
                observation_errors: Vec::new(),
                human_task_evidence: None,
            }),
            error: None,
            task_id: None,
            suspend_reason: None,
        };
        assert!(done.validate().is_ok());

        let mut suspended = done.clone();
        suspended.status = HandoffExecutionOutcome::Suspended;
        suspended.task_id = Some("ht-20260714-7f31c2".into());
        assert!(suspended.validate().is_ok());
        suspended.task_id = Some("ht-20260714-7f31c2\nsecret".into());
        assert!(suspended.validate().is_err());
        suspended.task_id = Some("ht-20260714-7f31c2".into());
        suspended.suspend_reason = Some("unsafe\u{85}reason".into());
        assert!(suspended.validate().is_err());

        let mut done_with_suspend_fields = done.clone();
        done_with_suspend_fields.task_id = Some("ht-20260714-7f31c2".into());
        assert!(done_with_suspend_fields.validate().is_err());

        let done_verified_true = HumanTaskResult {
            verified: true,
            ..done.clone()
        };
        assert!(done_verified_true.validate().is_err());
    }

    /// 0060: `collab_outcome` 系 wire 型の再導入を静的に禁止する。
    /// serde の skip だけでは `Option` field 追加を検出できないため、source を検査する。
    #[test]
    fn human_task_briefing_adds_no_protocol_schema() {
        let src = include_str!("collaborative_handoff.rs");
        // このテスト関数より前の本番定義だけを対象にする（本テスト文面の言及を除外）。
        let production = src
            .split("#[cfg(test)]")
            .next()
            .expect("production source before cfg(test)");
        for token in ["CollabOutcomeStatus", "CollabOutcome", "collab_outcome"] {
            assert!(
                !production.contains(token),
                "0060 forbids reintroducing `{token}` into aibe-protocol collaborative_handoff.rs"
            );
        }
    }

    #[test]
    fn human_task_evidence_round_trip() {
        let observation = PostHandoffObservation {
            cwd_exists: true,
            cwd: "/tmp".into(),
            git_head: None,
            git_branch: None,
            git_status: None,
            shell_log_tail: None,
            shell_log_truncated: None,
            observation_errors: Vec::new(),
            human_task_evidence: Some(HumanTaskEvidence {
                commands: vec![HumanTaskCommandEvidence {
                    index: 0,
                    command: "false".into(),
                    exit_code: Some(1),
                }],
                truncated: true,
            }),
        };
        let encoded = serde_json::to_value(&observation).expect("encode");
        let decoded: PostHandoffObservation =
            serde_json::from_value(encoded.clone()).expect("decode");
        assert_eq!(decoded, observation);
        assert_eq!(encoded["human_task_evidence"]["truncated"], json!(true));
        assert_eq!(
            encoded["human_task_evidence"]["commands"][0]["command"],
            json!("false")
        );
    }

    #[test]
    fn human_task_evidence_old_payload_decodes_without_field() {
        let old = json!({
            "cwd_exists": true,
            "cwd": "/tmp",
            "observation_errors": []
        });
        let decoded: PostHandoffObservation = serde_json::from_value(old).expect("old decode");
        assert!(decoded.human_task_evidence.is_none());
    }

    #[test]
    fn human_task_evidence_none_and_false_are_omitted() {
        let observation = PostHandoffObservation {
            cwd_exists: true,
            cwd: "/tmp".into(),
            git_head: None,
            git_branch: None,
            git_status: None,
            shell_log_tail: None,
            shell_log_truncated: None,
            observation_errors: Vec::new(),
            human_task_evidence: None,
        };
        let encoded = serde_json::to_value(&observation).expect("encode");
        assert!(encoded.get("human_task_evidence").is_none());

        let empty = HumanTaskEvidence {
            commands: Vec::new(),
            truncated: false,
        };
        let encoded_empty = serde_json::to_value(&empty).expect("encode empty");
        assert!(encoded_empty.get("commands").is_none());
        assert!(encoded_empty.get("truncated").is_none());

        let decoded_empty: HumanTaskEvidence =
            serde_json::from_value(json!({})).expect("decode empty");
        assert!(decoded_empty.commands.is_empty());
        assert!(!decoded_empty.truncated);
    }

    #[test]
    fn human_task_evidence_some_empty_round_trip() {
        let observation = PostHandoffObservation {
            cwd_exists: true,
            cwd: "/tmp".into(),
            git_head: None,
            git_branch: None,
            git_status: None,
            shell_log_tail: None,
            shell_log_truncated: None,
            observation_errors: Vec::new(),
            human_task_evidence: Some(HumanTaskEvidence {
                commands: Vec::new(),
                truncated: false,
            }),
        };
        let encoded = serde_json::to_value(&observation).expect("encode");
        assert_eq!(encoded["human_task_evidence"], json!({}));
        let decoded: PostHandoffObservation = serde_json::from_value(encoded).expect("decode");
        assert_eq!(
            decoded.human_task_evidence,
            Some(HumanTaskEvidence {
                commands: Vec::new(),
                truncated: false,
            })
        );
    }
}
