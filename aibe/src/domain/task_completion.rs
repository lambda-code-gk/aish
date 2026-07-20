//! Request-local Task Completion の純粋な契約と不変条件。

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

pub const TASK_COMPLETION_QUERY_BUDGET: u8 = 2;
pub const STALL_TERMINAL_REASON: &str = "no progress between queries";
pub const CONTRACT_TEXT_MAX_BYTES: usize = 8 * 1024;
pub const CONTRACT_MAX_CRITERIA: usize = 32;
pub const CONTRACT_MAX_LIST_ITEMS: usize = 32;
pub const EVALUATION_MAX_EVIDENCE_IDS: usize = 64;
pub const EVALUATION_MAX_REQUIRED_EVIDENCE: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskContract {
    pub goal: String,
    pub criteria: Vec<CompletionCriterion>,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub deliverables: Vec<String>,
    #[serde(default)]
    pub verification: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletionCriterion {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub deliverable_is_plan: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSource {
    Tool,
    Observation,
    Verification,
    Deliverable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRecord {
    pub evidence_id: String,
    pub criterion_ids: Vec<String>,
    pub source: EvidenceSource,
    #[serde(default)]
    pub observed_after_effect: bool,
    pub summary: String,
    #[serde(default)]
    pub verified: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CriterionStatus {
    Satisfied,
    Unsatisfied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CriterionEvaluation {
    pub criterion_id: String,
    pub status: CriterionStatus,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    #[serde(default)]
    pub required_evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletionEvaluation {
    pub criteria: Vec<CriterionEvaluation>,
    #[serde(default)]
    pub next_objective: Option<String>,
    #[serde(default)]
    pub needs_user: Option<String>,
    #[serde(default)]
    pub blocked: Option<String>,
    #[serde(default)]
    pub failure: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionOutcome {
    Done,
    NeedsUser,
    Blocked,
    BudgetExhausted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressSnapshot {
    pub unsatisfied: BTreeSet<String>,
    pub evidence_fingerprint: String,
    pub normalized_failure: Option<String>,
}

impl TaskContract {
    pub fn validate(&self) -> Result<(), String> {
        validate_bounded_text("goal", &self.goal)?;
        if self.goal.trim().is_empty()
            || self.criteria.is_empty()
            || self.deliverables.is_empty()
            || self.verification.is_empty()
        {
            return Err("contract requires goal, criteria, deliverables, and verification".into());
        }
        if self.criteria.len() > CONTRACT_MAX_CRITERIA {
            return Err(format!("contract exceeds {CONTRACT_MAX_CRITERIA} criteria"));
        }
        validate_string_list("constraints", &self.constraints)?;
        validate_string_list("deliverables", &self.deliverables)?;
        validate_string_list("verification", &self.verification)?;
        let mut ids = BTreeSet::new();
        for criterion in &self.criteria {
            validate_bounded_text("criterion id", &criterion.id)?;
            validate_bounded_text("criterion description", &criterion.description)?;
            if criterion.id.trim().is_empty() || criterion.description.trim().is_empty() {
                return Err("criterion id and description must not be empty".into());
            }
            if !ids.insert(criterion.id.clone()) {
                return Err(format!("duplicate criterion id: {}", criterion.id));
            }
        }
        validate_plan_criteria(self)?;
        Ok(())
    }

    pub fn execution_criterion_ids(&self) -> Vec<String> {
        self.criteria
            .iter()
            .filter(|criterion| !criterion.deliverable_is_plan)
            .map(|criterion| criterion.id.clone())
            .collect()
    }

    pub fn plan_criterion_ids(&self) -> Vec<String> {
        self.criteria
            .iter()
            .filter(|criterion| criterion.deliverable_is_plan)
            .map(|criterion| criterion.id.clone())
            .collect()
    }

    pub fn ids(&self) -> BTreeSet<String> {
        self.criteria.iter().map(|c| c.id.clone()).collect()
    }
}

impl CompletionEvaluation {
    pub fn validate_bounded(&self) -> Result<(), String> {
        if self.criteria.len() > CONTRACT_MAX_CRITERIA {
            return Err(format!(
                "evaluation exceeds {CONTRACT_MAX_CRITERIA} criteria"
            ));
        }
        for criterion in &self.criteria {
            validate_bounded_text("criterion id", &criterion.criterion_id)?;
            if criterion.evidence_ids.len() > EVALUATION_MAX_EVIDENCE_IDS {
                return Err(format!(
                    "criterion {} exceeds {EVALUATION_MAX_EVIDENCE_IDS} evidence ids",
                    criterion.criterion_id
                ));
            }
            if criterion.required_evidence.len() > EVALUATION_MAX_REQUIRED_EVIDENCE {
                return Err(format!(
                    "criterion {} exceeds {EVALUATION_MAX_REQUIRED_EVIDENCE} required evidence entries",
                    criterion.criterion_id
                ));
            }
            for evidence_id in &criterion.evidence_ids {
                validate_bounded_text("evidence id", evidence_id)?;
            }
            for required in &criterion.required_evidence {
                validate_bounded_text("required evidence", required)?;
            }
        }
        if let Some(next_objective) = &self.next_objective {
            validate_bounded_text("next_objective", next_objective)?;
        }
        if let Some(needs_user) = &self.needs_user {
            validate_bounded_text("needs_user", needs_user)?;
        }
        if let Some(blocked) = &self.blocked {
            validate_bounded_text("blocked", blocked)?;
        }
        if let Some(failure) = &self.failure {
            validate_bounded_text("failure", failure)?;
        }
        Ok(())
    }
}

fn validate_bounded_text(field: &str, value: &str) -> Result<(), String> {
    if value.len() > CONTRACT_TEXT_MAX_BYTES {
        return Err(format!("{field} exceeds {CONTRACT_TEXT_MAX_BYTES} bytes"));
    }
    Ok(())
}

fn validate_plan_criteria(contract: &TaskContract) -> Result<(), String> {
    let has_plan = contract
        .criteria
        .iter()
        .any(|criterion| criterion.deliverable_is_plan);
    let has_execution = contract
        .criteria
        .iter()
        .any(|criterion| !criterion.deliverable_is_plan);
    if has_plan && has_execution {
        return Err(
            "deliverable_is_plan cannot be combined with execution criteria in one contract".into(),
        );
    }
    if has_plan && verification_implies_execution(&contract.verification) {
        return Err(
            "deliverable_is_plan is incompatible with execution-oriented verification".into(),
        );
    }
    Ok(())
}

fn verification_implies_execution(verification: &[String]) -> bool {
    verification.iter().any(|item| {
        let lower = item.to_lowercase();
        [
            "read", "change", "write", "after", "observe", "verify", "create", "update", "delete",
            "modify", "execute", "run",
        ]
        .iter()
        .any(|keyword| lower.contains(keyword))
    })
}

fn validate_string_list(field: &str, values: &[String]) -> Result<(), String> {
    if values.len() > CONTRACT_MAX_LIST_ITEMS {
        return Err(format!("{field} exceeds {CONTRACT_MAX_LIST_ITEMS} entries"));
    }
    for value in values {
        validate_bounded_text(field, value)?;
    }
    Ok(())
}

fn non_empty_optional(value: &Option<String>) -> bool {
    value.as_deref().is_some_and(|text| !text.trim().is_empty())
}

fn optional_present_but_blank(value: &Option<String>) -> bool {
    value.as_deref().is_some_and(|text| text.trim().is_empty())
}

pub fn validate_evaluation(
    contract: &TaskContract,
    evidence: &[EvidenceRecord],
    evaluation: &CompletionEvaluation,
) -> Result<(), String> {
    contract.validate()?;
    let expected = contract.ids();
    let actual: BTreeSet<_> = evaluation
        .criteria
        .iter()
        .map(|criterion| criterion.criterion_id.clone())
        .collect();
    if actual.len() != evaluation.criteria.len() || actual != expected {
        return Err("evaluation criterion set must exactly match contract".into());
    }
    let evidence_by_id: BTreeMap<_, _> = evidence
        .iter()
        .map(|record| (record.evidence_id.as_str(), record))
        .collect();
    if evidence_by_id.len() != evidence.len() {
        return Err("duplicate evidence id".into());
    }
    for criterion in &evaluation.criteria {
        for evidence_id in &criterion.evidence_ids {
            let record = evidence_by_id
                .get(evidence_id.as_str())
                .ok_or_else(|| format!("unknown evidence id: {evidence_id}"))?;
            if !record.criterion_ids.contains(&criterion.criterion_id) {
                return Err(format!("evidence {evidence_id} is not linked to criterion"));
            }
        }
        if criterion.status == CriterionStatus::Satisfied {
            if criterion.evidence_ids.is_empty() {
                return Err("satisfied criterion requires evidence".into());
            }
            if criterion.evidence_ids.iter().any(|id| {
                evidence_by_id
                    .get(id.as_str())
                    .is_none_or(|record| !record.verified)
            }) {
                return Err("satisfied criterion references unverified evidence".into());
            }
        }
    }
    evaluation.validate_bounded()?;
    let has_unsatisfied = evaluation
        .criteria
        .iter()
        .any(|criterion| criterion.status == CriterionStatus::Unsatisfied);
    let has_continuation = non_empty_optional(&evaluation.next_objective);
    let has_needs_user = non_empty_optional(&evaluation.needs_user);
    let has_blocked = non_empty_optional(&evaluation.blocked);
    if optional_present_but_blank(&evaluation.next_objective) {
        return Err("next_objective must not be empty".into());
    }
    if optional_present_but_blank(&evaluation.needs_user) {
        return Err("needs_user must not be empty".into());
    }
    if optional_present_but_blank(&evaluation.blocked) {
        return Err("blocked must not be empty".into());
    }
    if has_needs_user && has_blocked {
        return Err("evaluation cannot combine needs_user and blocked".into());
    }
    if has_unsatisfied {
        if (has_needs_user || has_blocked) && has_continuation {
            return Err(
                "unsatisfied evaluation cannot combine terminal reason with next_objective".into(),
            );
        }
        if !has_continuation && !has_needs_user && !has_blocked {
            return Err("unsatisfied evaluation requires next_objective or terminal reason".into());
        }
    }
    if !has_unsatisfied
        && (evaluation.next_objective.is_some()
            || evaluation.needs_user.is_some()
            || evaluation.blocked.is_some())
    {
        return Err("completed evaluation contains contradictory continuation".into());
    }
    Ok(())
}

pub fn terminal_outcome(
    evaluation: &CompletionEvaluation,
    query_count: u8,
    stalled: bool,
) -> Option<CompletionOutcome> {
    let done = evaluation
        .criteria
        .iter()
        .all(|criterion| criterion.status == CriterionStatus::Satisfied);
    if done {
        return Some(CompletionOutcome::Done);
    }
    if evaluation.needs_user.is_some() {
        return Some(CompletionOutcome::NeedsUser);
    }
    if evaluation.blocked.is_some() || stalled {
        return Some(CompletionOutcome::Blocked);
    }
    if query_count >= TASK_COMPLETION_QUERY_BUDGET {
        return Some(CompletionOutcome::BudgetExhausted);
    }
    None
}

pub fn progress_snapshot(
    evaluation: &CompletionEvaluation,
    evidence: &[EvidenceRecord],
) -> ProgressSnapshot {
    let unsatisfied = evaluation
        .criteria
        .iter()
        .filter(|c| c.status == CriterionStatus::Unsatisfied)
        .map(|c| c.criterion_id.clone())
        .collect();
    let evidence_fingerprint = evidence
        .iter()
        .map(|record| {
            format!(
                "{}:{:?}:{}:{}",
                record.evidence_id, record.source, record.observed_after_effect, record.verified
            )
        })
        .collect::<Vec<_>>()
        .join("|");
    let normalized_failure = evaluation
        .failure
        .as_ref()
        .or(evaluation.blocked.as_ref())
        .map(|value| {
            value
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase()
        });
    ProgressSnapshot {
        unsatisfied,
        evidence_fingerprint,
        normalized_failure,
    }
}

pub fn is_stalled(previous: &ProgressSnapshot, current: &ProgressSnapshot) -> bool {
    previous.unsatisfied == current.unsatisfied
        && (previous.evidence_fingerprint == current.evidence_fingerprint
            || previous
                .normalized_failure
                .as_ref()
                .zip(current.normalized_failure.as_ref())
                .is_some_and(|(left, right)| left == right))
}
