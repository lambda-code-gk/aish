use ai::adapters::outbound::{render_response, render_response_structured, ShellExecRenderOptions};
use ai::domain::OutputFormat;
use aibe_protocol::{
    AgentTurnStatus, ClientResponse, CompletionCriterionReport, CompletionCriterionStatus,
    CompletionEvidenceReport, CompletionEvidenceSource, CompletionGapReport, CompletionOutcome,
    CompletionReport, ErrorCode, ProtocolMessageOut, VerificationTerminal,
};

#[test]
fn verification_report_is_bounded_and_auditable() {
    for (outcome, terminal) in [
        (CompletionOutcome::Done, VerificationTerminal::Done),
        (
            CompletionOutcome::NeedsUser,
            VerificationTerminal::NeedsUser,
        ),
        (CompletionOutcome::Blocked, VerificationTerminal::Blocked),
        (CompletionOutcome::Blocked, VerificationTerminal::Stagnated),
        (
            CompletionOutcome::BudgetExhausted,
            VerificationTerminal::BudgetExhausted,
        ),
    ] {
        let response = ClientResponse::AgentTurnResult {
            id: "turn".into(),
            status: AgentTurnStatus::Ok,
            assistant_message: ProtocolMessageOut {
                role: "assistant".into(),
                content: "result".into(),
            },
            tool_calls: vec![],
            completion_report: Some(CompletionReport {
                outcome,
                terminal_reason: Some("TOKEN=super-secret-token-value ".repeat(40)),
                criteria: vec![CompletionCriterionReport {
                    criterion_id: "c1".into(),
                    satisfied: terminal == VerificationTerminal::Done,
                    evidence: vec![CompletionEvidenceReport {
                        evidence_id: "e1".into(),
                        source: CompletionEvidenceSource::Verification,
                        summary: "bounded command result".into(),
                        verified: true,
                    }],
                    evaluation_status: Some(if terminal == VerificationTerminal::Done {
                        CompletionCriterionStatus::Satisfied
                    } else {
                        CompletionCriterionStatus::Unknown
                    }),
                }],
                unsatisfied_criteria: vec![],
                unverified_items: vec!["verification incomplete".into()],
                queries_used: 2,
                verification_terminal: Some(terminal),
                gaps: vec![CompletionGapReport {
                    criterion_id: "c1".into(),
                    observed: "artifact incomplete".into(),
                    required_work: "repair artifact".into(),
                    verification_plan_item_ids: vec!["v1".into()],
                }],
                worker_id: Some("fixture".into()),
                follow_up_count: Some(1),
            }),
        };
        let output = render_response(&response, false, ShellExecRenderOptions::default())
            .stdout
            .expect("output");
        assert!(output.contains(&format!("Verification terminal: {terminal:?}")));
        assert!(output.contains("Criterion c1:"));
        assert!(output.contains("source=Verification verified=true"));
        assert!(output.contains("Gap c1:"));
        assert!(output.contains("Worker: fixture"));
        assert!(output.contains("Follow-ups used: 1"));
        assert!(!output.contains("super-secret-token-value"));
        assert!(output.len() < 1800);

        let structured = render_response_structured(
            &response,
            false,
            OutputFormat::Json,
            None,
            ShellExecRenderOptions::default(),
        )
        .stdout
        .expect("structured output");
        let value: serde_json::Value = serde_json::from_str(&structured).expect("valid JSON");
        assert_eq!(
            value["completion_report"]["verification_terminal"],
            serde_json::to_value(terminal).expect("terminal")
        );
        assert!(!structured.contains("super-secret-token-value"));
        assert!(structured.len() < 2400);
    }

    for response in [
        ClientResponse::error(
            "id".into(),
            ErrorCode::ToolError,
            "TOKEN=super-secret-token-value ".repeat(40),
        ),
        ClientResponse::Cancelled {
            id: "id".into(),
            turn_id: "turn".into(),
            reason: Some("TOKEN=super-secret-token-value ".repeat(40)),
        },
    ] {
        let output = render_response(&response, false, ShellExecRenderOptions::default());
        assert!(output.stdout.is_none());
        assert_eq!(output.stderr.len(), 1);
        assert!(!output.stderr[0].contains("super-secret-token-value"));
        assert!(output.stderr[0].len() < 300);

        let structured = render_response_structured(
            &response,
            false,
            OutputFormat::Json,
            None,
            ShellExecRenderOptions::default(),
        )
        .stdout
        .expect("structured terminal output");
        assert!(!structured.contains("super-secret-token-value"));
        assert!(structured.len() < 600);
    }
}
