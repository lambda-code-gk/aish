#!/usr/bin/env bash
# Cursor MCP 用 Codex 起動ラッパー。
# 認証は ~/.codex（codex login）を使う。サンドボックスだけ danger-full-access + Landlock オフ。
set -euo pipefail
CODEX="${CODEX:-$(command -v codex)}"
# 隔離用 .codex-mcp は認証を持たないため CODEX_HOME は上書きしない
exec "$CODEX" mcp-server \
  --disable use_legacy_landlock \
  -c 'approval_policy=never' \
  -c 'sandbox_mode=danger-full-access' \
  "$@"
