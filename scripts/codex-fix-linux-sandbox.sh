#!/usr/bin/env bash
# Linux で Codex の sandbox、設定、MCP 経路を診断し、対処法を表示する。
# 正本: docs/codex-delegation.md § Linux sandbox
#
# 注意: Cursor エージェントのシェル sandbox 内で実行すると、UID remap /
# ローカル HTTP_PROXY / ネットワーク制限により、所有権・bwrap・WebSocket が
# 誤って FAIL に見えることがある。その場合はホスト端末で再実行する。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
CODEX="${CODEX:-$(command -v codex)}"
CODEX_HOME="${CODEX_HOME:-${HOME}/.codex}"

codex_in_cursor_agent_sandbox() {
  # Cursor agent shell injects a local proxy and may remap UIDs.
  if [[ -n "${CURSOR_SANDBOX:-}" || -n "${__CURSOR_SANDBOX_ENV_RESTORE:-}" ]]; then
    return 0
  fi
  local proxy="${HTTPS_PROXY:-${https_proxy:-${HTTP_PROXY:-${http_proxy:-}}}}"
  if [[ "$proxy" == http://127.0.0.1:* || "$proxy" == http://localhost:* ]]; then
    return 0
  fi
  return 1
}

echo "== Codex Linux sandbox 診断 =="
echo "codex: $CODEX"
"$CODEX" --version 2>/dev/null || true
echo "bwrap: $(command -v bwrap || echo 'not found')"
echo "uid=$(id -u) user=$(id -un) CODEX_HOME=$CODEX_HOME"
if [[ -r /proc/sys/kernel/apparmor_restrict_unprivileged_userns ]]; then
  echo "kernel.apparmor_restrict_unprivileged_userns=$(cat /proc/sys/kernel/apparmor_restrict_unprivileged_userns)"
fi

IN_AGENT_SANDBOX=0
if codex_in_cursor_agent_sandbox; then
  IN_AGENT_SANDBOX=1
  echo
  echo "WARN: Cursor エージェント sandbox 内で実行中です。"
  echo "  この環境では次が誤検知になりやすいです:"
  echo "    - ~/.codex が root 所有に見える（UID remap）"
  echo "    - bwrap / unshare 失敗"
  echo "    - WebSocket / provider reachability 失敗（ローカル proxy）"
  echo "  正本はホスト端末（sandbox 外）での再実行です:"
  echo "    ./scripts/codex-fix-linux-sandbox.sh"
fi
echo

echo "== CODEX_HOME 所有権 =="
if [[ -d "$CODEX_HOME" ]]; then
  owner="$(stat -c '%U:%G' "$CODEX_HOME" 2>/dev/null || echo unknown)"
  echo "owner: $owner"
  if [[ "$IN_AGENT_SANDBOX" == "1" ]]; then
    echo "NOTE: sandbox 内の owner 表示は信用しない。ホストで stat を確認すること。"
  elif [[ "$owner" != "$(id -un):$(id -gn)" && "$owner" != "$(id -un):"* ]]; then
    echo "WARN: CODEX_HOME の所有者が現在ユーザーと一致しません: $owner"
    echo "  修復例: sudo chown -R \"$(id -un):$(id -gn)\" \"$CODEX_HOME\""
  else
    echo "OK: CODEX_HOME owner matches current user"
  fi
else
  echo "WARN: CODEX_HOME がありません: $CODEX_HOME"
fi
echo

echo "== codex doctor =="
if command -v timeout >/dev/null 2>&1; then
  timeout 30 "$CODEX" doctor --summary --no-color --ascii ||
    echo "WARN: codex doctor failed or timed out"
else
  "$CODEX" doctor --summary --no-color --ascii || true
fi
if [[ "$IN_AGENT_SANDBOX" == "1" ]]; then
  echo "NOTE: doctor の websocket / reachability / state 失敗は sandbox 起因の誤検知の可能性が高い。"
fi
echo

echo "== bwrap バックエンド（既定） =="
if "$CODEX" sandbox -- /bin/pwd 2>&1; then
  echo "OK: bwrap sandbox"
  BWRAP_OK=1
else
  echo "FAIL: bwrap sandbox（Ubuntu 24.04 では AppArmor 制限でよくある）"
  BWRAP_OK=0
  if [[ "$IN_AGENT_SANDBOX" == "1" ]]; then
    echo "NOTE: エージェント sandbox 内では bwrap が落ちやすい。ホストで再確認すること。"
  fi
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

echo "== arg0 / codex-linux-sandbox =="
echo "Codex は ~/.codex/tmp/arg0/*/codex-linux-sandbox を一時生成してシェルを起動する。"
echo "MCP プロセス稼働中に arg0 を rm すると ENOENT でツール実行だけ死ぬ。"
echo "症状例: Unable to spawn .../codex-linux-sandbox because it doesn't exist"
echo "復旧: Cursor で MCP Restart / Cursor 再起動（生きた MCP を kill してから再接続）。"
echo "禁止: find ~/.codex/tmp/arg0 -mtime +N -exec rm のような稼働中掃除。"
if [[ -d "$CODEX_HOME/tmp/arg0" ]]; then
  echo "arg0 dirs: $(find "$CODEX_HOME/tmp/arg0" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | wc -l)"
fi
echo

if [[ "$IN_AGENT_SANDBOX" == "1" && "$BWRAP_OK" != "1" ]]; then
  echo "エージェント sandbox 内の結果は判定に使わないでください。ホストで再実行してください。"
  exit 2
fi

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
