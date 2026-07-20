//! 既存 Query Loop の外側で使う request-local Task Completion helpers。

use aibe_protocol::{
    CompletionCriterionReport, CompletionEvidenceReport, CompletionEvidenceSource,
    CompletionOutcome as WireOutcome, CompletionReport, ExecutedToolCall, ExecutedToolStatus,
    ToolRiskClass,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::domain::{
    terminal_outcome, validate_evaluation, CompletionEvaluation, CompletionOutcome,
    CriterionStatus, EvidenceRecord, EvidenceSource, TaskContract, STALL_TERMINAL_REASON,
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
    append_evidence_from_tools(contract, &[], calls)
}

pub fn append_evidence_from_tools(
    contract: &TaskContract,
    existing: &[EvidenceRecord],
    new_calls: &[ExecutedToolCall],
) -> Vec<EvidenceRecord> {
    let mut ledger = existing.to_vec();
    let mut effect_seen = existing.iter().any(|record| {
        record.source == EvidenceSource::Tool && record.summary.contains("status=Ok")
    });
    let execution_ids = contract.execution_criterion_ids();
    let mut next_index = existing.len();

    for call in new_calls {
        next_index += 1;
        let read_only = call.risk_class == Some(ToolRiskClass::ReadOnly);
        let ok = call.status == ExecutedToolStatus::Ok;
        let source = if read_only {
            EvidenceSource::Observation
        } else {
            EvidenceSource::Tool
        };
        let observed_after_effect = read_only && effect_seen;
        let criterion_ids = if read_only && observed_after_effect {
            pending_post_observation_criterion_ids(&ledger, contract, &call.name, &execution_ids)
        } else {
            execution_ids.clone()
        };
        let verified = read_only && ok && observed_after_effect && !criterion_ids.is_empty();
        if !read_only && ok {
            effect_seen = true;
        }
        ledger.push(EvidenceRecord {
            evidence_id: format!("e{next_index}"),
            criterion_ids,
            source,
            observed_after_effect,
            summary: bounded(&format!("{} status={:?}", call.name, call.status)),
            verified,
        });
    }

    ledger
}

fn pending_post_observation_criterion_ids(
    ledger: &[EvidenceRecord],
    contract: &TaskContract,
    tool_name: &str,
    execution_ids: &[String],
) -> Vec<String> {
    execution_ids
        .iter()
        .filter(|criterion_id| {
            ledger.iter().any(|record| {
                record.criterion_ids.contains(criterion_id)
                    && record.source == EvidenceSource::Tool
                    && !record.verified
            }) && read_observation_matches_criterion(contract, criterion_id, tool_name)
        })
        .cloned()
        .collect()
}

fn read_observation_matches_criterion(
    contract: &TaskContract,
    criterion_id: &str,
    tool_name: &str,
) -> bool {
    if !contract
        .criteria
        .iter()
        .any(|criterion| criterion.id == *criterion_id)
    {
        return false;
    }
    let verification = contract.verification.join(" ").to_lowercase();
    let tool_lower = tool_name.to_lowercase();
    verification.contains(&tool_lower)
        || (tool_lower == "read_file" && verification.contains("read"))
}

pub fn deliverable_evidence(
    contract: &TaskContract,
    deliverable: &str,
    next_index: usize,
) -> Vec<EvidenceRecord> {
    if deliverable.trim().is_empty() {
        return Vec::new();
    }
    let plan_ids = contract.plan_criterion_ids();
    if plan_ids.is_empty() {
        return Vec::new();
    }
    plan_ids
        .into_iter()
        .enumerate()
        .map(|(offset, criterion_id)| EvidenceRecord {
            evidence_id: format!("e{}", next_index + offset),
            criterion_ids: vec![criterion_id],
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
    let contract_json = serde_json::to_string(&json!({
        "aish_task_completion": { "contract": contract },
        "deliverable": ""
    }))
    .unwrap_or_else(|_| "{}".into());
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
    let evidence_lines = evidence
        .iter()
        .map(|record| {
            format!(
                "- {} (verified={}): {}",
                record.evidence_id,
                record.verified,
                bounded(&record.summary)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "[task completion continuation]\nFixed contract:\n{contract_json}\nUnsatisfied criteria:\n{unsatisfied}\nExisting evidence:\n{evidence_lines}\nNext evidence id: e{}\nNext objective: {}",
        evidence.len() + 1,
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
    let terminal_reason = evaluation
        .needs_user
        .as_deref()
        .or(evaluation.blocked.as_deref())
        .or(if stalled && outcome == CompletionOutcome::Blocked {
            evaluation
                .failure
                .as_deref()
                .or(Some(STALL_TERMINAL_REASON))
        } else {
            None
        })
        .map(bounded);
    Ok(CompletionReport {
        outcome: match outcome {
            CompletionOutcome::Done => WireOutcome::Done,
            CompletionOutcome::NeedsUser => WireOutcome::NeedsUser,
            CompletionOutcome::Blocked => WireOutcome::Blocked,
            CompletionOutcome::BudgetExhausted => WireOutcome::BudgetExhausted,
        },
        terminal_reason,
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
