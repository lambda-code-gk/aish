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

STATIC_TARGETS=(
  ai/src/application/human_handoff.rs
  ai/src/adapters/outbound/human_handoff.rs
  ai/src/ports/outbound/human_handoff.rs
  ai/src/domain/human_handoff.rs
  ai/src/main.rs
  aish/src/human_shell.rs
  aibe-protocol/src/collaborative_handoff.rs
  aibe-protocol/src/request.rs
  aibe-client/src/transport.rs
  aibe/src/adapters/outbound/tools/shell_exec.rs
  aish/src/adapters/outbound/pty_shell.rs
  aish/src/adapters/outbound/shell_completion.rs
  ai/tests/0055_minimal_human_handoff.rs
  ai/tests/0055_collaborative_handoff_vertical_e2e.rs
  aish/tests/0055_minimal_human_handoff.rs
)

fail=0
scan_file() {
  local path="$1"
  [[ -f "$path" ]] || return 0
  for token in "${FORBIDDEN_TYPES[@]}"; do
    if rg -n "$token" "$path" >/dev/null 2>&1; then
      echo "forbidden type '$token' in $path" >&2
      fail=1
    fi
  done
  if [[ "$path" == *"/tests/"* ]]; then
    return 0
  fi
  # 0063 Human Task durable checkpoint is intentional and outside 0055 scope.
  case "$path" in
    *human_task*|*/0063_*)
      return 0
      ;;
  esac
  for file in "${FORBIDDEN_FILES[@]}"; do
    if rg -n "$file" "$path" >/dev/null 2>&1; then
      echo "forbidden file reference '$file' in $path" >&2
      fail=1
    fi
  done
}

for path in "${STATIC_TARGETS[@]}"; do
  scan_file "$path"
done

if git rev-parse --verify main >/dev/null 2>&1; then
  while IFS= read -r path; do
    case "$path" in
      ai/*|aish/*|aibe/*|aibe-client/*|aibe-protocol/*)
        scan_file "$path"
        ;;
    esac
  done < <(git diff --name-only main...HEAD 2>/dev/null || true)
fi

if [[ "$fail" -ne 0 ]]; then
  exit 1
fi

echo "check-minimal-human-handoff-scope: ok"
