#!/usr/bin/env bash
# 0055 §33: collaborative durable workflow の書き込み境界を静的検査する。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

fail() {
  echo "COLLABORATIVE WORKFLOW FAIL: $*" >&2
  exit 1
}

required=(
  ai/src/domain/collaborative_workflow.rs
  ai/src/adapters/outbound/collaborative_workflow_store.rs
  ai/src/application/collaborative_workflow_reconciler.rs
  ai/tests/0055_collaborative_workflow.rs
)
for path in "${required[@]}"; do
  [[ -f "$path" ]] || fail "missing $path"
done

if rg -n '\.(save_handoff|save_checkpoint)\(' ai/src/application; then
  fail "application must mutate CollaborativeWorkflow instead of saving split entities"
fi

if rg -n 'token_plaintext' \
  ai/src/domain/collaborative_workflow.rs \
  ai/src/adapters/outbound/collaborative_workflow_store.rs \
  ai/src/application/collaborative_workflow_reconciler.rs; then
  fail "workflow aggregate/effect/reconciler must not persist token plaintext"
fi

rg -q 'workflow.json' ai/src/adapters/outbound/collaborative_workflow_store.rs \
  || fail "workflow store must use workflow.json as its durable root"
rg -q 'compare_and_swap_workflow' ai/src/adapters/outbound/collaborative_workflow_store.rs \
  || fail "workflow store must implement revision CAS"

echo "COLLABORATIVE WORKFLOW: all checks passed"
