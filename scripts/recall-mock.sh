#!/usr/bin/env bash
# recall 専用 mock 導通: cache 保存と `ai complete` の hook trailer を非対話で確認する。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

AI_BIN="${ROOT}/target/debug/ai"
if [[ ! -x "$AI_BIN" ]]; then
  cargo build -q -p ai
fi

HOME_DIR="$(mktemp -d)"
trap 'rm -rf "$HOME_DIR"' EXIT

export HOME="$HOME_DIR"
mkdir -p "$HOME_DIR/.local/share/ai/suggestions"

CACHE="$HOME_DIR/.local/share/ai/suggestions/test-session.json"
export AI_SUGGESTION_CACHE="$CACHE"
export AI_SESSION_ID=test-session

python3 - "$CACHE" <<'PY'
import json, sys
path = sys.argv[1]
doc = {
    "schema_version": 1,
    "ai_session_id": "test-session",
    "conversation_id": None,
    "shell": "bash",
    "updated_at": "1",
    "active_queue_index": 0,
    "active_candidate_index": 0,
    "queues": [{
        "turn_id": "t1",
        "captured_at": "1",
        "candidates": [{"text": "git status", "language": "bash", "bytes": 10}],
    }],
}
with open(path, "w", encoding="utf-8") as f:
    json.dump(doc, f)
PY

OUT="$("$AI_BIN" recall next 2>/dev/null || true)"
if [[ "$OUT" != "git status" ]]; then
  echo "recall-mock: expected 'git status', got '${OUT:-<empty>}'" >&2
  exit 1
fi

BASH_COMPLETE="$("$AI_BIN" complete bash)"
case "$BASH_COMPLETE" in
  *'_ai_recall_next'*) ;;
  *)
    echo "recall-mock: bash complete output missing _ai_recall_next hook" >&2
    exit 1
    ;;
esac
case "$BASH_COMPLETE" in
  *'AI_SUGGESTION_CACHE'*) ;;
  *)
    echo "recall-mock: bash complete output missing AI_SUGGESTION_CACHE export" >&2
    exit 1
    ;;
esac

echo "recall-mock: ok"
