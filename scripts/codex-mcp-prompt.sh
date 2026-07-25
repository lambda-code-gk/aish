#!/usr/bin/env bash
# Codex MCP の prompt 用テキストを stdout に出す。
#
# 既定: サブエージェント向け短いヘッダのみ（タスク本文は親が prompt に続けて渡す）。
# review / audit: 独立レビュー方針 + codex-context.sh のパケットを既定で同梱。
# その他: CODEX_USE_PACKET=1 のときだけパケットを同梱。
#
# 使い方:
#   ./scripts/codex-mcp-prompt.sh
#   あなたのタスク説明…
#   → 上記を連結して MCP codex の prompt に渡す
#
#   CODEX_TASK=review ./scripts/codex-mcp-prompt.sh
#
# 推奨 MCP config（親が config に渡す）:
#   CODEX_TASK=review|spike → model_reasoning_effort=low
#   それ以外 → medium
#   ヒントだけ見る: CODEX_PRINT_CONFIG_HINT=1 ./scripts/codex-mcp-prompt.sh
#
# 権限: scripts/codex-mcp-wrapper.sh が workspace-write + network off に固定する。
set -euo pipefail

TASK="${CODEX_TASK:-subagent}"
EXTRA_ROOTS="${CODEX_EXTRA_ROOTS:-}"
PACKET="${CODEX_USE_PACKET:-}"

case "$TASK" in
  review|spike) EFFORT=low ;;
  *) EFFORT=medium ;;
esac

if [[ -z "$PACKET" ]]; then
  case "$TASK" in
    review|audit) PACKET=1 ;;
    *) PACKET=0 ;;
  esac
fi

if [[ "${CODEX_PRINT_CONFIG_HINT:-0}" == "1" ]]; then
  cat <<EOF
# recommended MCP config for CODEX_TASK=${TASK}
{"approval_policy":"never","model_reasoning_effort":"${EFFORT}"}
EOF
  case "$TASK" in
    review|audit)
      echo "# start a fresh codex thread; do not continue an implementation or prior review thread"
      echo "# packet default: ${PACKET}"
      ;;
    *)
      echo "# continue the same design/implementation task with codex-reply + threadId"
      echo "# packet default: ${PACKET}"
      ;;
  esac
  exit 0
fi

cat <<EOF
Role: ${TASK} for aish workspace (Codex subagent).
Follow the repository AGENTS.md and the task body that follows.
Work only inside the sandbox writable roots; the repository root is the default writable root.
EOF

case "$TASK" in
  review|audit)
    cat <<'EOF'

## Independent review policy
- This invocation must use a fresh thread; do not inherit an implementation or prior review thread.
- Findings only: do not edit files, run tests, or run verify.sh.
- Return 重大 / 中 / 軽微 with classification tag, file, evidence, and recommended fix.
- Pass when 重大 and 中 are both zero.
EOF
    ;;
esac

if [[ -n "$EXTRA_ROOTS" ]]; then
  echo
  echo "## 追加許可パス（このターン）"
  echo "$EXTRA_ROOTS" | tr ',' '\n'
fi

if [[ "$PACKET" == "1" ]]; then
  echo
  echo "---"
  echo
  "$(dirname "${BASH_SOURCE[0]}")/codex-context.sh"
fi
