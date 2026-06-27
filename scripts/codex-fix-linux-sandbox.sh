#!/usr/bin/env bash
# Linux で Codex の sandbox、設定、MCP 経路を診断し、対処法を表示する。
# 正本: docs/codex-delegation.md § Linux sandbox
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
CODEX="${CODEX:-$(command -v codex)}"

echo "== Codex Linux sandbox 診断 =="
echo "codex: $CODEX"
"$CODEX" --version 2>/dev/null || true
echo "bwrap: $(command -v bwrap || echo 'not found')"
if [[ -r /proc/sys/kernel/apparmor_restrict_unprivileged_userns ]]; then
  echo "kernel.apparmor_restrict_unprivileged_userns=$(cat /proc/sys/kernel/apparmor_restrict_unprivileged_userns)"
fi
echo

echo "== codex doctor =="
if command -v timeout >/dev/null 2>&1; then
  timeout 30 "$CODEX" doctor --summary --no-color --ascii ||
    echo "WARN: codex doctor failed or timed out"
else
  "$CODEX" doctor --summary --no-color --ascii || true
fi
echo

echo "== bwrap バックエンド（既定） =="
if "$CODEX" sandbox -- /bin/pwd 2>&1; then
  echo "OK: bwrap sandbox"
  BWRAP_OK=1
else
  echo "FAIL: bwrap sandbox（Ubuntu 24.04 では AppArmor 制限でよくある）"
  BWRAP_OK=0
fi
echo

echo "== Landlock バックエンド（use_legacy_landlock） =="
if "$CODEX" sandbox --enable use_legacy_landlock -- /bin/pwd 2>&1; then
  echo "OK: landlock sandbox"
  LANDLOCK_OK=1
else
  echo "FAIL: landlock sandbox"
  LANDLOCK_OK=0
fi
echo

echo "== 経路別設定 =="
PATHS_OK=1
if grep -q codex-mcp-wrapper.sh .cursor/mcp.json 2>/dev/null; then
  echo "OK: MCP → scripts/codex-mcp-wrapper.sh"
else
  echo "要対応: .cursor/mcp.json を docs/codex-delegation.md 参照"
  PATHS_OK=0
fi
if [[ -x scripts/codex-cli.sh ]]; then
  echo "OK: CLI → scripts/codex-cli.sh"
else
  echo "要対応: scripts/codex-cli.sh"
  PATHS_OK=0
fi
if grep -q "sandbox_mode=workspace-write" scripts/codex-mcp-wrapper.sh &&
  ! grep -q "sandbox_mode=danger-full-access" scripts/codex-mcp-wrapper.sh; then
  echo "OK: MCP sandbox → workspace-write"
else
  echo "要対応: MCP sandbox を workspace-write に固定"
  PATHS_OK=0
fi
echo

if [[ "$BWRAP_OK" == "1" ]]; then
  echo "bwrap は動作しています。CLI / MCP は bwrap を使用します。"
  [[ "$PATHS_OK" == "1" ]]
  exit
fi

if [[ "$LANDLOCK_OK" == "1" ]]; then
  echo "Landlock フォールバックで動作しますが、Codex では非推奨です。"
  echo "bwrap 用 AppArmor profile を有効化してください:"
  echo "  sudo install -m 0644 /usr/share/apparmor/extra-profiles/bwrap-userns-restrict /etc/apparmor.d/bwrap-userns-restrict"
  echo "  sudo apparmor_parser -r /etc/apparmor.d/bwrap-userns-restrict"
  [[ "$PATHS_OK" == "1" ]]
  exit
fi

echo "Landlock も失敗しました。以下を確認してください:"
echo "  - codex / bubblewrap のバージョン"
echo "  - AppArmor プロファイル（要 sudo）"
echo "      sudo apt-get install -y apparmor-profiles apparmor-utils bubblewrap"
echo "      sudo install -m 0644 /usr/share/apparmor/extra-profiles/bwrap-userns-restrict /etc/apparmor.d/bwrap-userns-restrict"
echo "      sudo apparmor_parser -r /etc/apparmor.d/bwrap-userns-restrict"
exit 1
