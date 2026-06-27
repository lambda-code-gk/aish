#!/usr/bin/env bash
# Cursor MCP 用 Codex 起動ラッパー。
# 認証は ~/.codex（codex login）を使う。sandbox は workspace-write から広げない。
set -euo pipefail
umask 077
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CODEX="${CODEX:-$(command -v codex)}"
# 隔離用 .codex-mcp は認証を持たないため CODEX_HOME は上書きしない。
source "$ROOT/scripts/codex-sandbox-backend.sh"
codex_select_linux_sandbox "$CODEX"
exec "$CODEX" "${CODEX_SANDBOX_ARGS[@]}" mcp-server \
  -c 'approval_policy=never' \
  -c 'sandbox_mode=workspace-write' \
  -c 'sandbox_workspace_write.network_access=false' \
  "$@"
