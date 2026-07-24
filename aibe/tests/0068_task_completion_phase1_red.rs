//! Spec 0068 Task Completion domain / application acceptance tests.

use aibe::application::completion_envelope::{decode_completion_envelope, ContractGate};
use aibe::application::task_completion::{
    build_continuation, build_report, deliverable_evidence, evidence_from_tools,
};
use aibe::domain::{
    bound_evidence_target, classify_task_completion_eligibility, progress_snapshot,
    terminal_outcome, validate_contract_covers_request, validate_evaluation, CompletionCriterion,
    CompletionEvaluation, CompletionOutcome, CriterionEvaluation, CriterionStatus, EvidenceRecord,
    EvidenceSource, TaskCompletionEligibility, TaskContract, TaskKind, STALL_TERMINAL_REASON,
    TASK_COMPLETION_QUERY_BUDGET,
};
use aibe_protocol::{
    ExecutedToolCall, ToolApprovalState, ToolRiskClass, APPLY_PATCH, READ_FILE, SHELL_EXEC,
};
use serde_json::json;

fn contract() -> TaskContract {
    TaskContract {
        goal: "change then verify".into(),
        task_kind: TaskKind::Execution,
        criteria: vec![CompletionCriterion {
            id: "c1".into(),
            description: "the changed state is observed".into(),
            deliverable_is_plan: false,
            observes_targets: vec!["artifact.txt".into()],
            applicability: None,
        }],
        constraints: vec!["do not expose secrets".into()],
        deliverables: vec!["updated file".into()],
        verification: vec!["read the file after changing it".into()],
        verification_tools: vec!["read_file".into()],
        delegated_verification: None,
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
            applicability_evidence_ids: vec![],
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
fn task_contract_is_stable_and_structurally_complete() {
    let eligibility = TaskCompletionEligibility::Active {
        expected_kind: TaskKind::Execution,
    };
    let gate = ContractGate::strict(eligibility, "please change artifact.txt and verify");
    let fixed = contract();
    validate_contract_covers_request(&fixed, eligibility, "please change artifact.txt and verify")
        .expect("covers request");
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

    let missing_gate = ContractGate::strict(eligibility, "please change artifact.txt and verify");
    assert!(missing_gate
        .inspect_before_tools("tool request without a contract", true)
        .unwrap_err()
        .contains("required before tool"));
    assert_eq!(missing_gate.fixed_contract().expect("gate"), None);
    assert!(missing_gate
        .inspect_before_tools(&envelope(&fixed, None), false)
        .unwrap_err()
        .contains("after tool execution"));

    let plan = TaskContract {
        goal: "produce a plan".into(),
        task_kind: TaskKind::Plan,
        criteria: vec![CompletionCriterion {
            id: "c1".into(),
            description: "plan document".into(),
            deliverable_is_plan: true,
            observes_targets: vec![],
            applicability: None,
        }],
        constraints: vec![],
        deliverables: vec!["plan".into()],
        verification: vec!["plan content present".into()],
        verification_tools: vec![],
        delegated_verification: None,
    };
    assert!(validate_contract_covers_request(
        &plan,
        eligibility,
        "please change artifact.txt and verify"
    )
    .unwrap_err()
    .contains("requires task_kind=Execution"));
    let wrong_kind_gate =
        ContractGate::strict(eligibility, "please change artifact.txt and verify");
    assert!(wrong_kind_gate
        .inspect_before_tools(&envelope(&plan, None), true)
        .unwrap_err()
        .contains("requires task_kind=Execution"));
    assert_eq!(wrong_kind_gate.fixed_contract().expect("gate"), None);

    let investigation = TaskContract {
        task_kind: TaskKind::Investigation,
        ..contract()
    };
    assert!(validate_contract_covers_request(
        &investigation,
        eligibility,
        "please change artifact.txt and verify"
    )
    .unwrap_err()
    .contains("requires task_kind=Execution"));

    assert!(matches!(
        classify_task_completion_eligibility(true, &["read_file"]),
        TaskCompletionEligibility::Inactive
    ));
    assert!(matches!(
        classify_task_completion_eligibility(false, &["write_file", "shell_exec"]),
        TaskCompletionEligibility::Inactive
    ));
    assert!(matches!(
        classify_task_completion_eligibility(true, &["shell_exec"]),
        TaskCompletionEligibility::Active {
            expected_kind: TaskKind::Execution
        }
    ));
    assert!(matches!(
        classify_task_completion_eligibility(true, &["write_file", "shell_exec"]),
        TaskCompletionEligibility::Active {
            expected_kind: TaskKind::Execution
        }
    ));
}

#[test]
fn permissive_gate_ignores_marker_in_plain_assistant_text() {
    let gate = ContractGate::permissive();
    assert_eq!(
        gate.inspect_before_tools(
            "Task Completion uses the aish_task_completion JSON envelope when enabled.",
            false,
        )
        .expect("inactive turn must not decode envelope"),
        false
    );
    assert_eq!(gate.fixed_contract().expect("gate"), None);
    assert_eq!(
        gate.inspect_before_tools("tool request without a contract", true)
            .expect("inactive turn allows tools without contract"),
        false
    );
    assert_eq!(gate.fixed_contract().expect("gate"), None);
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
        ExecutedToolCall::ok(
            "w1".into(),
            APPLY_PATCH,
            json!({"path": "artifact.txt"}),
            "changed".into(),
        )
        .with_audit(
            ToolRiskClass::WriteLike,
            ToolApprovalState::ExplicitClientOptIn,
            false,
        ),
        ExecutedToolCall::ok(
            "r1".into(),
            READ_FILE,
            json!({"path": "artifact.txt"}),
            "new state".into(),
        )
        .with_audit(
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
    assert_eq!(evidence[1].target, bound_evidence_target("artifact.txt"));
    assert!(!evidence[1].summary.contains("artifact.txt"));
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
        ExecutedToolCall::ok(
            "r2".into(),
            READ_FILE,
            json!({"path": "other.txt"}),
            "other".into(),
        )
        .with_audit(
            ToolRiskClass::ReadOnly,
            ToolApprovalState::NotRequired,
            false,
        ),
    ];
    let unrelated_evidence = evidence_from_tools(&contract(), &unrelated_read);
    assert!(
        !unrelated_evidence[1].verified,
        "wrong-target read_file must not verify the change criterion"
    );

    let rewritten = vec![
        calls[0].clone(),
        calls[1].clone(),
        ExecutedToolCall::ok(
            "w2".into(),
            APPLY_PATCH,
            json!({"path": "artifact.txt"}),
            "changed again".into(),
        )
        .with_audit(
            ToolRiskClass::WriteLike,
            ToolApprovalState::ExplicitClientOptIn,
            false,
        ),
    ];
    let stale_ledger = evidence_from_tools(&contract(), &rewritten);
    assert!(
        stale_ledger[1].stale,
        "re-write must stale prior observation"
    );
    assert!(!stale_ledger[1].verified);
    assert!(validate_evaluation(
        &contract(),
        &stale_ledger,
        &evaluation(CriterionStatus::Satisfied, &["e2"]),
    )
    .unwrap_err()
    .contains("stale"));

    let shell_rewrite = vec![
        calls[0].clone(),
        calls[1].clone(),
        ExecutedToolCall::ok(
            "s1".into(),
            SHELL_EXEC,
            json!({"command": "sed", "args": ["artifact.txt"]}),
            "changed through shell".into(),
        )
        .with_audit(
            ToolRiskClass::DangerousShell,
            ToolApprovalState::ExplicitClientOptIn,
            false,
        ),
    ];
    let shell_stale_ledger = evidence_from_tools(&contract(), &shell_rewrite);
    assert_eq!(
        shell_stale_ledger[2].source,
        EvidenceSource::UnknownShellEffect
    );
    assert!(shell_stale_ledger[1].stale);
    assert!(!shell_stale_ledger[1].verified);
    assert!(validate_evaluation(
        &contract(),
        &shell_stale_ledger,
        &evaluation(CriterionStatus::Satisfied, &["e2"]),
    )
    .unwrap_err()
    .contains("stale"));

    let shell_then_read =
        evidence_from_tools(&contract(), &[shell_rewrite[2].clone(), calls[1].clone()]);
    assert_eq!(
        shell_then_read[0].source,
        EvidenceSource::UnknownShellEffect
    );
    assert!(!shell_then_read[1].observed_after_effect);
    assert!(
        !shell_then_read[1].verified,
        "shell success is not a known prior write effect"
    );

    let investigation = TaskContract {
        goal: "inspect repository".into(),
        task_kind: TaskKind::Investigation,
        criteria: vec![CompletionCriterion {
            id: "c1".into(),
            description: "status observed".into(),
            deliverable_is_plan: false,
            observes_targets: vec![],
            applicability: None,
        }],
        constraints: vec![],
        deliverables: vec!["status report".into()],
        verification: vec!["read status".into()],
        verification_tools: vec!["read_file".into()],
        delegated_verification: None,
    };
    let inspect = evidence_from_tools(
        &investigation,
        &[ExecutedToolCall::ok(
            "r1".into(),
            READ_FILE,
            json!({"path": "README.md"}),
            "ok".into(),
        )
        .with_audit(
            ToolRiskClass::ReadOnly,
            ToolApprovalState::NotRequired,
            false,
        )],
    );
    assert!(
        inspect[0].verified,
        "investigation read-only success may verify without prior effect"
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
        target: Some("artifact.txt".into()),
        stale: false,
        plan_item_id: None,
        value_fingerprint: None,
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
    let prior = vec![EvidenceRecord {
        evidence_id: "e1".into(),
        criterion_ids: vec!["c1".into()],
        source: EvidenceSource::Tool,
        observed_after_effect: false,
        summary: "apply_patch status=Ok".into(),
        verified: false,
        target: Some("artifact.txt".into()),
        stale: false,
        plan_item_id: None,
        value_fingerprint: None,
    }];
    let continuation = build_continuation(
        &contract(),
        &evaluation(CriterionStatus::Unsatisfied, &[]),
        &prior,
    );
    assert!(!continuation.contains(original));
    assert!(continuation.contains("c1"));
    assert!(continuation.contains("post-change read"));
    assert!(continuation.contains("Next objective: read the changed file"));
    assert!(continuation.contains("Fixed contract:"));
    assert!(continuation.contains("Existing evidence:"));
    assert!(continuation.contains("criteria=[c1]"));
    assert!(!continuation.contains("target=artifact.txt"));

    let plan_contract = TaskContract {
        goal: "produce a plan".into(),
        task_kind: TaskKind::Plan,
        criteria: vec![CompletionCriterion {
            id: "c1".into(),
            description: "plan document".into(),
            deliverable_is_plan: true,
            observes_targets: vec![],
            applicability: None,
        }],
        constraints: vec![],
        deliverables: vec!["plan".into()],
        verification: vec!["plan content present".into()],
        verification_tools: vec![],
        delegated_verification: None,
    };
    plan_contract.validate().expect("plan-only contract");
    let plan_evidence = deliverable_evidence(&plan_contract, "1. inspect\n2. change", 1);
    assert_eq!(plan_evidence.len(), 1);
    assert!(plan_evidence[0].verified);

    // 日本語 verification でも task_kind 構造で plan を拒否できる（キーワード非依存）。
    let sneaky = TaskContract {
        goal: "plan".into(),
        task_kind: TaskKind::Plan,
        criteria: vec![CompletionCriterion {
            id: "c1".into(),
            description: "plan".into(),
            deliverable_is_plan: true,
            observes_targets: vec![],
            applicability: None,
        }],
        constraints: vec![],
        deliverables: vec!["plan".into()],
        verification: vec!["変更後のファイルを読む".into()],
        verification_tools: vec![],
        delegated_verification: None,
    };
    sneaky
        .validate()
        .expect("plan kind is structural, not keyword");
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
        needs_user_report.outcome,
        aibe_protocol::CompletionOutcome::NeedsUser
    );
    let mut blocked = evaluation(CriterionStatus::Unsatisfied, &[]);
    blocked.next_objective = None;
    blocked.blocked = Some("permission denied".into());
    assert_eq!(
        terminal_outcome(&blocked, 1, false),
        Some(CompletionOutcome::Blocked)
    );
    assert_eq!(
        terminal_outcome(&evaluation(CriterionStatus::Unsatisfied, &[]), 2, false),
        Some(CompletionOutcome::BudgetExhausted)
    );
    assert_eq!(
        terminal_outcome(&evaluation(CriterionStatus::Unsatisfied, &[]), 2, true),
        Some(CompletionOutcome::Blocked)
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
        summary: "effect".into(),
        verified: false,
        target: Some("artifact.txt".into()),
        stale: false,
        plan_item_id: None,
        value_fingerprint: None,
    }];
    let first = progress_snapshot(&evaluation(CriterionStatus::Unsatisfied, &[]), &evidence);
    let second = progress_snapshot(&evaluation(CriterionStatus::Unsatisfied, &[]), &evidence);
    assert!(aibe::domain::is_stalled(&first, &second));
    let mut more_evidence = evidence.clone();
    more_evidence.push(EvidenceRecord {
        evidence_id: "e2".into(),
        criterion_ids: vec!["c1".into()],
        source: EvidenceSource::Observation,
        observed_after_effect: true,
        summary: "observed".into(),
        verified: true,
        target: Some("artifact.txt".into()),
        stale: false,
        plan_item_id: None,
        value_fingerprint: None,
    });
    let progressed = progress_snapshot(
        &evaluation(CriterionStatus::Unsatisfied, &[]),
        &more_evidence,
    );
    assert!(!aibe::domain::is_stalled(&first, &progressed));
    let stalled_report = build_report(
        &contract(),
        &evidence,
        &evaluation(CriterionStatus::Unsatisfied, &[]),
        2,
        true,
    )
    .expect("stalled");
    assert_eq!(
        stalled_report.outcome,
        aibe_protocol::CompletionOutcome::Blocked
    );
    assert_eq!(
        stalled_report.terminal_reason.as_deref(),
        Some(STALL_TERMINAL_REASON)
    );
}

#[test]
fn shell_verification_is_never_trusted_from_contract() {
    use aibe_protocol::SHELL_EXEC;
    let c = TaskContract {
        goal: "run check".into(),
        task_kind: TaskKind::Execution,
        criteria: vec![CompletionCriterion {
            id: "c1".into(),
            description: "check passes".into(),
            deliverable_is_plan: false,
            observes_targets: vec![],
            applicability: None,
        }],
        constraints: vec![],
        deliverables: vec!["ok".into()],
        verification: vec!["shell check".into()],
        verification_tools: vec!["shell_exec".into()],
        delegated_verification: None,
    };
    let calls = vec![ExecutedToolCall::ok(
        "s1".into(),
        SHELL_EXEC,
        json!({"command": "echo", "args": ["hi"]}),
        "hi".into(),
    )
    .with_audit(
        ToolRiskClass::DangerousShell,
        ToolApprovalState::ExplicitClientOptIn,
        false,
    )];
    let evidence = evidence_from_tools(&c, &calls);
    assert!(c.effective_verification_tools().is_empty());
    assert_eq!(evidence[0].source, EvidenceSource::UnknownShellEffect);
    assert!(!evidence[0].verified, "{:?}", evidence[0]);
    assert!(!evidence[0].summary.contains("echo"));
    assert!(!evidence[0].summary.contains("hi"));
    assert!(evidence[0]
        .target
        .as_deref()
        .is_some_and(|target| target.starts_with("target:sha256:")));
    assert!(validate_evaluation(
        &c,
        &evidence,
        &CompletionEvaluation {
            criteria: vec![CriterionEvaluation {
                criterion_id: "c1".into(),
                status: CriterionStatus::Satisfied,
                evidence_ids: vec!["e1".into()],
                required_evidence: vec![],
                applicability_evidence_ids: vec![],
            }],
            next_objective: None,
            needs_user: None,
            blocked: None,
            failure: None,
        },
    )
    .is_err());
}
