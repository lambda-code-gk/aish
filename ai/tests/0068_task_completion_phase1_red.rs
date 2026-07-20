//! Spec 0068 client presenter acceptance test.

use ai::adapters::outbound::{render_response, render_response_structured, ShellExecRenderOptions};
use ai::domain::OutputFormat;
use aibe_protocol::{
    AgentTurnStatus, ClientResponse, CompletionCriterionReport, CompletionEvidenceReport,
    CompletionEvidenceSource, CompletionOutcome, CompletionReport, ProtocolMessageOut,
};

#[test]
fn final_report_lists_evidence_and_unverified_items() {
    for outcome in [
        CompletionOutcome::Done,
        CompletionOutcome::NeedsUser,
        CompletionOutcome::Blocked,
        CompletionOutcome::BudgetExhausted,
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
                terminal_reason: (outcome == CompletionOutcome::NeedsUser)
                    .then(|| "provide approval".into())
                    .or_else(|| {
                        (outcome == CompletionOutcome::Blocked)
                            .then(|| "same failure repeated".into())
                    }),
                criteria: vec![CompletionCriterionReport {
                    criterion_id: "c1".into(),
                    satisfied: outcome == CompletionOutcome::Done,
                    evidence: vec![CompletionEvidenceReport {
                        evidence_id: "e1".into(),
                        source: CompletionEvidenceSource::Tool,
                        summary: "apply_patch status=Ok".into(),
                        verified: false,
                    }],
                }],
                unsatisfied_criteria: (outcome != CompletionOutcome::Done)
                    .then(|| "c1".into())
                    .into_iter()
                    .collect(),
                unverified_items: vec!["e1: write is not observation".into()],
                queries_used: 2,
            }),
        };
        let rendered = render_response(&response, false, ShellExecRenderOptions::default())
            .stdout
            .expect("report");
        assert!(rendered.contains(&format!("Task completion: {outcome:?}")));
        assert!(rendered.contains("Criterion c1:"));
        assert!(rendered.contains("verified=false"));
        assert!(rendered.contains("Unverified:"));
        if outcome == CompletionOutcome::NeedsUser {
            assert!(rendered.contains("Reason: provide approval"));
        }
        if outcome == CompletionOutcome::Blocked {
            assert!(rendered.contains("Reason: same failure repeated"));
        }
        if outcome == CompletionOutcome::BudgetExhausted {
            assert!(rendered.contains("Queries used: 2"));
        }
        assert!(!rendered.contains("API_KEY"));

        for (format, expected) in [
            (OutputFormat::Json, "\"completion_report\""),
            (OutputFormat::Tsv, "completion_report.json\t"),
            (OutputFormat::Env, "AI_COMPLETION_REPORT_JSON="),
        ] {
            let structured = render_response_structured(
                &response,
                false,
                format,
                None,
                ShellExecRenderOptions::default(),
            )
            .stdout
            .expect("structured report");
            assert!(structured.contains(expected));
            assert!(structured.contains("queries_used"));
            assert!(structured.contains("unverified_items"));
        }
    }
}
