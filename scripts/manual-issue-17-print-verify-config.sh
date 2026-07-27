#!/usr/bin/env bash
# verify シナリオ用の aibe 設定追記案を表示する（ファイルは書き換えない）。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FIXTURE="$ROOT/aibe/tests/fixtures/0069_agent_task_worker.sh"
CONFIG="${AIBE_CONFIG:-$HOME/.config/aibe/config.toml}"

if [[ ! -x "$FIXTURE" ]]; then
  echo "error: fixture not executable: $FIXTURE" >&2
  exit 1
fi

cat <<EOF
# --- Issue #17 verify: 次を ~/.config/aibe/config.toml に追記 ---

[[agent_task.workers]]
id = "0070-fixture"
executable = "$FIXTURE"
args = ["gap_repair"]
timeout_secs = 300
permission_profile = "workspace-write"
env_allowlist = ["PATH"]

# 既存 [tools.shell_exec] の allowed_commands に "test" を追加（例）:
# allowed_commands = ["ls", "git", ..., "test"]

# 反映後:
#   aibe restart
#   bash ./scripts/manual-issue-17-task-completion.sh verify

# 任意: Worker id を明示
#   ISSUE17_VERIFY_WORKER_ID=0070-fixture bash ./scripts/manual-issue-17-task-completion.sh verify

EOF

if [[ -f "$CONFIG" ]]; then
  echo "# --- 現在の config 状態（$CONFIG）---"
  if awk '
    /^\[\[agent_task\.workers\]\]/ { in_w=1; block=$0; next }
    /^\[/ && in_w { print block; in_w=0 }
    in_w { block=block ORS $0 }
    END { if (in_w) print block }
  ' "$CONFIG" | grep -q gap_repair; then
    echo "# OK: gap_repair Worker が見つかりました"
  else
    echo "# NG: gap_repair Worker 未設定"
  fi
  if awk '
    /^\[tools\.shell_exec\]/ { in_s=1; next }
    /^\[/ { in_s=0 }
    in_s && /^allowed_commands/ { print; exit }
  ' "$CONFIG" | grep -q '"test"\|'\''test'\''\|, test\|test,'; then
    echo "# OK: allowed_commands に test あり"
  else
    echo "# NG: allowed_commands に test なし（追記が必要）"
  fi
fi
