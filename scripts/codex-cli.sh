#!/usr/bin/env bash
# 手元 CLI 用。Ubuntu 24.04 の bwrap EPERM 回避で Landlock を有効化。
# MCP は codex-mcp-wrapper.sh を使うこと。
set -euo pipefail
CODEX="${CODEX:-$(command -v codex)}"
# workspace-write は permission profiles と Landlock が併用不可なため、Landlock のみ有効化。
exec "$CODEX" --enable use_legacy_landlock "$@"
