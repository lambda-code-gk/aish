#!/usr/bin/env bash
# Codex MCP の prompt 用テキストを stdout に出す。
#
# 既定: サブエージェント向け短いヘッダのみ（タスク本文は親が prompt に続けて渡す）。
# オプション: CODEX_USE_PACKET=1 で codex-context.sh のパケットを同梱（親がコンテキストを絞りたいとき）。
#
# 使い方:
#   ./scripts/codex-mcp-prompt.sh
#   あなたのタスク説明…
#   → 上記を連結して MCP codex の prompt に渡す
#
#   CODEX_USE_PACKET=1 CODEX_TASK=review ./scripts/codex-mcp-prompt.sh
#
# 権限: scripts/codex-mcp-wrapper.sh が workspace-write + network off に固定する。
set -euo pipefail

TASK="${CODEX_TASK:-subagent}"
EXTRA_ROOTS="${CODEX_EXTRA_ROOTS:-}"

cat <<EOF
Role: ${TASK} for aish workspace (Codex subagent).
Follow the repository AGENTS.md and the task body that follows.
Work only inside the sandbox writable roots; the repository root is the default writable root.
EOF

if [[ -n "$EXTRA_ROOTS" ]]; then
  echo
  echo "## 追加許可パス（このターン）"
  echo "$EXTRA_ROOTS" | tr ',' '\n'
fi

if [[ "${CODEX_USE_PACKET:-0}" == "1" ]]; then
  echo
  echo "---"
  echo
  "$(dirname "${BASH_SOURCE[0]}")/codex-context.sh"
fi
