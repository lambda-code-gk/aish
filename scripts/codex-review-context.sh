#!/usr/bin/env bash
# 後方互換: 実装後レビュー用パケット（codex-context.sh の review に委譲）
export CODEX_TASK="${CODEX_TASK:-review}"
exec "$(dirname "${BASH_SOURCE[0]}")/codex-context.sh" "$@"
