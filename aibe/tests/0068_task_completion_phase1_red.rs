//! Spec 0068 Task Completion domain / application acceptance tests.

use aibe::application::completion_envelope::{decode_completion_envelope, ContractGate};
use aibe::application::task_completion::{
    append_evidence_from_tools, build_continuation, build_report, deliverable_evidence,
    evidence_from_tools,
};
use aibe::domain::{
    progress_snapshot, terminal_outcome, validate_evaluation, CompletionCriterion,
    CompletionEvaluation, CompletionOutcome, CriterionEvaluation, CriterionStatus, EvidenceRecord,
    EvidenceSource, TaskContract, STALL_TERMINAL_REASON, TASK_COMPLETION_QUERY_BUDGET,
};
use aibe_protocol::{ExecutedToolCall, ToolApprovalState, ToolRiskClass, APPLY_PATCH, READ_FILE};
use serde_json::json;

fn contract() -> TaskContract {
    TaskContract {
        goal: "change then verify".into(),
        criteria: vec![CompletionCriterion {
            id: "c1".into(),
            description: "the changed state is observed".into(),
            deliverable_is_plan: false,
        }],
        constraints: vec!["do not expose secrets".into()],
        deliverables: vec!["updated file".into()],
        verification: vec!["read the file after changing it".into()],
    }
}

fn evaluation(status: CriterionStatus, evidence_ids: &[&str]) -> CompletionEvaluation {
    CompletionEvaluation {
        criteria: vec![CriterionEvaluation {
            criterion_id: "c1".into(),
            status,
            evidence_ids: evidence_ids.iter().map(|id| (*id).into()).collect(),
            required_evidence: if status == CriterionStatus::Unsatisfied {
                vec!["post-change read".into()]
            } else {
                vec![]
            },
        }],
        next_objective: (status == CriterionStatus::Unsatisfied)
            .then(|| "read the changed file".into()),
        needs_user: None,
        blocked: None,
        failure: None,
    }
}

fn envelope(contract: &TaskContract, evaluation: Option<&CompletionEvaluation>) -> String {
    json!({
        "aish_task_completion": {
            "contract": contract,
            "evaluation": evaluation,
        },
        "deliverable": "bounded result"
    })
    .to_string()
}

#[test]
#[ignore = "0068: contract completeness against original user request is not yet asserted"]
fn task_contract_is_stable_and_complete() {
    let gate = ContractGate::default();
    let fixed = contract();
    assert!(gate
        .inspect_before_tools(&envelope(&fixed, None), true)
        .expect("valid contract before tool"));
    assert_eq!(gate.fixed_contract().expect("gate"), Some(fixed.clone()));

    let mut changed = fixed.clone();
    changed.criteria[0].id = "changed".into();
    assert!(gate
        .inspect_before_tools(&envelope(&changed, None), false)
        .unwrap_err()
        .contains("changed"));

    let missing_gate = ContractGate::default();
    assert!(missing_gate
        .inspect_before_tools("tool request without a contract", true)
        .unwrap_err()
        .contains("required before tool"));
    assert_eq!(missing_gate.fixed_contract().expect("gate"), None);
    assert!(missing_gate
        .inspect_before_tools(&envelope(&fixed, None), false)
        .unwrap_err()
        .contains("after tool execution"));
}

#[test]
fn assistant_claim_is_not_verified_evidence() {
    let claim = "I completed and verified everything";
    assert!(decode_completion_envelope(claim)
        .expect("plain assistant")
        .is_none());
    assert!(validate_evaluation(
        &contract(),
        &[],
        &evaluation(CriterionStatus::Satisfied, &[])
    )
    .unwrap_err()
    .contains("requires evidence"));

    let mut mixed = contract();
    mixed.criteria[0].deliverable_is_plan = true;
    assert!(mixed
        .validate()
        .unwrap_err()
        .contains("deliverable_is_plan"));
    let plan_evidence = deliverable_evidence(&contract(), "plan only", 1);
    assert!(
        plan_evidence.is_empty(),
        "execution task rejects plan evidence"
    );
}

#[test]
fn side_effect_requires_post_observation() {
    let calls = vec![
        ExecutedToolCall::ok("w1".into(), APPLY_PATCH, json!({}), "changed".into()).with_audit(
            ToolRiskClass::WriteLike,
            ToolApprovalState::ExplicitClientOptIn,
            false,
        ),
        ExecutedToolCall::ok("r1".into(), READ_FILE, json!({}), "new state".into()).with_audit(
            ToolRiskClass::ReadOnly,
            ToolApprovalState::NotRequired,
            false,
        ),
    ];
    let evidence = evidence_from_tools(&contract(), &calls);
    assert!(
        !evidence[0].verified,
        "write success is effect evidence only"
    );
    assert!(evidence[1].verified);
    assert!(evidence[1].observed_after_effect);
    assert!(validate_evaluation(
        &contract(),
        &evidence,
        &evaluation(CriterionStatus::Satisfied, &["e1"]),
    )
    .is_err());
    assert!(validate_evaluation(
        &contract(),
        &evidence,
        &evaluation(CriterionStatus::Satisfied, &["e2"]),
    )
    .is_ok());

    let pre_observation = evidence_from_tools(&contract(), &[calls[1].clone(), calls[0].clone()]);
    assert!(
        !pre_observation[0].verified,
        "an observation before the effect cannot verify a change criterion"
    );

    let unrelated_read = vec![
        calls[0].clone(),
        ExecutedToolCall::ok("pwd".into(), "pwd", json!({}), "/tmp".into()).with_audit(
            ToolRiskClass::ReadOnly,
            ToolApprovalState::NotRequired,
            false,
        ),
    ];
    let unrelated_evidence = evidence_from_tools(&contract(), &unrelated_read);
    assert!(
        !unrelated_evidence[1].verified,
        "unrelated read-only observation must not verify the change criterion"
    );
}

#[test]
fn completion_evaluator_is_structured_and_fail_closed() {
    let evidence = vec![EvidenceRecord {
        evidence_id: "e1".into(),
        criterion_ids: vec!["c1".into()],
        source: EvidenceSource::Observation,
        observed_after_effect: true,
        summary: "observed".into(),
        verified: true,
    }];
    for invalid in [
        CompletionEvaluation {
            criteria: vec![],
            ..evaluation(CriterionStatus::Satisfied, &["e1"])
        },
        evaluation(CriterionStatus::Satisfied, &["missing"]),
        CompletionEvaluation {
            next_objective: Some("contradiction".into()),
            ..evaluation(CriterionStatus::Satisfied, &["e1"])
        },
        {
            let mut both = evaluation(CriterionStatus::Unsatisfied, &[]);
            both.next_objective = Some("continue".into());
            both.needs_user = Some("approval".into());
            both
        },
        {
            let mut both = evaluation(CriterionStatus::Unsatisfied, &[]);
            both.needs_user = Some("approval".into());
            both.blocked = Some("cannot proceed".into());
            both
        },
        {
            let mut blank_next = evaluation(CriterionStatus::Unsatisfied, &[]);
            blank_next.next_objective = Some("   ".into());
            blank_next.blocked = Some("cannot proceed".into());
            blank_next
        },
        {
            let mut blank_needs_user = evaluation(CriterionStatus::Unsatisfied, &[]);
            blank_needs_user.needs_user = Some("   ".into());
            blank_needs_user.blocked = Some("cannot proceed".into());
            blank_needs_user
        },
    ] {
        assert!(validate_evaluation(&contract(), &evidence, &invalid).is_err());
    }
    assert!(decode_completion_envelope("{\"aish_task_completion\":oops}").is_err());
}

#[test]
fn continuation_is_gap_driven_and_detects_plan_only() {
    let original = "please change the production file";
    let continuation = build_continuation(
        &contract(),
        &evaluation(CriterionStatus::Unsatisfied, &[]),
        &[],
    );
    assert!(!continuation.contains(original));
    assert!(continuation.contains("c1"));
    assert!(continuation.contains("post-change read"));
    assert!(continuation.contains("Next objective: read the changed file"));
    assert!(continuation.contains("Fixed contract:"));
    assert!(continuation.contains("Existing evidence:"));

    let plan_contract = TaskContract {
        goal: "produce a plan".into(),
        criteria: vec![CompletionCriterion {
            id: "c1".into(),
            description: "plan document".into(),
            deliverable_is_plan: true,
        }],
        constraints: vec![],
        deliverables: vec!["plan".into()],
        verification: vec!["plan content present".into()],
    };
    plan_contract.validate().expect("plan-only contract");
    let plan_evidence = deliverable_evidence(&plan_contract, "1. inspect\n2. change", 1);
    assert_eq!(plan_evidence.len(), 1);
    assert!(plan_evidence[0].verified);
}

#[test]
fn terminal_outcomes_are_distinct() {
    assert_eq!(
        terminal_outcome(&evaluation(CriterionStatus::Satisfied, &["e1"]), 1, false),
        Some(CompletionOutcome::Done)
    );
    let mut needs_user = evaluation(CriterionStatus::Unsatisfied, &[]);
    needs_user.next_objective = None;
    needs_user.needs_user = Some("provide approval".into());
    assert_eq!(
        terminal_outcome(&needs_user, 1, false),
        Some(CompletionOutcome::NeedsUser)
    );
    let needs_user_report =
        build_report(&contract(), &[], &needs_user, 1, false).expect("needs-user report");
    assert_eq!(
        needs_user_report.terminal_reason.as_deref(),
        Some("provide approval")
    );
    assert!(needs_user_report
        .unverified_items
        .iter()
        .any(|item| item.contains("post-change read")));
    assert_eq!(
        terminal_outcome(&evaluation(CriterionStatus::Unsatisfied, &[]), 1, true),
        Some(CompletionOutcome::Blocked)
    );
    let stall_report = build_report(
        &contract(),
        &[],
        &evaluation(CriterionStatus::Unsatisfied, &[]),
        1,
        true,
    )
    .expect("stall report");
    assert_eq!(
        stall_report.terminal_reason.as_deref(),
        Some(STALL_TERMINAL_REASON)
    );
    assert_eq!(
        terminal_outcome(&evaluation(CriterionStatus::Unsatisfied, &[]), 2, false),
        Some(CompletionOutcome::BudgetExhausted)
    );
}

#[test]
fn progress_and_stall_are_bounded() {
    assert_eq!(TASK_COMPLETION_QUERY_BUDGET, 2);
    let evidence = vec![EvidenceRecord {
        evidence_id: "e1".into(),
        criterion_ids: vec!["c1".into()],
        source: EvidenceSource::Tool,
        observed_after_effect: false,
        summary: "secret-token-value".into(),
        verified: false,
    }];
    let first = progress_snapshot(&evaluation(CriterionStatus::Unsatisfied, &[]), &evidence);
    let second = progress_snapshot(&evaluation(CriterionStatus::Unsatisfied, &[]), &evidence);
    assert_eq!(first, second);
    assert!(!first.evidence_fingerprint.contains("secret-token-value"));
    let mut repeated_a = evaluation(CriterionStatus::Unsatisfied, &[]);
    repeated_a.failure = Some("  TOOL   timeout ".into());
    let mut repeated_b = evaluation(CriterionStatus::Unsatisfied, &[]);
    repeated_b.failure = Some("tool timeout".into());
    let mut more_evidence = evidence.clone();
    more_evidence.push(EvidenceRecord {
        evidence_id: "e2".into(),
        ..evidence[0].clone()
    });
    assert!(aibe::domain::is_stalled(
        &progress_snapshot(&repeated_a, &evidence),
        &progress_snapshot(&repeated_b, &more_evidence),
    ));
    let first_pass = evidence_from_tools(
        &contract(),
        &[
            ExecutedToolCall::ok("w1".into(), APPLY_PATCH, json!({}), "changed".into()).with_audit(
                ToolRiskClass::WriteLike,
                ToolApprovalState::ExplicitClientOptIn,
                false,
            ),
        ],
    );
    let second_pass = append_evidence_from_tools(
        &contract(),
        &first_pass,
        &[
            ExecutedToolCall::ok("r1".into(), READ_FILE, json!({}), "new".into()).with_audit(
                ToolRiskClass::ReadOnly,
                ToolApprovalState::NotRequired,
                false,
            ),
        ],
    );
    assert!(!first_pass[0].verified);
    assert_eq!(second_pass[0].evidence_id, first_pass[0].evidence_id);
    assert_eq!(second_pass[0].verified, first_pass[0].verified);
    assert!(second_pass[1].verified);
    assert!(build_report(
        &contract(),
        &evidence,
        &evaluation(CriterionStatus::Unsatisfied, &[]),
        2,
        false,
    )
    .is_ok());
}
