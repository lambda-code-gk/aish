#!/usr/bin/env bash
# 手元 CLI 用。bwrap を優先し、利用不能な環境だけ Landlock へフォールバックする。
# MCP は codex-mcp-wrapper.sh を使うこと。
set -euo pipefail
umask 077
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CODEX="${CODEX:-$(command -v codex)}"
source "$ROOT/scripts/codex-sandbox-backend.sh"
codex_select_linux_sandbox "$CODEX"
exec "$CODEX" "${CODEX_SANDBOX_ARGS[@]}" "$@"
