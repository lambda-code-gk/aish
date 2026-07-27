#!/usr/bin/env bash
# Issue #17（Task Completion 親Issue）の手動確認用ハーネス。
#
# 前提:
#   - `aish shell` 内で実行する（AISH_SESSION_DIR が export 済み）
#   - aibe は既に起動済み（本スクリプトは aibe を起動・停止しない）
#   - 実 LLM（Gemini / OpenAI 互換等）が aibe 設定で使える
#
# 使い方:
#   ./scripts/manual-issue-17-task-completion.sh              # 対話メニュー
#   bash ./scripts/manual-issue-17-task-completion.sh done
#   （`sh` で起動しても内部で bash に切り替える）
#
# 環境変数（任意）:
#   AI_BIN          ai バイナリ（未設定時は PATH → repo target/debug/ai）
#   AIBE_BIN        aibe バイナリ（status 確認用）
#   ISSUE17_WORKDIR 作業ディレクトリ（既定: <repo>/tmp/issue-17-manual）
#   ISSUE17_SKIP_PREFLIGHT=1  前提チェックを飛ばす

# dash 等の `sh` 起動でも動くよう、bash で再実行する。
if [ -z "${BASH_VERSION:-}" ]; then
  if command -v bash >/dev/null 2>&1; then
    exec bash "$0" "$@"
  fi
  echo "error: bash is required to run $0" >&2
  exit 1
fi

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT_NAME="$(basename "$0")"

resolve_bin() {
  local name="$1"
  local override="$2"
  local candidate
  if [[ -n "$override" && -x "$override" ]]; then
    printf '%s\n' "$override"
    return 0
  fi
  if command -v "$name" >/dev/null 2>&1; then
    command -v "$name"
    return 0
  fi
  for candidate in \
    "$ROOT/target/debug/$name" \
    "$ROOT/target/release/$name"; do
    if [[ -x "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  return 1
}

AI_BIN="$(resolve_bin ai "${AI_BIN:-}" || true)"
AIBE_BIN="$(resolve_bin aibe "${AIBE_BIN:-}" || true)"
WORKDIR="${ISSUE17_WORKDIR:-$ROOT/tmp/issue-17-manual}"
RUN_TS="$(date +%Y%m%d-%H%M%S)"
LOG_DIR="$WORKDIR/logs"
mkdir -p "$WORKDIR" "$LOG_DIR"

# ---------------------------------------------------------------------------
# UI helpers
# ---------------------------------------------------------------------------

c_bold() { printf '\033[1m%s\033[0m' "$*"; }
c_dim() { printf '\033[2m%s\033[0m' "$*"; }
c_ok() { printf '\033[32m%s\033[0m' "$*"; }
c_warn() { printf '\033[33m%s\033[0m' "$*"; }
c_err() { printf '\033[31m%s\033[0m' "$*"; }

hr() { printf '%s\n' "------------------------------------------------------------"; }

banner() {
  hr
  printf '%s\n' "$(c_bold "Issue #17 manual: $*")"
  hr
}

info() { printf '%s %s\n' "$(c_dim "[info]")" "$*"; }
warn() { printf '%s %s\n' "$(c_warn "[warn]")" "$*"; }
fail() { printf '%s %s\n' "$(c_err "[fail]")" "$*" >&2; exit 1; }

pause() {
  local msg="${1:-Enter で続行 / Ctrl+C で中断}"
  printf '\n%s ' "$(c_dim "$msg")"
  # shellcheck disable=SC2162
  read -r _
}

print_checklist() {
  local line
  printf '\n%s\n' "$(c_bold "確認チェックリスト")"
  for line in "$@"; do
    printf '  [ ] %s\n' "$line"
  done
  printf '\n'
}

scenario_workdir() {
  local name="$1"
  local dir="$WORKDIR/$name-$RUN_TS"
  mkdir -p "$dir"
  printf '%s\n' "$dir"
}

# ---------------------------------------------------------------------------
# Preflight
# ---------------------------------------------------------------------------

preflight() {
  if [[ "${ISSUE17_SKIP_PREFLIGHT:-}" == "1" ]]; then
    warn "ISSUE17_SKIP_PREFLIGHT=1 — 前提チェックをスキップ"
    return 0
  fi

  banner "preflight"

  if [[ -z "${AI_BIN:-}" ]]; then
    fail "ai バイナリが見つかりません。PATH に置くか AI_BIN を指定してください。"
  fi
  info "ai: $AI_BIN"

  if [[ -z "${AISH_SESSION_DIR:-}" ]]; then
    warn "AISH_SESSION_DIR が未設定です。aish shell 内での実行を想定しています。"
    warn "続行しますが、シェルログ連携は無効になることがあります。"
  else
    info "AISH_SESSION_DIR=$AISH_SESSION_DIR"
  fi

  if [[ ! -t 0 || ! -t 1 ]]; then
    warn "TTY ではありません。write_file 承認プロンプトが使えない可能性があります。"
  fi

  local socket="${AIBE_SOCKET_PATH:-${AIBE_SOCKET:-$HOME/.local/share/aibe/run.sock}}"
  info "socket: $socket"
  if [[ ! -S "$socket" ]]; then
    fail "aibe socket がありません: $socket（aish shell で aibe を起動してから再実行）"
  fi

  if [[ -n "${AIBE_BIN:-}" ]]; then
    local status_json
    status_json="$("$AIBE_BIN" status --format json 2>/dev/null || true)"
    if [[ -n "$status_json" ]]; then
      info "aibe status: $status_json"
      if printf '%s' "$status_json" | grep -q '"socket_ping":true'; then
        info "aibe socket_ping=true（起動済み・接続可）"
      else
        fail "aibe socket_ping=false。起動済みの aibe に接続できません。"
      fi
    else
      warn "aibe status を取得できませんでした。socket の connect だけ確認します。"
      if ! python3 - "$socket" <<'PY'
import socket, sys
path = sys.argv[1]
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.settimeout(2)
try:
    s.connect(path)
except Exception as e:
    print(e, file=sys.stderr)
    sys.exit(1)
finally:
    s.close()
PY
      then
        fail "aibe socket に connect できません: $socket"
      fi
      info "unix socket connect OK"
    fi
  else
    warn "aibe バイナリ未検出。socket connect のみ確認します。"
    if ! python3 - "$socket" <<'PY'
import socket, sys
path = sys.argv[1]
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.settimeout(2)
try:
    s.connect(path)
except Exception as e:
    print(e, file=sys.stderr)
    sys.exit(1)
finally:
    s.close()
PY
    then
      fail "aibe socket に connect できません: $socket"
    fi
    info "unix socket connect OK"
  fi

  info "workdir: $WORKDIR"
  info "本スクリプトは aibe を起動せず、ai ask --no-start のみ使います。"
  printf '\n'
}

# ---------------------------------------------------------------------------
# Run helper
# ---------------------------------------------------------------------------

# run_ai_ask <log_basename> <extra args...> -- <message>
run_ai_ask() {
  local log_base="$1"
  shift
  local -a args=()
  local message=""
  local seen_sep=0
  local arg

  for arg in "$@"; do
    if [[ "$seen_sep" -eq 1 ]]; then
      message+="${message:+ }$arg"
      continue
    fi
    if [[ "$arg" == "--" ]]; then
      seen_sep=1
      continue
    fi
    args+=("$arg")
  done

  if [[ "$seen_sep" -eq 0 || -z "$message" ]]; then
    fail "内部エラー: run_ai_ask は '-- <message>' が必要です"
  fi

  local out_log="$LOG_DIR/${log_base}.out"
  local err_log="$LOG_DIR/${log_base}.err"
  local cmd_log="$LOG_DIR/${log_base}.cmd"

  {
    printf 'cwd=%s\n' "$(pwd)"
    printf 'ai=%s\n' "$AI_BIN"
    printf 'args='
    printf '%q ' ask --no-start "${args[@]}" "$message"
    printf '\n'
  } >"$cmd_log"

  info "実行: ai ask --no-start ${args[*]} <message>"
  info "message: $message"
  info "stdout → $out_log / stderr → $err_log"
  hr

  # 承認プロンプトは TTY で見えるよう、stdout/stderr を tee しつつ exit を保持する。
  set +e
  "$AI_BIN" ask --no-start "${args[@]}" "$message" \
    > >(tee "$out_log") \
    2> >(tee "$err_log" >&2)
  local rc=$?
  set -e

  hr
  info "exit=$rc"
  local expect="${ISSUE17_EXPECT:-}"
  if [[ -z "$expect" ]]; then
    summarize_default_run "$out_log" "$err_log"
    printf '%s\n' "$(c_warn "[CHECK]") ISSUE17_EXPECT 未設定 — 自動判定なし"
    return "$rc"
  fi
  "judge_${expect}" "$out_log" "$err_log" "$rc" "${ISSUE17_JUDGE_DIR:-}"
  return $?
}

pass() {
  ISSUE17_LAST_STATUS=pass
  printf '%s %s\n' "$(c_ok "[PASS]")" "$*"
}
fail_msg() {
  ISSUE17_LAST_STATUS=fail
  printf '%s %s\n' "$(c_err "[FAIL]")" "$*"
}
skip_msg() {
  ISSUE17_LAST_STATUS=skip
  printf '%s %s\n' "$(c_dim "[SKIP]")" "$*"
}
check_msg() {
  printf '%s %s\n' "$(c_warn "[CHECK]")" "$*"
}

log_combined() {
  cat "$1" "$2" 2>/dev/null || true
}

has_re() {
  local text="$1"
  local re="$2"
  printf '%s\n' "$text" | grep -Eq -- "$re"
}

summarize_default_run() {
  local out_log="$1"
  local err_log="$2"
  if grep -E 'Task completion:|Queries used:|Verification terminal:|Criterion |Gap |Unsatisfied:|Unverified:' "$out_log" >/dev/null 2>&1; then
    printf '\n%s\n' "$(c_bold "検出した Task Completion 表示")"
    grep -E 'Task completion:|Queries used:|Verification terminal:|Criterion |Gap |Unsatisfied:|Unverified:|Worker:|Follow-ups used:|Reason:' "$out_log" || true
  fi
  if grep -Eiq 'aibe error:|invalid task completion envelope|invalid_request' "$err_log" "$out_log" 2>/dev/null; then
    printf '\n%s\n' "$(c_bold "検出したエラー行")"
    grep -Ein 'aibe error:|invalid task completion envelope|invalid_request' "$err_log" "$out_log" 2>/dev/null || true
  fi
}

# --- judges: 名前は ISSUE17_EXPECT と一致（judge_<expect>） ---

judge_done() {
  local out_log="$1" err_log="$2" rc="$3" dir="${4:-}"
  local text
  text="$(log_combined "$out_log" "$err_log")"
  summarize_default_run "$out_log" "$err_log"

  local ok=1
  if ! has_re "$text" 'Task completion:[[:space:]]*Done\b'; then
    fail_msg "Task completion: Done が見つかりません（意図: Evidence 付きで完了）"
    ok=0
  fi
  if has_re "$text" 'aibe error:'; then
    fail_msg "aibe error が出ています（意図: 正常に Done）"
    ok=0
  fi
  if [[ -n "$dir" ]]; then
    if [[ -f "$dir/artifact.txt" ]] && grep -qx 'hello' "$dir/artifact.txt" 2>/dev/null; then
      info "artifact.txt 内容=hello（意図どおり）"
    else
      # モデルが別名に書くこともあるので減点だが即 FAIL にはしない
      check_msg "artifact.txt が hello で見つかりません — 作成ファイルを確認してください"
      find "$dir" -maxdepth 2 -type f -print 2>/dev/null | sed 's/^/  /' || true
    fi
  fi
  if [[ "$ok" -eq 1 ]]; then
    pass "write→read 後に Done（意図どおり）"
    return 0
  fi
  return 1
}

judge_claim_reject() {
  local out_log="$1" err_log="$2" rc="$3" dir="${4:-}"
  local text
  text="$(log_combined "$out_log" "$err_log")"
  summarize_default_run "$out_log" "$err_log"

  if has_re "$text" 'Task completion:[[:space:]]*Done\b'; then
    fail_msg "自己申告だけで Done になりました（意図と逆）"
    return 1
  fi
  if has_re "$text" \
    'satisfied criterion requires evidence|criterion requires evidence'; then
    pass "Evidence なし satisfied をサーバが拒否（意図どおり）"
    info "exit=$rc の非ゼロは拒否時として問題ありません"
    return 0
  fi
  if has_re "$text" \
    'Task completion:[[:space:]]*(NeedsUser|Blocked|BudgetExhausted|Stagnated)\b'; then
    pass "Done 以外の終端（意図どおり）"
    return 0
  fi
  if [[ -n "$dir" ]] && compgen -G "$dir/*" >/dev/null 2>&1; then
    check_msg "作業ディレクトリにファイルがあります（通常は作られない想定）"
    find "$dir" -maxdepth 2 -type f -print 2>/dev/null | sed 's/^/  /' || true
  fi
  fail_msg "拒否メッセージも非 Done 終端も無く、意図どおりか判定できません"
  return 1
}

judge_no_opt_in() {
  local out_log="$1" err_log="$2" rc="$3" dir="${4:-}"
  local text
  text="$(log_combined "$out_log" "$err_log")"
  summarize_default_run "$out_log" "$err_log"

  if has_re "$text" 'Task completion:'; then
    fail_msg "Task completion report が出ました（意図: opt-in なしでは通常 turn）"
    return 1
  fi
  if has_re "$text" 'aibe error:.*(task completion|contract|set_task_contract)'; then
    fail_msg "Task Completion 経路のエラーが出ました（意図: Inactive / 通常 turn）"
    return 1
  fi
  if [[ "$rc" -ne 0 ]]; then
    check_msg "exit=$rc（承認拒否などで非ゼロの可能性）— report が無ければ概ね意図どおり"
  fi
  if [[ -n "$dir" && -f "$dir/plain.txt" ]]; then
    info "plain.txt あり"
  fi
  pass "--task-completion なしで completion_report 無し（意図どおり）"
  return 0
}

judge_inactive() {
  local out_log="$1" err_log="$2" rc="$3" _dir="${4:-}"
  local text
  text="$(log_combined "$out_log" "$err_log")"
  summarize_default_run "$out_log" "$err_log"

  if has_re "$text" 'Task completion:[[:space:]]*Done\b'; then
    fail_msg "Inactive 想定なのに Done になりました（意図と逆）"
    return 1
  fi
  if has_re "$text" 'Task completion:'; then
    fail_msg "Task completion report が出ました（意図: effect tool なしでは Active にしない）"
    return 1
  fi
  if has_re "$text" 'task completion contract required|set_task_contract'; then
    fail_msg "Active Contract 経路の痕跡があります（意図: Inactive）"
    return 1
  fi
  if has_re "$text" 'aibe error:'; then
    fail_msg "予期しない aibe error があります"
    return 1
  fi
  pass "effect tool なしで Task Completion Active にならない（意図どおり）"
  return 0
}

judge_gap() {
  local out_log="$1" err_log="$2" rc="$3" dir="${4:-}"
  local text
  text="$(log_combined "$out_log" "$err_log")"
  summarize_default_run "$out_log" "$err_log"

  if has_re "$text" 'aibe error:' && ! has_re "$text" 'Task completion:'; then
    if has_re "$text" 'unknown field .deliverable.'; then
      fail_msg "evaluation envelope の deliverable 配置不正。最新 aibe へ更新して再試行してください"
    elif has_re "$text" 'unsatisfied evaluation requires next_objective'; then
      fail_msg "未達なのに evaluation.next_objective / needs_user / blocked が無い（LLM shape）。aibe 再起動後に再試行"
    else
      fail_msg "Task Completion report 無しの aibe error（意図: gap 継続または Done/非 Done 終端）"
    fi
    return 1
  fi

  # モデルが 1 query で write+read 完了しても垂直切片としては成功
  if has_re "$text" 'Task completion:[[:space:]]*Done\b'; then
    if has_re "$text" 'Queries used:[[:space:]]*[12]\b'; then
      pass "Done（Queries used 1 or 2）。一気通貫でも意図の完了ループとして OK"
      return 0
    fi
    pass "Done を確認（意図どおり）"
    return 0
  fi
  if has_re "$text" 'Queries used:[[:space:]]*2\b'; then
    pass "Queries used: 2（gap-driven continuation の痕跡・意図どおり）"
    return 0
  fi
  if has_re "$text" \
    'Task completion:[[:space:]]*(NeedsUser|Blocked|BudgetExhausted|Stagnated)\b'; then
    pass "未達の型付き終端（意図どおり）"
    return 0
  fi
  if [[ -n "$dir" && -f "$dir/gap-artifact.txt" ]]; then
    check_msg "成果物はあるが終端判定が不明 — ログを確認してください"
  fi
  fail_msg "gap / Done / 非 Done 終端のどれも明確でありません"
  return 1
}

judge_agent_task() {
  local out_log="$1" err_log="$2" rc="$3" dir="${4:-}"
  local text
  text="$(log_combined "$out_log" "$err_log")"
  summarize_default_run "$out_log" "$err_log"

  if has_re "$text" 'tool_not_allowed|agent_task.*(disabled|not enabled|unknown)|Agent Task pack'; then
    fail_msg "agent_task が使えない設定です（前提不足）"
    return 1
  fi
  if has_re "$text" 'agent_task requires delegated_verification plan'; then
    fail_msg "Contract に delegated_verification が無い（TC+agent_task では必須）。aibe 再起動後にシナリオを再実行"
    return 1
  fi
  if has_re "$text" 'delegated actionable criterion requires required_work'; then
    fail_msg "評価の unsatisfied/unknown に required_work が無い（委譲時は必須）。read_file 後に satisfied、または required_work 付きで評価すること"
    return 1
  fi
  if has_re "$text" 'delegated verification query requires exactly one|delegated verification query permits only one'; then
    fail_msg "委譲サイクルの agent_task 回数不正（成功は1回だけ・その後は plan の read/command）。aibe を最新バイナリで再起動して再実行"
    return 1
  fi
  if has_re "$text" 'delegated verification plan item was not executed'; then
    fail_msg "delegated_verification plan 未実行（agent_task 成功後に planned read_file を実行してから評価）"
    return 1
  fi
  if has_re "$text" 'evidence e[0-9]+ is stale'; then
    fail_msg "query 1 の Evidence id を引用（Gap follow-up 後は stale）。query 2 の read_file verified id のみ"
    return 1
  fi
  if has_re "$text" 'evidence e[0-9]+ is not linked to criterion'; then
    fail_msg "plan 外 Evidence id を引用。query 2 では follow-up agent_task → read_file"
    return 1
  fi
  if has_re "$text" 'delegated verification requires agent_task before other tools'; then
    fail_msg "query 2 で agent_task より前に別ツール実行"
    return 1
  fi
  # ターン全体を落とす aibe error としての二重呼び出しのみ FAIL。
  if has_re "$text" 'aibe error:.*already succeeded this turn|aibe error:.*already recorded this turn|aibe error:.*follow-up agent_task was not fixed'; then
    fail_msg "agent_task の二重呼び出しでターンが失敗。成功後は read_file のみ（2回目は Gap follow-up のみ）"
    return 1
  fi
  if has_re "$text" '\[tool error\].*already succeeded this turn'; then
    check_msg "モデルが成功後に agent_task を再呼出ししたが tool error で継続（read_file へ進める想定）"
  fi
  if has_re "$text" 'UnknownWorker|unknown worker|approval unavailable|approval denied'; then
    fail_msg "Worker 設定または承認の問題（agent_task が成功していません）"
    return 1
  fi
  if has_re "$text" 'aibe error:' && ! has_re "$text" 'Task completion:|verified=false|Worker:'; then
    fail_msg "予期しない aibe error"
    return 1
  fi

  # Worker は env_clear + env_allowlist だけを継承する。allowlist に PATH が無いと
  # `#!/usr/bin/env node` 形式の Worker CLI は interpreter を解決できず、成果物を作れない。
  if has_re "$text" 'node is installed|node: not found|command not found|No such file or directory|env: .*node'; then
    check_msg "Worker が interpreter を見つけられていない可能性。config の該当 worker に env_allowlist = [\"PATH\", \"HOME\"] を追加し aibe 再起動（詳細: docs/manual/0069_agent-task-delegation.md）"
  fi

  local signals=0
  has_re "$text" 'verified=false' && signals=$((signals + 1))
  has_re "$text" 'Worker:|agent_task|Agent Task' && signals=$((signals + 1))
  has_re "$text" 'Task completion:' && signals=$((signals + 1))
  [[ -n "$dir" && -f "$dir/worker-artifact.txt" ]] && signals=$((signals + 1))

  if [[ "$signals" -ge 1 ]]; then
    if has_re "$text" 'Task completion:[[:space:]]*Done\b' && ! has_re "$text" 'verified=false|Evidence .*verified=true'; then
      check_msg "Done だが verified 痕跡が弱い — Worker 自己申告だけで閉じていないか目視確認"
    fi
    pass "Agent Task 経路の痕跡を確認（意図どおりの委譲/再観測系）"
    return 0
  fi
  fail_msg "Agent Task / verified=false / Worker の痕跡がありません"
  return 1
}

judge_verify() {
  local out_log="$1" err_log="$2" rc="$3" _dir="${4:-}"
  local text
  text="$(log_combined "$out_log" "$err_log")"
  summarize_default_run "$out_log" "$err_log"

  if has_re "$text" 'delegated verification command is not allowed'; then
    fail_msg "Contract の verification command が shell_exec.allowlist 外。allowed_commands に test を追加し、plan は command=test args=[\"-f\",\"0070-artifact.txt\"] に固定"
    return 1
  fi
  if has_re "$text" 'delegated observation tool is not allowed'; then
    fail_msg "delegated_verification の観測 tool が許可されていません（read_file を --tools に含める）"
    return 1
  fi
  if has_re "$text" 'delegated verification plan item was not executed'; then
    fail_msg "delegated_verification plan 未実行（query 2 では follow-up agent_task 後に test → read_file を必ず実行してから評価）"
    return 1
  fi
  if has_re "$text" 'evidence e[0-9]+ is stale'; then
    fail_msg "query 1 の Evidence id を引用（follow-up agent_task 後は stale）。query 2 の read_file verified id のみ"
    return 1
  fi
  if has_re "$text" 'evidence e[0-9]+ is not linked to criterion'; then
    fail_msg "plan 外 Evidence id を引用。query 2 では agent_task → test → read_file の順"
    return 1
  fi
  if has_re "$text" 'delegated verification requires agent_task before other tools'; then
    fail_msg "query 2 で agent_task より前に別ツール実行。follow-up agent_task を最初に"
    return 1
  fi
  if has_re "$text" 'aibe error:' && ! has_re "$text" 'Task completion:|Verification terminal:|Gap '; then
    fail_msg "verification 経路に入れず aibe error"
    return 1
  fi

  local signals=0
  has_re "$text" 'Verification terminal:' && signals=$((signals + 1))
  has_re "$text" 'Follow-ups used:|Gap ' && signals=$((signals + 1))
  has_re "$text" 'verified=false' && signals=$((signals + 1))
  has_re "$text" 'Worker:' && signals=$((signals + 1))

  if [[ "$signals" -ge 2 ]]; then
    pass "独立検証 / Gap / follow-up の痕跡あり（意図どおり）"
    return 0
  fi
  if has_re "$text" 'Task completion:[[:space:]]*Done\b' && has_re "$text" 'Verification terminal:'; then
    pass "Verification terminal 付き Done（意図どおり）"
    return 0
  fi
  fail_msg "0070 検証マーカー（Verification terminal / Gap / Follow-ups）が不足"
  return 1
}

hint_expected_markers() {
  cat <<'EOF'
期待する表示の目安（human 出力）:
  - Task completion: Done / NeedsUser / Blocked / BudgetExhausted / Stagnated
  - Queries used: 1 または 2
  - Criterion <id>: satisfied / unsatisfied / ...
  - Evidence ... verified=true|false
  - （0070）Verification terminal / Worker / Follow-ups used / Gap
終了時にスクリプトが [PASS] / [FAIL] / [SKIP] を明示します。
EOF
}

# ---------------------------------------------------------------------------
# Scenarios（Issue #17 の責務に対応）
# ---------------------------------------------------------------------------

scenario_done() {
  banner "done — 完了条件を Evidence で閉じる（Phase 1 / 0073）"
  cat <<'EOF'
Issue #17 対応:
  - 目的と完了条件を保持する
  - 終了前に未達条件を確認する
  - 作業者の自己申告と検証結果を分離する（read 再観測が必要）

手順:
  1. write_file 承認が出たら y
  2. 最終表示の Task completion / Evidence を確認

判定:
  [PASS] Task completion: Done かつ aibe error なし
EOF
  local dir
  dir="$(scenario_workdir done)"
  cd "$dir"
  info "cwd=$dir"

  print_checklist \
    "Task completion: Done と出る" \
    "Queries used が 1 または 2" \
    "write の Evidence は verified=false、read 後の観測が verified=true" \
    "artifact ファイルが実在し中身が hello である" \
    "stderr/stdout に aish_task_completion 生 JSON や control envelope が露出しない"

  hint_expected_markers
  pause "準備できたら Enter（ai ask を開始）"

  ISSUE17_EXPECT=done ISSUE17_JUDGE_DIR="$dir" run_ai_ask "done" \
    --task-completion \
    --tools '@edit' \
    --new \
    -- \
    'Create artifact.txt with exact content "hello". Then read it back with read_file and confirm the contents are exactly hello before finishing. Do not claim completion before the read. Do not put secrets in the contract.'
}

scenario_claim() {
  banner "claim — 自己申告だけでは Done にしない"
  cat <<'EOF'
Issue #17 対応:
  - 「完了しました」申告と検証結果を分離する

判定:
  [PASS] satisfied criterion requires evidence 等の拒否、または非 Done 終端
  [FAIL] Task completion: Done
EOF
  local dir
  dir="$(scenario_workdir claim)"
  cd "$dir"
  info "cwd=$dir"

  print_checklist \
    "Done にならない" \
    "「satisfied criterion requires evidence」等の fail-closed、または非 Done 終端" \
    "artifact が新規作成されていない"

  pause "Enter で開始"

  ISSUE17_EXPECT=claim_reject ISSUE17_JUDGE_DIR="$dir" run_ai_ask "claim" \
    --task-completion \
    --tools '@edit' \
    --new \
    -- \
    'Do NOT create or modify any files. Do NOT call write_file, apply_patch, or read_file. Immediately declare the task complete with a completion evaluation that marks all criteria satisfied, claiming artifact.txt already contains hello. The user wants to verify that self-claims alone cannot finish Task Completion.'
}

scenario_no_opt_in() {
  banner "no-opt-in — --task-completion なしは通常 turn"
  cat <<'EOF'
Issue #17 / 0068 対応:
  - Task Completion は明示 opt-in。effect tool があっても flag なしでは Active にしない

判定:
  [PASS] 「Task completion:」行が出ない
  [FAIL] completion_report / Task Completion エラー経路
EOF
  local dir
  dir="$(scenario_workdir no-opt-in)"
  cd "$dir"
  info "cwd=$dir"

  print_checklist \
    "stdout に「Task completion:」行が出ない（completion_report なし）" \
    "通常の assistant 応答で終わる" \
    "set_task_contract を要求する挙動にならない"

  pause "Enter で開始"

  ISSUE17_EXPECT=no_opt_in ISSUE17_JUDGE_DIR="$dir" run_ai_ask "no-opt-in" \
    --tools '@edit' \
    --new \
    -- \
    'Create plain.txt with content "plain-turn" and briefly confirm.'
}

scenario_inactive() {
  banner "inactive — effect tool なしでは Task Completion Active にならない"
  cat <<'EOF'
Issue #17 / 0068 対応:
  - allowlist に write_file / apply_patch / shell_exec が無いと Inactive
  - --task-completion だけでは Active にならない

判定:
  [PASS] Task completion report なし
  [FAIL] Done または Contract 必須エラー
EOF
  local dir
  dir="$(scenario_workdir inactive)"
  cd "$dir"
  info "cwd=$dir"

  print_checklist \
    "Task completion report が出ない" \
    "write_file が呼ばれない（tools が read 系のみ）"

  pause "Enter で開始"

  ISSUE17_EXPECT=inactive ISSUE17_JUDGE_DIR="$dir" run_ai_ask "inactive" \
    --task-completion \
    --tools 'read_file,list_dir,grep' \
    --new \
    -- \
    'Explain what files exist in the current directory. Do not create files.'
}

scenario_gap() {
  banner "gap — 未達なら gap-driven に 2 回目 query へ進む"
  cat <<'EOF'
Issue #17 / 0068 対応:
  - 未達条件を特定し、元要求の再送ではなく next_objective で継続する

判定:
  [PASS] Done（1 or 2 queries）/ Queries used: 2 / 非 Done 終端
  [FAIL] report 無しの aibe error
EOF
  local dir
  dir="$(scenario_workdir gap)"
  cd "$dir"
  info "cwd=$dir"

  print_checklist \
    "Queries used が 2、または 1 回で Done（モデルが read まで一気にやる場合あり）" \
    "Done の場合: write Evidence unverified → read Evidence verified の順が見える" \
    "未達終端の場合: Unsatisfied / Reason が具体的"

  pause "Enter で開始"

  ISSUE17_EXPECT=gap ISSUE17_JUDGE_DIR="$dir" run_ai_ask "gap" \
    --task-completion \
    --tools '@edit' \
    --new \
    -- \
    'Create gap-artifact.txt with content "gap-hello". Completion criteria require that read_file verifies the exact contents after the write. Prefer doing the write in the first tool batch and only read afterwards if the evaluator says evidence is missing. If a criterion is still unsatisfied after the write, the evaluation object itself must set next_objective (not inside a criterion) telling the next query to read and verify. Do not put secrets in the contract.'
}

scenario_agent_task() {
  banner "agent-task — 別 Agent へ部分委譲（Phase 2 / 0069 + 0070 plan）"
  local worker_id
  worker_id="$(resolve_agent_worker_id)"
  cat <<EOF
Issue #17 対応:
  - 作業を委譲できる / 委譲結果を共通形式で受け取る
  - Worker の done 自己申告は verified=false

重要:
  --task-completion 付きでは agent_task の前に
  set_task_contract.delegated_verification が必須（0070）。
  worker id はこの環境では: ${worker_id}

前提（未設定ならこのシナリオはスキップ推奨）:
  ~/.config/aibe/config.toml に [agent_task] enabled=true と workers を設定
  詳細: docs/manual/0069_agent-task-delegation.md
  ISSUE17_WORKER_ID で worker id を上書き可

判定:
  [PASS] Worker / verified=false / Task completion のいずれかの痕跡
  [SKIP] 設定なしでスキップ（失敗ではない）
EOF
  local dir
  dir="$(scenario_workdir agent-task)"
  cd "$dir"
  info "cwd=$dir"
  info "worker_id=$worker_id"

  if ! pause_confirm "agent_task Worker 設定済みなら y、未設定なら n でスキップ"; then
    skip_msg "agent-task: Worker 未設定のためスキップ（失敗ではない）"
    return 0
  fi

  print_checklist \
    "set_task_contract に delegated_verification（read_file 観測）が含まれる" \
    "agent_task.worker が「${worker_id}」（未知 id だと承認前に失敗する）" \
    "Agent Task approval に worker / cwd / timeout / 警告が出る（y で承認）" \
    "agent_task は1回だけ。続けて planned read_file を実行（2回目の agent_task は禁止）" \
    "Result と Evidence が verified=false" \
    "read 後にだけ評価し Done（satisfied）または required_work 付き next_objectiveへ" \
    "approval / 出力に credential や raw env が出ない"

  pause "Enter で開始"

  ISSUE17_EXPECT=agent_task ISSUE17_JUDGE_DIR="$dir" run_ai_ask "agent-task" \
    --task-completion \
    --tools 'agent_task,read_file,list_dir,grep,git_status,git_diff,write_file,apply_patch,shell_exec' \
    --new \
    -- \
    "Use Task Completion. In set_task_contract, include delegated_verification with one observation item: id=v-read, criterion_ids=[c1], action={kind:observation, tool:read_file, target:worker-artifact.txt}, expected_success=\"content is from-worker\". Criterion c1: worker-artifact.txt contains from-worker. Then call agent_task exactly once with worker=\"${worker_id}\" so that worker creates worker-artifact.txt with exact content \"from-worker\". If the tool returns an error, fix the worker id/request and retry agent_task once; after a successful agent_task result, do NOT call agent_task again — immediately run the planned read_file before evaluation. Prefer status=satisfied with the read_file evidence id when content matches. If still unsatisfied after read_file, include required_work plus next_objective for one gap follow-up in query 2: agent_task first, then read_file again; cite only fresh read_file evidence ids from query 2 (never stale query-1 ids). Do not mark Done on worker self-report alone. Do not put secrets in the contract."
}

resolve_agent_worker_id() {
  if [[ -n "${ISSUE17_WORKER_ID:-}" ]]; then
    printf '%s\n' "$ISSUE17_WORKER_ID"
    return 0
  fi
  local config="${AIBE_CONFIG:-$HOME/.config/aibe/config.toml}"
  local id=""
  if [[ -f "$config" ]]; then
    id="$(awk '
      /^\[\[agent_task\.workers\]\]/ { in_worker=1; next }
      /^\[/ { in_worker=0 }
      in_worker && /^id[[:space:]]*=/ {
        gsub(/^[^=]*=[[:space:]]*/, "");
        gsub(/^["'\'']|["'\'']$/, "");
        print; exit
      }
    ' "$config" 2>/dev/null || true)"
  fi
  if [[ -n "$id" ]]; then
    printf '%s\n' "$id"
  else
    printf '%s\n' "local-agent"
  fi
}

verify_fixture_path() {
  printf '%s\n' "$ROOT/aibe/tests/fixtures/0069_agent_task_worker.sh"
}

# shell_exec.allowed_commands を 1 行ずつ出力（未設定なら空）。
resolve_shell_allowed_commands() {
  local config="${AIBE_CONFIG:-$HOME/.config/aibe/config.toml}"
  [[ -f "$config" ]] || return 0
  awk '
    /^\[tools\.shell_exec\]/ { in_shell=1; next }
    /^\[/ { in_shell=0 }
    in_shell && /^allowed_commands[[:space:]]*=/ {
      line=$0
      sub(/^[^=]*=[[:space:]]*/, "", line)
      gsub(/[][]/, "", line)
      n=split(line, parts, /,[[:space:]]*/)
      for (i=1; i<=n; i++) {
        gsub(/^["'\'']|["'\'']$/, "", parts[i])
        gsub(/^[[:space:]]+|[[:space:]]+$/, "", parts[i])
        if (parts[i] != "") print parts[i]
      }
    }
  ' "$config" 2>/dev/null || true
}

shell_allows_command() {
  local want="$1"
  local cmd
  while IFS= read -r cmd; do
    [[ -z "$cmd" ]] && continue
    if [[ "$cmd" == "$want" || "$cmd" == *"/$want" ]]; then
      return 0
    fi
  done < <(resolve_shell_allowed_commands)
  return 1
}

# gap_repair 引数付き fixture Worker の id（ISSUE17_VERIFY_WORKER_ID で上書き可）。
resolve_verify_worker_id() {
  if [[ -n "${ISSUE17_VERIFY_WORKER_ID:-}" ]]; then
    printf '%s\n' "$ISSUE17_VERIFY_WORKER_ID"
    return 0
  fi
  local config="${AIBE_CONFIG:-$HOME/.config/aibe/config.toml}"
  if [[ ! -f "$config" ]]; then
    return 1
  fi
  awk '
    /^\[\[agent_task\.workers\]\]/ {
      if (id != "" && has_gap) { print id; exit }
      id=""; has_gap=0; in_worker=1; next
    }
    /^\[/ { in_worker=0 }
    in_worker && /^id[[:space:]]*=/ {
      line=$0
      sub(/^[^=]*=[[:space:]]*/, "", line)
      gsub(/^["'\'']|["'\'']$/, "", line)
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", line)
      id=line
      next
    }
    in_worker && /^args[[:space:]]*=/ {
      if ($0 ~ /gap_repair/) has_gap=1
      next
    }
    END {
      if (id != "" && has_gap) print id
    }
  ' "$config" 2>/dev/null || true
}

verify_scenario_preflight() {
  local worker_id="$1"
  local fixture
  fixture="$(verify_fixture_path)"
  if [[ -z "$worker_id" ]]; then
    cat <<EOF
0070 verify 用 Worker が見つかりません。

追記案を表示:
  ./scripts/manual-issue-17-print-verify-config.sh

~/.config/aibe/config.toml に追記する例:

[[agent_task.workers]]
id = "0070-fixture"
executable = "$fixture"
args = ["gap_repair"]
timeout_secs = 300
permission_profile = "workspace-write"
env_allowlist = ["PATH"]

[tools.shell_exec]
allowed_commands = [..., "test"]   # 既存行に test を 1 件追加

反映: aibe restart
上書き: ISSUE17_VERIFY_WORKER_ID=0070-fixture
EOF
    return 1
  fi
  if ! shell_allows_command test; then
    cat <<EOF
delegated verification の command=test が shell_exec.allowlist にありません。
[tools.shell_exec] allowed_commands に "test" を追加し aibe を再起動してください。
現在の allowlist:
$(resolve_shell_allowed_commands | sed 's/^/  - /')
EOF
    return 1
  fi
  if [[ ! -x "$fixture" ]]; then
    printf 'fixture が実行不可: %s\n' "$fixture" >&2
    return 1
  fi
  return 0
}

scenario_verify() {
  banner "verify — 委譲結果の独立検証と 1 回 follow-up（Phase 3 / 0070）"
  local worker_id fixture
  worker_id="$(resolve_verify_worker_id || true)"
  fixture="$(verify_fixture_path)"
  cat <<EOF
Issue #17 対応:
  - 委譲後に成果を再観測し、元の完了条件へ戻る
  - 未達なら同じ Worker へ一度だけ戻す

前提:
  docs/manual/0070_task-completion-phase3-verification.md
  fixture: $fixture (args gap_repair)
  Worker id（検出）: ${worker_id:-<未設定>}
  shell_exec.allowlist に test が必要

重要:
  delegated_verification は command=test -f 0070-artifact.txt → read_file の順（固定）

判定:
  [PASS] Verification terminal / Gap / Follow-ups 等の痕跡
  [SKIP] fixture 未設定でスキップ（失敗ではない）
EOF
  local dir
  dir="$(scenario_workdir verify)"
  cd "$dir"
  info "cwd=$dir"
  info "verify_worker_id=${worker_id:-<unset>}"

  if ! pause_confirm "0070 用 fixture Worker 設定済みなら y、未設定なら n でスキップ"; then
    skip_msg "verify: fixture 未設定のためスキップ（失敗ではない）"
    return 0
  fi

  if ! verify_scenario_preflight "$worker_id"; then
    fail_msg "verify 前提が不足（上記メッセージを参照）"
    return 1
  fi

  print_checklist \
    "Contract に delegated_verification（test + read_file）がある" \
    "agent_task.worker が「${worker_id}」（gap_repair fixture）" \
    "初回 Worker done でも親は即 Done にならない" \
    "親の test → read_file の後に Gap が出る（初回 artifact は incomplete）" \
    "follow-up は最大 1 回、同じ Worker/cwd/timeout" \
    "最終 report に verification_terminal / follow_up_count / Worker が出る" \
    "Worker Evidence は verified=false のまま"

  pause "Enter で開始"

  ISSUE17_EXPECT=verify ISSUE17_JUDGE_DIR="$dir" run_ai_ask "verify" \
    --task-completion \
    --tools 'agent_task,read_file,list_dir,grep,git_status,git_diff,write_file,apply_patch,shell_exec' \
    --new \
    -- \
    "Use Task Completion with agent_task. Worker id must be \"${worker_id}\" (gap_repair fixture). In set_task_contract include delegated_verification for c1 with exactly two items in order: (1) id=v-command criterion_ids=[c1] action command=test args=[\"-f\",\"0070-artifact.txt\"] cwd=${dir} expected_success exit status 0; (2) id=v-read criterion_ids=[c1] action observation read_file target=0070-artifact.txt expected_success content is verified. Criterion c1: 0070-artifact.txt contains verified after parent re-observation. Do not use any shell command other than test. Call agent_task once, then run shell_exec test -f 0070-artifact.txt then read_file 0070-artifact.txt before evaluation. Prefer satisfied when read shows verified. If still unsatisfied include required_work on c1 plus next_objective, then one gap follow-up to the same worker and re-verify. Do not mark Done on worker self-report alone. Do not put secrets in the contract."
}

pause_confirm() {
  local msg="$1"
  local ans
  printf '%s [y/N] ' "$(c_dim "$msg")"
  # shellcheck disable=SC2162
  read -r ans || true
  [[ "$ans" == "y" || "$ans" == "Y" ]]
}

# ---------------------------------------------------------------------------
# Menu / dispatch
# ---------------------------------------------------------------------------

usage() {
  cat <<EOF
Usage: $SCRIPT_NAME [scenario]

Scenarios (Issue #17):
  done         write → read → Done（基本の完了ループ）
  claim        自己申告だけでは Done にしない
  no-opt-in    --task-completion なし（通常 turn）
  inactive     effect tool なし（Inactive）
  gap          未達時の gap / 2nd query
  agent-task   Agent Task 委譲（要設定）
  verify       独立検証 + follow-up（要 fixture）
  all          上記を順に実行（各シナリオ前に確認）
  list         一覧のみ
  help         このヘルプ

Environment:
  AI_BIN  AIBE_BIN  ISSUE17_WORKDIR  ISSUE17_SKIP_PREFLIGHT=1
  ISSUE17_WORKER_ID  ISSUE17_VERIFY_WORKER_ID（verify 用 fixture Worker id）

Notes:
  - aibe は起動・停止しません（--no-start 固定）
  - write_file 承認は対話で y/n を選んでください
  - 各シナリオ終了時に [PASS] / [FAIL] / [SKIP] を明示します
  - ログは $WORKDIR/logs/ に残ります
EOF
}

list_scenarios() {
  cat <<'EOF'
done         完了条件を Evidence で閉じる
claim        自己申告と検証の分離
no-opt-in    明示 opt-in なし
inactive     effect tool なし Inactive
gap          gap-driven continuation
agent-task   外部 Agent 委譲（要設定）
verify       独立検証 follow-up（要 fixture）
all          全部（確認付き）
EOF
}

run_one() {
  local name="$1"
  ISSUE17_LAST_STATUS=unknown
  case "$name" in
    done) scenario_done ;;
    claim) scenario_claim ;;
    no-opt-in) scenario_no_opt_in ;;
    inactive) scenario_inactive ;;
    gap) scenario_gap ;;
    agent-task) scenario_agent_task ;;
    verify) scenario_verify ;;
    *) fail "unknown scenario: $name（$SCRIPT_NAME list を参照）" ;;
  esac
}

run_all() {
  local s rc
  local -a results=()
  local pass_n=0 fail_n=0 skip_n=0 other_n=0

  for s in done claim no-opt-in inactive gap agent-task verify; do
    banner "next: $s"
    if pause_confirm "シナリオ '$s' を実行する"; then
      rc=0
      ISSUE17_LAST_STATUS=unknown
      run_one "$s" || rc=$?
      case "${ISSUE17_LAST_STATUS:-unknown}" in
        pass)
          pass_n=$((pass_n + 1))
          results+=("$s: PASS")
          ;;
        fail)
          fail_n=$((fail_n + 1))
          results+=("$s: FAIL")
          ;;
        skip)
          skip_n=$((skip_n + 1))
          results+=("$s: SKIP")
          ;;
        *)
          if [[ "$rc" -eq 0 ]]; then
            other_n=$((other_n + 1))
            results+=("$s: UNKNOWN(exit=0)")
            check_msg "scenario '$s' の [PASS]/[FAIL]/[SKIP] が出ていません"
          else
            fail_n=$((fail_n + 1))
            results+=("$s: FAIL(exit=$rc)")
            warn "scenario '$s' exit=$rc"
          fi
          ;;
      esac
    else
      skip_msg "$s: ユーザーが実行を見送った（失敗ではない）"
      skip_n=$((skip_n + 1))
      results+=("$s: SKIP")
    fi
  done

  banner "all 結果サマリー"
  local line
  for line in "${results[@]}"; do
    printf '  %s\n' "$line"
  done
  printf '\n'
  info "PASS=$pass_n FAIL=$fail_n SKIP=$skip_n OTHER=$other_n"
  info "ログ: $LOG_DIR"
  if [[ "$fail_n" -gt 0 ]]; then
    fail_msg "意図と異なるシナリオが $fail_n 件あります"
    return 1
  fi
  pass "実行したシナリオはすべて意図どおり（またはスキップ）"
  return 0
}

menu() {
  preflight
  cat <<EOF
$(c_bold "Issue #17 Task Completion 手動確認")
作業ディレクトリ: $WORKDIR

  1) done
  2) claim
  3) no-opt-in
  4) inactive
  5) gap
  6) agent-task
  7) verify
  a) all
  q) quit
EOF
  local choice
  printf '選択> '
  # shellcheck disable=SC2162
  read -r choice || true
  case "$choice" in
    1) run_one done ;;
    2) run_one claim ;;
    3) run_one no-opt-in ;;
    4) run_one inactive ;;
    5) run_one gap ;;
    6) run_one agent-task ;;
    7) run_one verify ;;
    a|A|all) run_all ;;
    q|Q|"") info "終了"; return 0 ;;
    *) fail "不正な選択: $choice" ;;
  esac
}

main() {
  local cmd="${1:-}"
  case "$cmd" in
    "" ) menu ;;
    help|-h|--help) usage ;;
    list|ls) list_scenarios ;;
    all)
      preflight
      run_all
      ;;
    done|claim|no-opt-in|inactive|gap|agent-task|verify)
      preflight
      run_one "$cmd"
      ;;
    *)
      usage >&2
      fail "unknown argument: $cmd"
      ;;
  esac
}

main "$@"
