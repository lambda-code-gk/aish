#!/usr/bin/env bash
# Codex 統合スクリプトと設定の退行を静的に検査する。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

scripts=(
  scripts/codex-sandbox-backend.sh
  scripts/codex-cli.sh
  scripts/codex-mcp-wrapper.sh
  scripts/codex-mcp-prompt.sh
  scripts/codex-fix-linux-sandbox.sh
)

for script in "${scripts[@]}"; do
  bash -n "$script"
done

if rg -n 'sandbox linux|sandbox_mode=danger-full-access' \
  scripts/codex-cli.sh scripts/codex-mcp-wrapper.sh \
  scripts/codex-sandbox-backend.sh .codex/config.toml; then
  echo "Codex tooling error: obsolete or unrestricted sandbox setting found" >&2
  exit 1
fi

if rg -n '^\[profiles\.' .codex/config.toml; then
  echo "Codex tooling error: project-local profiles are unsupported" >&2
  exit 1
fi

rg -q "sandbox_mode=workspace-write" scripts/codex-mcp-wrapper.sh
rg -q "sandbox_workspace_write.network_access=false" scripts/codex-mcp-wrapper.sh
rg -q "umask 077" scripts/codex-cli.sh
rg -q "umask 077" scripts/codex-mcp-wrapper.sh
rg -q "doctor --summary" scripts/codex-fix-linux-sandbox.sh

echo "check-codex-tooling: ok"
