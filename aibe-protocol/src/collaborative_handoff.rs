//! Minimal human handoff wire DTOs（0055 / 0061）。

use serde::{Deserialize, Serialize};

/// human shell から制御が戻ったときの実行結果カテゴリ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffExecutionOutcome {
    HumanControlReturned,
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
