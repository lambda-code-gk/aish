//! 既存 Query Loop の外側で使う request-local Task Completion helpers。

use aibe_protocol::{
    CompletionCriterionReport, CompletionCriterionStatus as WireCriterionStatus,
    CompletionEvidenceReport, CompletionEvidenceSource, CompletionGapReport,
    CompletionOutcome as WireOutcome, CompletionReport, ExecutedToolCall, ExecutedToolStatus,
    ProcessExecutionTerminal, ToolRiskClass, VerificationTerminal as WireVerificationTerminal,
    APPLY_PATCH, SHELL_EXEC, WRITE_FILE,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::domain::{
    bound_evidence_target, build_gap, criterion_has_conflicting_observation_fingerprints,
    gap_follow_up_request, sha256_hex, terminal_outcome, validate_evaluation, AgentTaskRequest,
    AgentTaskResult, CompletionEvaluation, CompletionOutcome, CriterionObservationState,
    CriterionStatus, DelegatedVerificationAction, EvidenceRecord, EvidenceSource,
    GapObservationKind, TaskContract, TaskKind, STALL_TERMINAL_REASON,
};

use crate::ports::outbound::TurnEventSink;

const SUMMARY_LIMIT: usize = 240;
pub const COMPLETION_REPORT_MAX_BYTES: usize = 128 * 1024;
pub const COMPLETION_REPORT_MAX_EVIDENCE_PER_CRITERION: usize = 8;
pub const COMPLETION_REPORT_MAX_EVIDENCE_TOTAL: usize = 64;
pub const COMPLETION_REPORT_MAX_UNVERIFIED_ITEMS: usize = 32;
pub const COMPLETION_REPORT_TEXT_MAX_BYTES: usize = 1024;

pub fn system_instruction(expected_kind: TaskKind) -> String {
    let kind = match expected_kind {
        TaskKind::Plan => "plan",
        TaskKind::Investigation => "investigation",
        TaskKind::Execution => "execution",
    };
    format!(
        "Task completion control for a {kind} request: before the first effect or read-only tool, call set_task_contract exactly once with the TaskContract object itself as arguments (goal, task_kind={kind}, criteria with stable id/description/deliverable_is_plan/observes_targets, constraints as a string array, deliverables as a string array, verification as a string array, verification_tools as a string array). \
The control function causes no external side effect and may be returned without assistant text. Never include the Contract in assistant text after it is fixed. \
Final responses in each query must be one JSON object whose only top-level keys are aish_task_completion and deliverable (no markdown). \
Put evaluation inside aish_task_completion. Put deliverable at the top level as a sibling of aish_task_completion — never inside aish_task_completion. \
evaluation.criteria is an array of criterion objects only: each uses criterion_id, status, evidence_ids (string array, e.g. [\"e1\"]), required_evidence (string array, never a bare string), and required_work when applicable. \
Do not put next_objective, needs_user, blocked, or failure inside a criterion object. Those belong on the evaluation object itself. \
When any criterion status is unsatisfied or unknown, evaluation MUST include exactly one of: non-empty next_objective (continue to another query), needs_user, or blocked. Omitting all three is invalid. \
When delegated_verification is set, every unsatisfied or unknown criterion MUST include non-empty required_work (criterion-specific repair instruction, <=1024 bytes). Omit required_work for satisfied / not_applicable. \
When every criterion is satisfied or not_applicable, omit next_objective/needs_user/blocked. \
Example satisfied: {{\"aish_task_completion\":{{\"evaluation\":{{\"criteria\":[{{\"criterion_id\":\"c1\",\"status\":\"satisfied\",\"evidence_ids\":[\"e2\"],\"required_evidence\":[]}}]}}}},\"deliverable\":\"done\"}}. \
Example unsatisfied continue: {{\"aish_task_completion\":{{\"evaluation\":{{\"criteria\":[{{\"criterion_id\":\"c1\",\"status\":\"unsatisfied\",\"evidence_ids\":[\"e1\"],\"required_evidence\":[\"read_file verification\"],\"required_work\":\"recreate worker-artifact.txt with exact content from-worker\"}}],\"next_objective\":\"run the planned read_file and re-evaluate\"}}}},\"deliverable\":\"\"}}. \
Do not place API keys, tokens, credentials, environment-variable values, or secret contents in Contract fields. Evidence ids are e1, e2, ... in effect/read-only tool execution order; the control call is not Evidence. \
Write-like success is not verified until a later matching observation or verification tool on the same target. \
When status is satisfied, evidence_ids must contain only verified Observation or Verification ids (after write_file, use the later read_file id such as e2). Never mark satisfied using only the write_file effect id. \
Only server-trusted dedicated read-only tools may verify. For delegated work, only a shell_exec exactly matching the pre-fixed delegated_verification command/args/cwd is Verification; every other shell_exec remains UnknownShellEffect. \
If you will call agent_task, set_task_contract MUST include a non-empty delegated_verification plan before the first agent_task: items with stable id, criterion_ids covering the delegated criteria, action observation{{tool,target}} and/or one command{{command,args,cwd}}, and expected_success. Calling agent_task without that plan is invalid. \
After the first agent_task tool result, do not call agent_task again in the same query. Execute every delegated_verification plan item (command then observations such as read_file) before the evaluation JSON. Never emit evaluation until every plan item in the current query has executed. A second agent_task is allowed only in query 2 when the continuation supplies the exact Gap follow-up AgentTaskRequest. \
If delegated verification is unsatisfied after that parent re-observation, query 2 must execute the exact AgentTaskRequest supplied in the continuation (including Gap instructions). In query 2 after follow-up agent_task, never cite stale=true evidence ids from query 1; use only fresh verification/observation ids from the current query. \
Investigation trusted read-only successes may verify without a prior write. Do not mark deliverable_is_plan for execution/investigation."
    )
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
    let execution_ids = contract.execution_criterion_ids();
    let verification_tools = contract.effective_verification_tools();
    let mut next_index = existing.len();

    for call in new_calls {
        next_index += 1;
        let target = tool_target(call);
        let read_only = call.risk_class == Some(ToolRiskClass::ReadOnly);
        let write_like = call.risk_class == Some(ToolRiskClass::WriteLike)
            || matches!(call.name.as_str(), WRITE_FILE | APPLY_PATCH);
        let shell =
            call.risk_class == Some(ToolRiskClass::DangerousShell) || call.name == SHELL_EXEC;
        let ok = call.status == ExecutedToolStatus::Ok;
        let verification_tool = verification_tools.iter().any(|name| name == &call.name);
        let delegated_command = matching_delegated_command(contract, call);
        let plan_item_id = delegated_command.map(|item| item.id.clone()).or_else(|| {
            matching_delegated_observation(contract, call, target.as_deref())
                .map(|item| item.id.clone())
        });

        if shell && ok {
            // 任意 shell command は変更対象を信頼して限定できないため、過去の観測を
            // 保守的にすべて無効化する。
            for record in &mut ledger {
                if matches!(
                    record.source,
                    EvidenceSource::Observation | EvidenceSource::Verification
                ) {
                    record.verified = false;
                    record.stale = true;
                }
            }
        } else if call.name == aibe_protocol::AGENT_TASK && ok {
            // Worker は cwd 配下へ広範囲に副作用し得るため、観測を保守的に無効化する。
            for record in &mut ledger {
                if matches!(
                    record.source,
                    EvidenceSource::Observation | EvidenceSource::Verification
                ) {
                    record.verified = false;
                    record.stale = true;
                }
            }
        } else if write_like && ok {
            if let Some(target) = target.as_ref() {
                for record in &mut ledger {
                    if record.target.as_ref() == Some(target)
                        && matches!(
                            record.source,
                            EvidenceSource::Observation | EvidenceSource::Verification
                        )
                    {
                        record.verified = false;
                        record.stale = true;
                    }
                }
            }
        }

        let (source, observed_after_effect, criterion_ids, verified) = classify_evidence(
            contract,
            &ledger,
            &execution_ids,
            call,
            target.as_deref(),
            read_only,
            write_like,
            shell,
            ok,
            verification_tool,
            delegated_command,
        );

        ledger.push(EvidenceRecord {
            evidence_id: format!("e{next_index}"),
            criterion_ids,
            source,
            observed_after_effect,
            summary: bounded(&format!(
                "{} status={:?}{}",
                call.name,
                call.status,
                structured_observation_suffix(call)
            )),
            verified,
            target,
            stale: false,
            plan_item_id,
            value_fingerprint: call
                .output
                .as_deref()
                .map(|output| format!("sha256:{}", sha256_hex(output.as_bytes()))),
        });
    }

    ledger
}

fn structured_observation_suffix(call: &ExecutedToolCall) -> String {
    let mut fields = Vec::new();
    if let Some(terminal) = call.process_terminal {
        let terminal = match terminal {
            ProcessExecutionTerminal::NonzeroExit => "nonzero_exit",
            ProcessExecutionTerminal::ObservableTimeout => "observable_timeout",
            ProcessExecutionTerminal::InfrastructureFailure => "infrastructure_failure",
        };
        fields.push(format!("process_terminal={terminal}"));
    }
    if call.name == aibe_protocol::AGENT_TASK
        && call
            .output
            .as_deref()
            .and_then(|output| serde_json::from_str::<AgentTaskResult>(output).ok())
            .is_some_and(|result| result.observation_incomplete)
    {
        fields.push("observation_incomplete=true".into());
    }
    if fields.is_empty() {
        String::new()
    } else {
        format!(" {}", fields.join(" "))
    }
}

#[allow(clippy::too_many_arguments)]
fn classify_evidence(
    contract: &TaskContract,
    ledger: &[EvidenceRecord],
    execution_ids: &[String],
    call: &ExecutedToolCall,
    target: Option<&str>,
    read_only: bool,
    write_like: bool,
    shell: bool,
    ok: bool,
    verification_tool: bool,
    delegated_command: Option<&crate::domain::DelegatedVerificationPlanItem>,
) -> (EvidenceSource, bool, Vec<String>, bool) {
    if call.name == aibe_protocol::AGENT_TASK {
        return (
            EvidenceSource::AgentTask,
            false,
            execution_ids.to_vec(),
            false,
        );
    }
    if write_like {
        return (EvidenceSource::Tool, false, execution_ids.to_vec(), false);
    }

    // plan 一致の実プロセス実行は Ok/Error とも Verification。
    // verified は status=Ok のときだけ（非 zero は unsatisfied/unknown 観測であり Failed ではない）。
    if shell {
        if let Some(item) = delegated_command {
            return (
                EvidenceSource::Verification,
                true,
                item.criterion_ids.clone(),
                ok,
            );
        }
    }

    if read_only && ok && contract.delegated_verification.is_some() {
        if let Some(item) = matching_delegated_observation(contract, call, target) {
            let after_effect = has_prior_effect(ledger, target);
            // 委譲前に固定した plan item 自体が verification-tool 指定なので、
            // contract.verification_tools の既定（Execution は read_file/git_diff）に
            // 含まれない grep / list_dir / git_status でも after_effect なら verified にする。
            return (
                EvidenceSource::Observation,
                after_effect,
                item.criterion_ids.clone(),
                after_effect,
            );
        }
        return (EvidenceSource::Observation, false, Vec::new(), false);
    }

    if read_only && ok {
        match contract.task_kind {
            TaskKind::Investigation => {
                let criterion_ids =
                    matching_criterion_ids(contract, ledger, execution_ids, target, false, false);
                return (
                    EvidenceSource::Observation,
                    false,
                    criterion_ids.clone(),
                    verification_tool && (!criterion_ids.is_empty() || execution_ids.len() == 1),
                );
            }
            TaskKind::Execution => {
                let after_effect = has_prior_effect(ledger, target);
                let criterion_ids = matching_criterion_ids(
                    contract,
                    ledger,
                    execution_ids,
                    target,
                    true,
                    after_effect,
                );
                let verified = after_effect && !criterion_ids.is_empty() && verification_tool;
                return (
                    EvidenceSource::Observation,
                    after_effect,
                    criterion_ids,
                    verified,
                );
            }
            TaskKind::Plan => {
                return (EvidenceSource::Observation, false, Vec::new(), false);
            }
        }
    }

    if shell {
        return (
            EvidenceSource::UnknownShellEffect,
            false,
            execution_ids.to_vec(),
            false,
        );
    }

    (
        if read_only {
            EvidenceSource::Observation
        } else {
            EvidenceSource::Tool
        },
        false,
        execution_ids.to_vec(),
        false,
    )
}

fn matching_criterion_ids(
    contract: &TaskContract,
    ledger: &[EvidenceRecord],
    execution_ids: &[String],
    target: Option<&str>,
    require_pending_effect: bool,
    after_effect: bool,
) -> Vec<String> {
    execution_ids
        .iter()
        .filter(|criterion_id| {
            let criterion = contract
                .criteria
                .iter()
                .find(|item| item.id == **criterion_id);
            let Some(criterion) = criterion else {
                return false;
            };
            if require_pending_effect {
                let pending = ledger.iter().any(|record| {
                    record.criterion_ids.contains(criterion_id)
                        && matches!(
                            record.source,
                            EvidenceSource::Tool
                                | EvidenceSource::AgentTask
                                | EvidenceSource::Verification
                        )
                        && !record.verified
                        && !record.stale
                        && (target.is_none()
                            || record.target.is_none()
                            || record.target.as_deref() == target)
                });
                if !pending || !after_effect {
                    return false;
                }
            }
            if criterion.observes_targets.is_empty() {
                return true;
            }
            target.is_some_and(|value| {
                criterion
                    .observes_targets
                    .iter()
                    .filter_map(|expected| bound_evidence_target(expected))
                    .any(|expected| expected == value)
            })
        })
        .cloned()
        .collect()
}

fn has_prior_effect(ledger: &[EvidenceRecord], target: Option<&str>) -> bool {
    ledger.iter().any(|record| {
        matches!(
            record.source,
            EvidenceSource::Tool | EvidenceSource::AgentTask | EvidenceSource::Verification
        ) && !record.stale
            && record.summary.contains("status=Ok")
            && (target.is_none() || record.target.is_none() || record.target.as_deref() == target)
    })
}

/// 実プロセス実行済みの shell_exec のみ Verification / plan 実行として認める。
/// collaborative handoff / 事前拒否は subprocess を走らせないため除外する。
/// `rejected_or_failed` は承認後の実行失敗（非 zero 等）を含み、plan 未実行とは区別する。
fn shell_process_was_executed(call: &ExecutedToolCall) -> bool {
    matches!(
        call.decision.as_deref(),
        Some("executed")
            | Some("auto_approved_session")
            | Some("auto_approved_pattern")
            | Some("rejected_or_failed")
    )
}

fn matching_delegated_command<'a>(
    contract: &'a TaskContract,
    call: &ExecutedToolCall,
) -> Option<&'a crate::domain::DelegatedVerificationPlanItem> {
    if call.name != SHELL_EXEC || !shell_process_was_executed(call) {
        return None;
    }
    let command = call.arguments.get("command")?.as_str()?;
    let args = call
        .arguments
        .get("args")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .map(|item| item.as_str().map(str::to_string))
                .collect::<Option<Vec<_>>>()
        })
        .unwrap_or_else(|| Some(Vec::new()))?;
    contract
        .delegated_verification
        .as_ref()?
        .items
        .iter()
        .find(|item| {
            matches!(
                &item.action,
                DelegatedVerificationAction::Command {
                    command: expected_command,
                    args: expected_args,
                    ..
                } if expected_command == command && expected_args == &args
            )
        })
}

pub fn verification_infrastructure_failure<'a>(
    validated_plan_calls: &'a [&'a ExecutedToolCall],
) -> Option<&'a ExecutedToolCall> {
    validated_plan_calls
        .iter()
        .copied()
        .find(|call| call.process_terminal == Some(ProcessExecutionTerminal::InfrastructureFailure))
}

pub fn build_interrupted_report(
    contract: Option<&TaskContract>,
    evidence: &[EvidenceRecord],
    evaluation: Option<&CompletionEvaluation>,
    terminal: WireVerificationTerminal,
    reason: &str,
    queries_used: u8,
) -> CompletionReport {
    debug_assert!(matches!(
        terminal,
        WireVerificationTerminal::Failed | WireVerificationTerminal::Cancelled
    ));
    let mut evidence_budget = COMPLETION_REPORT_MAX_EVIDENCE_TOTAL;
    let criteria = contract
        .map(|contract| {
            let mut criteria = contract
                .criteria
                .iter()
                .map(|criterion| {
                    let status =
                        interrupted_criterion_status(contract, &criterion.id, evidence, evaluation);
                    let limit = COMPLETION_REPORT_MAX_EVIDENCE_PER_CRITERION.min(evidence_budget);
                    let mut records = evidence
                        .iter()
                        .filter(|record| record.criterion_ids.contains(&criterion.id))
                        .collect::<Vec<_>>();
                    records.sort_by(|left, right| left.evidence_id.cmp(&right.evidence_id));
                    let projected = records
                        .into_iter()
                        .take(limit)
                        .map(to_wire_evidence)
                        .collect::<Vec<_>>();
                    evidence_budget = evidence_budget.saturating_sub(projected.len());
                    CompletionCriterionReport {
                        criterion_id: bounded(&criterion.id),
                        satisfied: status == WireCriterionStatus::Satisfied,
                        evidence: projected,
                        evaluation_status: Some(status),
                    }
                })
                .collect::<Vec<_>>();
            criteria.sort_by(|left, right| left.criterion_id.cmp(&right.criterion_id));
            criteria
        })
        .unwrap_or_default();
    let unsatisfied_criteria = criteria
        .iter()
        .filter(|criterion| {
            matches!(
                criterion.evaluation_status,
                Some(WireCriterionStatus::Unsatisfied | WireCriterionStatus::Unknown)
            )
        })
        .map(|criterion| criterion.criterion_id.clone())
        .collect();
    let marker = match terminal {
        WireVerificationTerminal::Failed => "verification infrastructure failed",
        WireVerificationTerminal::Cancelled => "task completion was cancelled",
        _ => unreachable!("terminal checked above"),
    };
    let mut unverified_items = evidence
        .iter()
        .filter(|record| !record.verified || record.stale)
        .map(|record| {
            bounded_report_text(&format!(
                "{}{}: {}",
                record.evidence_id,
                if record.stale { " (stale)" } else { "" },
                record.summary
            ))
        })
        .take(COMPLETION_REPORT_MAX_UNVERIFIED_ITEMS.saturating_sub(1))
        .collect::<Vec<_>>();
    unverified_items.push(marker.into());
    let mut report = CompletionReport {
        outcome: WireOutcome::Blocked,
        terminal_reason: Some(bounded_report_text(reason)),
        criteria,
        unsatisfied_criteria,
        unverified_items,
        queries_used,
        verification_terminal: Some(terminal),
        gaps: Vec::new(),
        worker_id: None,
        follow_up_count: contract
            .and_then(|contract| contract.delegated_verification.as_ref())
            .map(|_| queries_used.saturating_sub(1).min(1)),
    };
    if !shrink_report_to_serialized_bound(&mut report).unwrap_or(false) {
        for criterion in &mut report.criteria {
            criterion.evidence.clear();
        }
        report.unverified_items = vec![marker.into()];
    }
    report
}

fn interrupted_criterion_status(
    contract: &TaskContract,
    criterion_id: &str,
    evidence: &[EvidenceRecord],
    evaluation: Option<&CompletionEvaluation>,
) -> WireCriterionStatus {
    if let Some(status) = evaluation.and_then(|evaluation| {
        evaluation
            .criteria
            .iter()
            .find(|criterion| criterion.criterion_id == criterion_id)
            .map(|criterion| criterion.status)
    }) {
        return match status {
            CriterionStatus::Satisfied => WireCriterionStatus::Satisfied,
            CriterionStatus::Unsatisfied => WireCriterionStatus::Unsatisfied,
            CriterionStatus::Unknown => WireCriterionStatus::Unknown,
            CriterionStatus::NotApplicable => WireCriterionStatus::NotApplicable,
        };
    }
    if criterion_has_conflicting_observation_fingerprints(contract, criterion_id, evidence) {
        return WireCriterionStatus::Unknown;
    }
    let complete = if let Some(plan) = &contract.delegated_verification {
        let items = plan
            .items
            .iter()
            .filter(|item| item.criterion_ids.iter().any(|id| id == criterion_id))
            .collect::<Vec<_>>();
        !items.is_empty()
            && items.iter().all(|item| {
                evidence.iter().any(|record| {
                    record.criterion_ids.iter().any(|id| id == criterion_id)
                        && record.plan_item_id.as_deref() == Some(item.id.as_str())
                        && record.verified
                        && !record.stale
                })
            })
    } else {
        evidence.iter().any(|record| {
            record.criterion_ids.iter().any(|id| id == criterion_id)
                && record.verified
                && !record.stale
        })
    };
    if complete {
        return WireCriterionStatus::Satisfied;
    }
    WireCriterionStatus::Unknown
}

fn matching_delegated_observation<'a>(
    contract: &'a TaskContract,
    call: &ExecutedToolCall,
    target: Option<&str>,
) -> Option<&'a crate::domain::DelegatedVerificationPlanItem> {
    contract
        .delegated_verification
        .as_ref()?
        .items
        .iter()
        .find(|item| {
            matches!(
                &item.action,
                DelegatedVerificationAction::Observation {
                    tool,
                    target: expected_target,
                } if tool == &call.name
                    && bound_evidence_target(expected_target).as_deref() == target
            )
        })
}

fn call_matches_plan_action(call: &ExecutedToolCall, action: &DelegatedVerificationAction) -> bool {
    match action {
        DelegatedVerificationAction::Command { command, args, .. } => {
            let args_match = call
                .arguments
                .get("args")
                .and_then(|value| value.as_array())
                .is_some_and(|actual| {
                    actual
                        .iter()
                        .map(|value| value.as_str())
                        .eq(args.iter().map(|value| Some(value.as_str())))
                });
            // 起動・実行の有無と成功判定を分離する。非 zero は plan 実行済みの観測結果。
            call.name == SHELL_EXEC
                && shell_process_was_executed(call)
                && call
                    .arguments
                    .get("command")
                    .and_then(|value| value.as_str())
                    == Some(command.as_str())
                && args_match
        }
        DelegatedVerificationAction::Observation { tool, target } => {
            call.name == *tool
                && tool_target(call).as_deref() == bound_evidence_target(target).as_deref()
        }
    }
}

/// 一つのouter queryが Agent Task 後に固定plan全件を順序どおり 1:1 で実行したことを検査する。
/// 失敗した agent_task（UnknownWorker 修正再試行前、または成功後の premature 再呼出し tool error）は
/// 成功カウントに含めない。
pub fn validate_delegated_cycle_calls<'a>(
    contract: &TaskContract,
    calls: &'a [ExecutedToolCall],
) -> Result<Vec<&'a ExecutedToolCall>, String> {
    let Some(plan) = &contract.delegated_verification else {
        return Ok(Vec::new());
    };
    let agent_positions = calls
        .iter()
        .enumerate()
        .filter(|(_, call)| {
            call.name == aibe_protocol::AGENT_TASK && call.status == ExecutedToolStatus::Ok
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if agent_positions.len() != 1 {
        return Err(if agent_positions.is_empty() {
            "delegated verification query requires exactly one successful agent_task".into()
        } else {
            "delegated verification query permits only one successful agent_task".into()
        });
    }
    let first_ok_agent = agent_positions[0];
    for call in calls.iter().take(first_ok_agent) {
        if call.name == aibe_protocol::AGENT_TASK && call.status == ExecutedToolStatus::Error {
            continue;
        }
        return Err(format!(
            "delegated verification requires agent_task before other tools; found {} first",
            call.name
        ));
    }
    // 各plan itemに一致するcallはexactly oneとする。これにより、採用call以外の
    // duplicate InfrastructureFailureでFailedへ短絡することを防ぐ。
    let search_from = agent_positions[0] + 1;
    let mut command_positions = Vec::new();
    let mut observation_positions = Vec::new();
    let mut selected_positions = Vec::with_capacity(plan.items.len());
    let mut selected = Vec::with_capacity(plan.items.len());
    for item in &plan.items {
        let matches = calls
            .iter()
            .enumerate()
            .skip(search_from)
            .filter_map(|(index, call)| {
                call_matches_plan_action(call, &item.action).then_some((index, call))
            })
            .collect::<Vec<_>>();
        let [(position, call)] = matches.as_slice() else {
            return Err(if matches.is_empty() {
                format!(
                    "delegated verification plan item was not executed in order: {}",
                    item.id
                )
            } else {
                format!(
                    "delegated verification plan item was executed more than once: {}",
                    item.id
                )
            });
        };
        selected_positions.push(*position);
        selected.push(*call);
        match &item.action {
            DelegatedVerificationAction::Command { .. } => command_positions.push(*position),
            DelegatedVerificationAction::Observation { .. } => {
                observation_positions.push(*position)
            }
        }
    }
    if selected_positions
        .windows(2)
        .any(|positions| positions[0] >= positions[1])
    {
        return Err("delegated verification plan items were executed out of order".into());
    }
    if command_positions
        .iter()
        .max()
        .zip(observation_positions.iter().min())
        .is_some_and(|(last_command, first_observation)| last_command >= first_observation)
    {
        return Err("delegated verification commands must run before observations".into());
    }
    Ok(selected)
}

fn tool_target(call: &ExecutedToolCall) -> Option<String> {
    let args = &call.arguments;
    if let Some(path) = args
        .get("path")
        .or_else(|| args.get("file"))
        .or_else(|| args.get("file_path"))
        .and_then(|value| value.as_str())
    {
        return bound_evidence_target(path);
    }
    if call.name == aibe_protocol::AGENT_TASK {
        // Worker は cwd 配下へ広範囲に副作用し得る。個別ファイル digest と一致させず、
        // target=None（global/unknown）として後続 observation の prior effect に使う。
        return None;
    }
    if call.name == SHELL_EXEC {
        if let Some(path) = args
            .get("args")
            .and_then(|value| value.as_array())
            .and_then(|items| {
                items
                    .iter()
                    .rev()
                    .find_map(|item| item.as_str().filter(|text| text.contains('/')))
                    .or_else(|| items.iter().rev().find_map(|item| item.as_str()))
            })
        {
            return bound_evidence_target(path);
        }
        if let Some(command) = args.get("command").and_then(|value| value.as_str()) {
            return bound_evidence_target(command);
        }
    }
    None
}

pub fn deliverable_evidence(
    contract: &TaskContract,
    deliverable: &str,
    next_index: usize,
) -> Vec<EvidenceRecord> {
    if deliverable.trim().is_empty() {
        return Vec::new();
    }
    if contract.task_kind != TaskKind::Plan {
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
            target: None,
            stale: false,
            plan_item_id: None,
            value_fingerprint: Some(format!("sha256:{}", sha256_hex(deliverable.as_bytes()))),
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
    let satisfied = evaluation
        .criteria
        .iter()
        .filter(|criterion| criterion.status == CriterionStatus::Satisfied)
        .map(|criterion| {
            format!(
                "- {}: evidence {}",
                criterion.criterion_id,
                criterion.evidence_ids.join(",")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let evidence_lines = evidence
        .iter()
        .map(|record| {
            format!(
                "- {} criteria=[{}] source={:?} verified={} stale={}: {}",
                record.evidence_id,
                record.criterion_ids.join(","),
                record.source,
                record.verified,
                record.stale,
                bounded(&record.summary)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let stale_ids = evidence
        .iter()
        .filter(|record| record.stale)
        .map(|record| record.evidence_id.as_str())
        .collect::<Vec<_>>();
    let stale_section = if stale_ids.is_empty() {
        String::new()
    } else {
        format!(
            "\nStale evidence (never cite in evaluation): {}\n",
            stale_ids.join(", ")
        )
    };
    let citeable = evidence
        .iter()
        .filter(|record| {
            !record.stale
                && record.verified
                && matches!(
                    record.source,
                    EvidenceSource::Observation | EvidenceSource::Verification
                )
        })
        .map(|record| record.evidence_id.as_str())
        .collect::<Vec<_>>();
    let citeable_section = if citeable.is_empty() {
        String::new()
    } else {
        format!(
            "\nNon-stale verified evidence (only these may support satisfied now): {}\n",
            citeable.join(", ")
        )
    };
    format!(
        "[task completion continuation]\nFixed contract:\n{contract_json}\nSatisfied criteria:\n{satisfied}\nUnsatisfied criteria:\n{unsatisfied}\nExisting evidence:\n{evidence_lines}{stale_section}{citeable_section}Next evidence id: e{}\nNext objective: {}",
        evidence.len() + 1,
        bounded(evaluation.next_objective.as_deref().unwrap_or("resolve the stated gap"))
    )
}

pub fn build_delegated_continuation(
    contract: &TaskContract,
    evaluation: &CompletionEvaluation,
    evidence: &[EvidenceRecord],
    calls: &[ExecutedToolCall],
) -> Result<DelegatedContinuation, String> {
    let initial = calls
        .iter()
        .find(|call| {
            call.name == aibe_protocol::AGENT_TASK && call.status == ExecutedToolStatus::Ok
        })
        .ok_or_else(|| {
            "delegated continuation requires successful initial agent_task".to_string()
        })?;
    let initial: AgentTaskRequest = serde_json::from_value(initial.arguments.clone())
        .map_err(|error| format!("invalid initial agent_task request: {error}"))?;
    let observation_states = gap_observation_states(contract, evidence, calls, evaluation);
    let gap = build_gap(contract, evidence, evaluation, &observation_states)?;
    let follow_up = gap_follow_up_request(contract, &initial, &gap)?;
    let base = build_continuation(contract, evaluation, evidence);
    let request = serde_json::to_string(&follow_up)
        .map_err(|error| format!("cannot serialize follow-up request: {error}"))?;
    Ok(DelegatedContinuation {
        prompt: format!(
            "{base}\nDelegated Gap follow-up (query 2): execute exactly once with agent_task as the first tool, then rerun every delegated_verification plan item in order before any evaluation JSON. Evidence ids e1-e{} from query 1 become stale after follow-up agent_task — cite only new ids from e{} onward (fresh verified read_file/verification from this query). Do not call other tools before agent_task:\n{request}",
            evidence.len(),
            evidence.len() + 1,
        ),
        request: follow_up,
    })
}

pub struct DelegatedContinuation {
    pub prompt: String,
    pub request: AgentTaskRequest,
}

pub fn build_report(
    contract: &TaskContract,
    evidence: &[EvidenceRecord],
    evaluation: &CompletionEvaluation,
    queries_used: u8,
    stalled: bool,
) -> Result<CompletionReport, String> {
    build_report_with_calls(contract, evidence, &[], evaluation, queries_used, stalled)
}

pub fn build_report_with_calls(
    contract: &TaskContract,
    evidence: &[EvidenceRecord],
    calls: &[ExecutedToolCall],
    evaluation: &CompletionEvaluation,
    queries_used: u8,
    stalled: bool,
) -> Result<CompletionReport, String> {
    validate_evaluation(contract, evidence, evaluation)?;
    let outcome = terminal_outcome(evaluation, queries_used, stalled)
        .ok_or_else(|| "completion evaluation requires another query".to_string())?;
    let mut evidence_budget = COMPLETION_REPORT_MAX_EVIDENCE_TOTAL;
    let mut evidence_omitted = false;
    let mut ordered_evaluations = evaluation.criteria.iter().collect::<Vec<_>>();
    ordered_evaluations.sort_by(|left, right| left.criterion_id.cmp(&right.criterion_id));
    let criteria = ordered_evaluations
        .into_iter()
        .map(|criterion| {
            let criterion_limit = COMPLETION_REPORT_MAX_EVIDENCE_PER_CRITERION.min(evidence_budget);
            let mut records = criterion
                .evidence_ids
                .iter()
                .filter_map(|id| evidence.iter().find(|record| &record.evidence_id == id))
                .collect::<Vec<_>>();
            records.sort_by(|left, right| left.evidence_id.cmp(&right.evidence_id));
            evidence_omitted |= records.len() > criterion_limit;
            let criterion_evidence = records
                .into_iter()
                .take(criterion_limit)
                .map(to_wire_evidence)
                .collect::<Vec<_>>();
            evidence_budget = evidence_budget.saturating_sub(criterion_evidence.len());
            CompletionCriterionReport {
                criterion_id: bounded(&criterion.criterion_id),
                satisfied: criterion.status == CriterionStatus::Satisfied,
                evidence: criterion_evidence,
                // 四状態は常に投影する。非委譲では Unknown/NotApplicable を validation で拒否する。
                evaluation_status: Some(match criterion.status {
                    CriterionStatus::Satisfied => WireCriterionStatus::Satisfied,
                    CriterionStatus::Unsatisfied => WireCriterionStatus::Unsatisfied,
                    CriterionStatus::Unknown => WireCriterionStatus::Unknown,
                    CriterionStatus::NotApplicable => WireCriterionStatus::NotApplicable,
                }),
            }
        })
        .collect();
    let mut unsatisfied_criteria = evaluation
        .criteria
        .iter()
        .filter(|criterion| {
            matches!(
                criterion.status,
                CriterionStatus::Unsatisfied | CriterionStatus::Unknown
            )
        })
        .map(|criterion| bounded(&criterion.criterion_id))
        .collect::<Vec<_>>();
    unsatisfied_criteria.sort();
    let mut unverified_items: Vec<String> = evidence
        .iter()
        .filter(|record| !record.verified || record.stale)
        .map(|record| {
            bounded_report_text(&format!(
                "{}{}: {}",
                record.evidence_id,
                if record.stale { " (stale)" } else { "" },
                record.summary
            ))
        })
        .collect();
    unverified_items.extend(
        evaluation
            .criteria
            .iter()
            .filter(|criterion| {
                matches!(
                    criterion.status,
                    CriterionStatus::Unsatisfied | CriterionStatus::Unknown
                )
            })
            .flat_map(|criterion| {
                let status = match criterion.status {
                    CriterionStatus::Unknown => "unknown",
                    _ => "required evidence",
                };
                criterion.required_evidence.iter().map(move |required| {
                    bounded(&format!(
                        "{}: {status}: {}",
                        criterion.criterion_id, required
                    ))
                })
            }),
    );
    if evidence_omitted {
        unverified_items.push("additional criterion evidence omitted by report bound".into());
    }
    if unverified_items.len() > COMPLETION_REPORT_MAX_UNVERIFIED_ITEMS {
        unverified_items.truncate(COMPLETION_REPORT_MAX_UNVERIFIED_ITEMS - 1);
        unverified_items.push("additional unverified items omitted by report bound".into());
    }
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
        .map(bounded_report_text);
    let delegated = contract.delegated_verification.is_some();
    let mut report = CompletionReport {
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
        verification_terminal: delegated.then_some(match outcome {
            CompletionOutcome::Done => WireVerificationTerminal::Done,
            CompletionOutcome::NeedsUser => WireVerificationTerminal::NeedsUser,
            CompletionOutcome::Blocked if stalled => WireVerificationTerminal::Stagnated,
            CompletionOutcome::Blocked => WireVerificationTerminal::Blocked,
            CompletionOutcome::BudgetExhausted => WireVerificationTerminal::BudgetExhausted,
        }),
        gaps: Vec::new(),
        worker_id: None,
        follow_up_count: delegated.then_some(queries_used.saturating_sub(1).min(1)),
    };
    if delegated && outcome != CompletionOutcome::Done {
        attach_gap_report_with_calls(&mut report, contract, evidence, calls, evaluation);
    }
    if !shrink_report_to_serialized_bound(&mut report)? {
        return Err("completion report exceeds byte bound".into());
    }
    Ok(report)
}

fn shrink_report_to_serialized_bound(report: &mut CompletionReport) -> Result<bool, String> {
    const OMISSION: &str = "additional report details omitted by serialized byte bound";
    let serialized_len = |report: &CompletionReport| {
        serde_json::to_vec(report)
            .map(|bytes| bytes.len())
            .map_err(|error| format!("cannot serialize completion report: {error}"))
    };
    if serialized_len(report)? <= COMPLETION_REPORT_MAX_BYTES {
        return Ok(true);
    }
    if !report.unverified_items.iter().any(|item| item == OMISSION) {
        if report.unverified_items.len() >= COMPLETION_REPORT_MAX_UNVERIFIED_ITEMS {
            report.unverified_items.pop();
        }
        report.unverified_items.push(OMISSION.into());
    }
    loop {
        if serialized_len(report)? <= COMPLETION_REPORT_MAX_BYTES {
            return Ok(true);
        }
        if let Some(criterion) = report
            .criteria
            .iter_mut()
            .rev()
            .find(|criterion| !criterion.evidence.is_empty())
        {
            criterion.evidence.pop();
            continue;
        }
        if report.unverified_items.len() > 1 {
            let omission_index = report
                .unverified_items
                .iter()
                .position(|item| item == OMISSION)
                .unwrap_or(report.unverified_items.len() - 1);
            let remove_index = if omission_index == 0 { 1 } else { 0 };
            report.unverified_items.remove(remove_index);
            continue;
        }
        // Gap は criterion row 本体ではなく bounded な監査 summary である。長い
        // verification plan ID が多数 criterion に対応すると、合法な Contract でも
        // report 内で参照が多重化するため、安定 ID 順の末尾から summary を省略する。
        // criterion row と omission marker は常に残す。
        if !report.gaps.is_empty() {
            report.gaps.pop();
            continue;
        }
        return Ok(false);
    }
}

pub fn attach_gap_report(
    report: &mut CompletionReport,
    contract: &TaskContract,
    evidence: &[EvidenceRecord],
    evaluation: &CompletionEvaluation,
) {
    attach_gap_report_with_calls(report, contract, evidence, &[], evaluation);
}

pub fn attach_gap_report_with_calls(
    report: &mut CompletionReport,
    contract: &TaskContract,
    evidence: &[EvidenceRecord],
    calls: &[ExecutedToolCall],
    evaluation: &CompletionEvaluation,
) {
    let observation_states = gap_observation_states(contract, evidence, calls, evaluation);
    let Ok(gap) = build_gap(contract, evidence, evaluation, &observation_states) else {
        return;
    };
    report.gaps = gap
        .entries
        .into_iter()
        .map(|entry| CompletionGapReport {
            criterion_id: bounded(&entry.criterion_id),
            observed: bounded_report_text(&entry.observed),
            required_work: bounded_report_text(&entry.required_work),
            verification_plan_item_ids: entry
                .verification_plan_item_ids
                .iter()
                .map(|item| bounded(item))
                .collect(),
        })
        .collect();
    let bounded = shrink_report_to_serialized_bound(report).unwrap_or(false);
    if !bounded {
        // terminal +全criterion rowの最小projectionはContract validation上bounded。
        // serializer異常時にも追加Gapだけは残さず、呼出し側へunbounded reportを渡さない。
        report.gaps.clear();
    }
    debug_assert!(
        serde_json::to_vec(report).is_ok_and(|bytes| bytes.len() <= COMPLETION_REPORT_MAX_BYTES)
    );
}

fn gap_observation_states(
    contract: &TaskContract,
    evidence: &[EvidenceRecord],
    calls: &[ExecutedToolCall],
    evaluation: &CompletionEvaluation,
) -> Vec<CriterionObservationState> {
    let worker_observation_incomplete = calls.iter().any(|call| {
        call.name == aibe_protocol::AGENT_TASK
            && call
                .output
                .as_deref()
                .and_then(|output| serde_json::from_str::<AgentTaskResult>(output).ok())
                .is_some_and(|result| result.observation_incomplete)
    });
    evaluation
        .criteria
        .iter()
        .filter(|criterion| {
            criterion.evidence_ids.is_empty()
                && matches!(
                    criterion.status,
                    CriterionStatus::Unsatisfied | CriterionStatus::Unknown
                )
        })
        .map(|criterion| {
            let plan_items = contract
                .delegated_verification
                .as_ref()
                .map(|plan| {
                    plan.items
                        .iter()
                        .filter(|item| item.criterion_ids.contains(&criterion.criterion_id))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let plan_not_executed = plan_items.iter().any(|item| {
                !calls
                    .iter()
                    .any(|call| call_matches_plan_action(call, &item.action))
            });
            let observation_incomplete = worker_observation_incomplete
                || plan_items.iter().any(|item| {
                    calls.iter().any(|call| {
                        call_matches_plan_action(call, &item.action)
                            && call.process_terminal
                                == Some(ProcessExecutionTerminal::ObservableTimeout)
                    })
                });
            let relevant = evidence
                .iter()
                .filter(|record| record.criterion_ids.contains(&criterion.criterion_id))
                .collect::<Vec<_>>();
            let evidence_conflict = criterion_has_conflicting_observation_fingerprints(
                contract,
                &criterion.criterion_id,
                evidence,
            ) || relevant.iter().any(|verified| {
                verified.verified
                    && !verified.stale
                    && relevant.iter().any(|other| {
                        (!other.verified || other.stale)
                            && ((verified.plan_item_id.is_some()
                                && verified.plan_item_id == other.plan_item_id)
                                || (verified.target.is_some()
                                    && verified.target == other.target
                                    && verified.value_fingerprint.is_some()
                                    && other.value_fingerprint.is_some()
                                    && verified.value_fingerprint != other.value_fingerprint))
                    })
            });
            CriterionObservationState {
                criterion_id: criterion.criterion_id.clone(),
                kind: if plan_not_executed {
                    GapObservationKind::PlanNotExecuted
                } else if observation_incomplete {
                    GapObservationKind::ObservationIncomplete
                } else if evidence_conflict {
                    GapObservationKind::EvidenceConflict
                } else {
                    GapObservationKind::NoObservation
                },
            }
        })
        .collect()
}

fn to_wire_evidence(record: &EvidenceRecord) -> CompletionEvidenceReport {
    CompletionEvidenceReport {
        evidence_id: record.evidence_id.clone(),
        source: match record.source {
            EvidenceSource::Tool => CompletionEvidenceSource::Tool,
            EvidenceSource::UnknownShellEffect => CompletionEvidenceSource::UnknownShellEffect,
            EvidenceSource::Observation => CompletionEvidenceSource::Observation,
            EvidenceSource::Verification => CompletionEvidenceSource::Verification,
            EvidenceSource::Deliverable => CompletionEvidenceSource::Deliverable,
            EvidenceSource::AgentTask => CompletionEvidenceSource::AgentTask,
        },
        summary: bounded_report_text(&record.summary),
        verified: record.verified && !record.stale,
    }
}

fn bounded(value: &str) -> String {
    aish_replay::sanitize_log_text(value)
        .chars()
        .take(SUMMARY_LIMIT)
        .collect()
}

fn bounded_report_text(value: &str) -> String {
    let sanitized = aish_replay::sanitize_log_text(value);
    if sanitized.len() <= COMPLETION_REPORT_TEXT_MAX_BYTES {
        return sanitized;
    }
    let mut end = COMPLETION_REPORT_TEXT_MAX_BYTES;
    while !sanitized.is_char_boundary(end) {
        end -= 1;
    }
    sanitized[..end].to_string()
}

/// provider delta を最終 envelope 判定まで保持し、control JSON を client へ流さない。
pub const COMPLETION_EVENT_BUFFER_MAX_BYTES: usize = 64 * 1024;
pub const COMPLETION_EVENT_BUFFER_MAX_DELTAS: usize = 1024;

pub struct CompletionEventBuffer {
    inner: Arc<dyn TurnEventSink>,
    state: Mutex<CompletionEventBufferState>,
}

#[derive(Default)]
struct CompletionEventBufferState {
    deltas: Vec<String>,
    buffered_bytes: usize,
    delta_count: usize,
    overflowed: bool,
}

impl CompletionEventBuffer {
    pub fn new(inner: Arc<dyn TurnEventSink>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            state: Mutex::new(CompletionEventBufferState::default()),
        })
    }

    pub async fn buffered_usage(&self) -> (usize, usize, bool) {
        let state = self.state.lock().await;
        (state.buffered_bytes, state.delta_count, state.overflowed)
    }

    pub async fn flush_for_response(&self, id: &str, response: &aibe_protocol::ClientResponse) {
        let buffered = {
            let mut state = self.state.lock().await;
            let deltas = std::mem::take(&mut state.deltas);
            let overflowed = state.overflowed;
            *state = CompletionEventBufferState::default();
            (!overflowed).then_some(deltas)
        };
        match response {
            aibe_protocol::ClientResponse::AgentTurnResult {
                assistant_message,
                completion_report: Some(_),
                ..
            } => {
                let deliverable = aish_replay::sanitize_log_text(&assistant_message.content);
                if !deliverable.is_empty()
                    && !deliverable
                        .contains(crate::application::completion_envelope::TASK_COMPLETION_MARKER)
                {
                    self.inner.assistant_streaming(id, deliverable).await;
                }
            }
            aibe_protocol::ClientResponse::AgentTurnResult {
                status: aibe_protocol::AgentTurnStatus::Suspended,
                ..
            } => {
                if let Some(deltas) = buffered {
                    let contains_control = deltas
                        .join("")
                        .contains(crate::application::completion_envelope::TASK_COMPLETION_MARKER);
                    if !contains_control {
                        for delta in deltas {
                            self.inner.assistant_streaming(id, delta).await;
                        }
                    }
                }
            }
            _ => {
                // Active request の fail-closed 応答では、検査前に生成された assistant
                // 本文を表示経路へ流さない。
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
        let mut state = self.state.lock().await;
        if state.overflowed {
            return;
        }
        let next_bytes = state.buffered_bytes.saturating_add(delta.len());
        let next_count = state.delta_count.saturating_add(1);
        if next_bytes > COMPLETION_EVENT_BUFFER_MAX_BYTES
            || next_count > COMPLETION_EVENT_BUFFER_MAX_DELTAS
        {
            state.deltas.clear();
            state.buffered_bytes = 0;
            state.delta_count = 0;
            state.overflowed = true;
            return;
        }
        state.buffered_bytes = next_bytes;
        state.delta_count = next_count;
        state.deltas.push(delta);
    }

    async fn final_response(&self, _id: &str) {}
}
