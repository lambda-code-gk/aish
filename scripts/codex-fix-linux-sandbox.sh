#!/usr/bin/env bash
# Linux で Codex の bwrap サンドボックスが失敗するか診断し、対処法を表示する。
# 正本: docs/codex-delegation.md § MCP / bwrap
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "== Codex Linux sandbox 診断 =="
echo "codex: $(command -v codex || echo 'not found')"
codex --version 2>/dev/null || true
echo "bwrap: $(command -v bwrap || echo 'not found')"
if [[ -r /proc/sys/kernel/apparmor_restrict_unprivileged_userns ]]; then
  echo "kernel.apparmor_restrict_unprivileged_userns=$(cat /proc/sys/kernel/apparmor_restrict_unprivileged_userns)"
fi
echo

echo "== bwrap バックエンド（既定） =="
if codex sandbox linux -- /bin/pwd 2>&1; then
  echo "OK: bwrap sandbox"
  BWRAP_OK=1
else
  echo "FAIL: bwrap sandbox（Ubuntu 24.04 では AppArmor 制限でよくある）"
  BWRAP_OK=0
fi
echo

echo "== Landlock バックエンド（use_legacy_landlock） =="
if codex --enable use_legacy_landlock sandbox linux -- /bin/pwd 2>&1; then
  echo "OK: landlock sandbox"
  LANDLOCK_OK=1
else
  echo "FAIL: landlock sandbox"
  LANDLOCK_OK=0
fi
echo

echo "== 経路別設定 =="
if grep -q codex-mcp-wrapper.sh .cursor/mcp.json 2>/dev/null; then
  echo "OK: MCP → scripts/codex-mcp-wrapper.sh"
else
  echo "要対応: .cursor/mcp.json を docs/codex-delegation.md 参照"
fi
if [[ -x scripts/codex-cli.sh ]]; then
  echo "OK: CLI → scripts/codex-cli.sh"
else
  echo "要対応: scripts/codex-cli.sh"
fi
echo

if [[ "$BWRAP_OK" == "1" ]]; then
  echo "bwrap は動作しています。追加作業は不要です。"
  exit 0
fi

if [[ "$LANDLOCK_OK" == "1" ]]; then
  echo "推奨（CLI）: .codex/config.toml に [features] use_legacy_landlock = true"
  echo "Cursor MCP: scripts/codex-mcp-wrapper.sh（Landlock は MCP と併用不可のため danger-full-access）"
  if grep -q codex-mcp-wrapper.sh .cursor/mcp.json 2>/dev/null; then
    echo "OK: .cursor/mcp.json は wrapper 経由"
  else
    echo "要対応: .cursor/mcp.json を docs/codex-delegation.md 参照"
  fi
  exit 0
fi

echo "Landlock も失敗しました。以下を確認してください:"
echo "  - codex / bubblewrap のバージョン"
echo "  - AppArmor プロファイル（要 sudo）:"
echo "      sudo apt install -y apparmor-profiles apparmor-utils bubblewrap"
echo "      sudo install -m 0644 /usr/share/apparmor/extra-profiles/bwrap-userns-restrict /etc/apparmor.d/bwrap-userns-restrict 2>/dev/null || true"
echo "      sudo apparmor_parser -r /etc/apparmor.d/bwrap-userns-restrict 2>/dev/null || true"
echo "  - 最終手段（セキュリティ低下）: sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0"
exit 1
