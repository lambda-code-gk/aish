//! 既存 Query Loop の外側で使う request-local Task Completion helpers。

use aibe_protocol::{
    CompletionCriterionReport, CompletionEvidenceReport, CompletionEvidenceSource,
    CompletionOutcome as WireOutcome, CompletionReport, ExecutedToolCall, ExecutedToolStatus,
    ToolRiskClass,
};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::domain::{
    terminal_outcome, validate_evaluation, CompletionEvaluation, CompletionOutcome,
    CriterionStatus, EvidenceRecord, EvidenceSource, TaskContract,
};
use crate::ports::outbound::TurnEventSink;

const SUMMARY_LIMIT: usize = 240;

pub fn system_instruction() -> &'static str {
    "Task completion control: respond with one JSON object and no surrounding markdown. \
The object must have aish_task_completion.contract (goal, criteria with stable id/description/deliverable_is_plan, constraints, deliverables, verification), \
optional aish_task_completion.evaluation (criteria with criterion_id/status/evidence_ids/required_evidence, next_objective, needs_user, blocked, failure), and deliverable. \
Fix the contract before requesting the first tool. Every later envelope must repeat it unchanged. \
Final responses in each query must include evaluation. Evidence ids are e1, e2, ... in tool execution order; write-like success is not verified and needs a later read-only observation."
}

pub fn evidence_from_tools(
    contract: &TaskContract,
    calls: &[ExecutedToolCall],
) -> Vec<EvidenceRecord> {
    let criterion_ids: Vec<_> = contract.ids().into_iter().collect();
    let has_successful_effect = calls.iter().any(|call| {
        call.risk_class != Some(ToolRiskClass::ReadOnly) && call.status == ExecutedToolStatus::Ok
    });
    let mut effect_seen = false;
    calls
        .iter()
        .enumerate()
        .map(|(index, call)| {
            let read_only = call.risk_class == Some(ToolRiskClass::ReadOnly);
            let ok = call.status == ExecutedToolStatus::Ok;
            let observed_after_effect = read_only && effect_seen;
            let source = if read_only {
                EvidenceSource::Observation
            } else {
                EvidenceSource::Tool
            };
            let verified = read_only && ok && (!has_successful_effect || observed_after_effect);
            if !read_only && ok {
                effect_seen = true;
            }
            EvidenceRecord {
                evidence_id: format!("e{}", index + 1),
                criterion_ids: criterion_ids.clone(),
                source,
                observed_after_effect,
                summary: bounded(&format!("{} status={:?}", call.name, call.status)),
                verified,
            }
        })
        .collect()
}

pub fn deliverable_evidence(
    contract: &TaskContract,
    deliverable: &str,
    next_index: usize,
) -> Vec<EvidenceRecord> {
    if deliverable.trim().is_empty() {
        return Vec::new();
    }
    contract
        .criteria
        .iter()
        .filter(|criterion| criterion.deliverable_is_plan)
        .enumerate()
        .map(|(offset, criterion)| EvidenceRecord {
            evidence_id: format!("e{}", next_index + offset),
            criterion_ids: vec![criterion.id.clone()],
            source: EvidenceSource::Deliverable,
            observed_after_effect: false,
            summary: bounded(deliverable),
            verified: true,
        })
        .collect()
}

pub fn build_continuation(
    contract: &TaskContract,
    evaluation: &CompletionEvaluation,
    evidence: &[EvidenceRecord],
) -> String {
    let unsatisfied = evaluation
        .criteria
        .iter()
        .filter(|criterion| criterion.status == CriterionStatus::Unsatisfied)
        .map(|criterion| {
            format!(
                "- {}: required evidence: {}",
                criterion.criterion_id,
                criterion.required_evidence.join(", ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let evidence_ids = evidence
        .iter()
        .map(|record| record.evidence_id.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "[task completion continuation]\nFixed goal: {}\nUnsatisfied criteria:\n{}\nAvailable evidence ids: {}\nNext objective: {}",
        bounded(&contract.goal),
        unsatisfied,
        evidence_ids,
        bounded(evaluation.next_objective.as_deref().unwrap_or("resolve the stated gap"))
    )
}

pub fn build_report(
    contract: &TaskContract,
    evidence: &[EvidenceRecord],
    evaluation: &CompletionEvaluation,
    queries_used: u8,
    stalled: bool,
) -> Result<CompletionReport, String> {
    validate_evaluation(contract, evidence, evaluation)?;
    let outcome = terminal_outcome(evaluation, queries_used, stalled)
        .ok_or_else(|| "completion evaluation requires another query".to_string())?;
    let criteria = evaluation
        .criteria
        .iter()
        .map(|criterion| CompletionCriterionReport {
            criterion_id: criterion.criterion_id.clone(),
            satisfied: criterion.status == CriterionStatus::Satisfied,
            evidence: criterion
                .evidence_ids
                .iter()
                .filter_map(|id| evidence.iter().find(|record| &record.evidence_id == id))
                .map(to_wire_evidence)
                .collect(),
        })
        .collect();
    let unsatisfied_criteria = evaluation
        .criteria
        .iter()
        .filter(|criterion| criterion.status == CriterionStatus::Unsatisfied)
        .map(|criterion| criterion.criterion_id.clone())
        .collect();
    let mut unverified_items: Vec<String> = evidence
        .iter()
        .filter(|record| !record.verified)
        .map(|record| format!("{}: {}", record.evidence_id, record.summary))
        .collect();
    unverified_items.extend(
        evaluation
            .criteria
            .iter()
            .filter(|criterion| criterion.status == CriterionStatus::Unsatisfied)
            .flat_map(|criterion| {
                criterion.required_evidence.iter().map(|required| {
                    bounded(&format!(
                        "{}: required evidence: {}",
                        criterion.criterion_id, required
                    ))
                })
            }),
    );
    Ok(CompletionReport {
        outcome: match outcome {
            CompletionOutcome::Done => WireOutcome::Done,
            CompletionOutcome::NeedsUser => WireOutcome::NeedsUser,
            CompletionOutcome::Blocked => WireOutcome::Blocked,
            CompletionOutcome::BudgetExhausted => WireOutcome::BudgetExhausted,
        },
        terminal_reason: evaluation
            .needs_user
            .as_deref()
            .or(evaluation.blocked.as_deref())
            .map(bounded),
        criteria,
        unsatisfied_criteria,
        unverified_items,
        queries_used,
    })
}

fn to_wire_evidence(record: &EvidenceRecord) -> CompletionEvidenceReport {
    CompletionEvidenceReport {
        evidence_id: record.evidence_id.clone(),
        source: match record.source {
            EvidenceSource::Tool => CompletionEvidenceSource::Tool,
            EvidenceSource::Observation => CompletionEvidenceSource::Observation,
            EvidenceSource::Verification => CompletionEvidenceSource::Verification,
            EvidenceSource::Deliverable => CompletionEvidenceSource::Deliverable,
        },
        summary: bounded(&record.summary),
        verified: record.verified,
    }
}

fn bounded(value: &str) -> String {
    value.chars().take(SUMMARY_LIMIT).collect()
}

/// provider delta を最終 envelope 判定まで保持し、control JSON を client へ流さない。
pub struct CompletionEventBuffer {
    inner: Arc<dyn TurnEventSink>,
    deltas: Mutex<Vec<String>>,
}

impl CompletionEventBuffer {
    pub fn new(inner: Arc<dyn TurnEventSink>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            deltas: Mutex::new(Vec::new()),
        })
    }

    pub async fn flush_for_response(&self, id: &str, response: &aibe_protocol::ClientResponse) {
        match response {
            aibe_protocol::ClientResponse::AgentTurnResult {
                assistant_message,
                completion_report: Some(_),
                ..
            } => {
                if !assistant_message.content.is_empty() {
                    self.inner
                        .assistant_streaming(id, assistant_message.content.clone())
                        .await;
                }
            }
            _ => {
                let mut deltas = self.deltas.lock().await;
                let contains_control = deltas
                    .join("")
                    .contains(crate::application::completion_envelope::TASK_COMPLETION_MARKER);
                if !contains_control {
                    for delta in deltas.drain(..) {
                        self.inner.assistant_streaming(id, delta).await;
                    }
                }
                deltas.clear();
            }
        }
        self.inner.final_response(id).await;
    }
}

#[async_trait]
impl TurnEventSink for CompletionEventBuffer {
    async fn progress(
        &self,
        id: &str,
        phase: aibe_protocol::ProgressPhase,
        message: Option<String>,
    ) {
        self.inner.progress(id, phase, message).await;
    }

    async fn assistant_streaming(&self, _id: &str, delta: String) {
        self.deltas.lock().await.push(delta);
    }

    async fn final_response(&self, _id: &str) {}
}
