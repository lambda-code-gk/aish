#!/usr/bin/env bash
# 0055 minimal human handoff のスコープ漏れを静的検査する。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

FORBIDDEN_TYPES=(
  SideAgent
  SideConversation
  RequestHumanAction
  ChildGoal
  PendingWorkflowEffect
  CollaborativeWorkflow
  WorkflowReconciler
  HandoffLease
  SideRunLock
  Orphaned
  ResumeReturnedParent
)

FORBIDDEN_FILES=(
  workflow.json
  handoff.json
  checkpoint.json
  lease.json
  side-run-lock.json
  candidates.jsonl
  shell_sessions.jsonl
)

TARGETS=(
  ai/src/application/human_handoff.rs
  ai/src/adapters/outbound/human_handoff.rs
  ai/src/ports/outbound/human_handoff.rs
  ai/src/domain/human_handoff.rs
  aish/src/human_shell.rs
  aibe-protocol/src/collaborative_handoff.rs
)

fail=0
for path in "${TARGETS[@]}"; do
  for token in "${FORBIDDEN_TYPES[@]}"; do
    if rg -n "$token" "$path" >/dev/null 2>&1; then
      echo "forbidden type '$token' in $path" >&2
      fail=1
    fi
  done
  for file in "${FORBIDDEN_FILES[@]}"; do
    if rg -n "$file" "$path" >/dev/null 2>&1; then
      echo "forbidden file reference '$file' in $path" >&2
      fail=1
    fi
  done
done

if [[ "$fail" -ne 0 ]]; then
  exit 1
fi

echo "check-minimal-human-handoff-scope: ok"
