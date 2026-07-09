#!/usr/bin/env bash
# ローカル・CI 共通の品質ゲート（fmt / clippy / test / 静的検査）。
#
# 進捗は行単位で即時表示する（`| tail` 等で包むと完了まで無出力に見える）。
# 各ステップと終了時に経過時間を必ず表示する（成功・失敗どちらでも EXIT trap でサマリー出力）。
# サマリーは `.verify-timing-last` にも書き出す（AI エージェントはこの小ファイルだけ読めばよい）。
# 静的検査のみ: VERIFY_SKIP_TEST=1 ./scripts/verify.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

VERIFY_STARTED_AT=$SECONDS
VERIFY_STEP_NAMES=()
VERIFY_STEP_ELAPSED=()
VERIFY_TIMING_REPORTED=0
VERIFY_FAILED_STEP=""

format_elapsed() {
  local elapsed="$1"
  printf '%dm%02ds' $((elapsed / 60)) $((elapsed % 60))
}

record_verify_step() {
  local label="$1"
  local elapsed="$2"
  local status="$3"
  VERIFY_STEP_NAMES+=("$label")
  VERIFY_STEP_ELAPSED+=("$elapsed")
  if [[ "$status" -ne 0 ]]; then
    VERIFY_FAILED_STEP="$label"
    printf '    failed after %s\n' "$(format_elapsed "$elapsed")"
    return "$status"
  fi
  printf '    done in %s\n' "$(format_elapsed "$elapsed")"
  return 0
}

report_verify_timing() {
  if [[ "${VERIFY_TIMING_REPORTED}" -eq 1 ]]; then
    return 0
  fi
  VERIFY_TIMING_REPORTED=1

  local wall_elapsed=$((SECONDS - VERIFY_STARTED_AT))
  local timing_file="${VERIFY_TIMING_FILE:-$ROOT/.verify-timing-last}"

  {
    echo
    echo "verify timing summary:"
    if [[ "${#VERIFY_STEP_NAMES[@]}" -eq 0 ]]; then
      printf '  %-52s %s\n' "(no steps recorded)" "$(format_elapsed "$wall_elapsed")"
    else
      local total=0
      local i
      for i in "${!VERIFY_STEP_NAMES[@]}"; do
        local elapsed="${VERIFY_STEP_ELAPSED[$i]}"
        total=$((total + elapsed))
        printf '  %-52s %s\n' "${VERIFY_STEP_NAMES[$i]}" "$(format_elapsed "$elapsed")"
      done
      printf '  %-52s %s\n' "total (steps)" "$(format_elapsed "$total")"
    fi
    printf '  %-52s %s\n' "total (wall clock)" "$(format_elapsed "$wall_elapsed")"
    if [[ -n "${VERIFY_FAILED_STEP}" ]]; then
      echo "verify FAILED at: ${VERIFY_FAILED_STEP}"
    fi
    echo "verify timing file: ${timing_file#"$ROOT"/}"
  } | tee "$timing_file"
}

trap report_verify_timing EXIT

run() {
  local label="$*"
  echo "==> $*"
  local step_start=$SECONDS
  local status=0
  "$@" || status=$?
  local elapsed=$((SECONDS - step_start))
  record_verify_step "$label" "$elapsed" "$status" || exit "$status"
}

record_verify_skip() {
  local label="$1"
  echo "==> $label"
  VERIFY_STEP_NAMES+=("$label")
  VERIFY_STEP_ELAPSED+=(0)
}

# メモリ不足環境では並列ビルド・並列テストを避ける（既定: 直列）。
# 十分な RAM があるマシンだけ VERIFY_PARALLEL=1 で並列に戻せる。
if [[ "${VERIFY_PARALLEL:-0}" != "1" ]]; then
  export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"
  VERIFY_TEST_JOBS=1
else
  VERIFY_TEST_JOBS="${VERIFY_TEST_JOBS:-}"
fi

run cargo fmt --all -- --check
run ./scripts/test-verify-targeted.sh
run cargo clippy --workspace -- -D warnings

# aibe-client 統合テストが spawn するバイナリ（workspace と同じ target を使う）
run cargo build -p aibe -q

if [[ "${VERIFY_SKIP_TEST:-0}" == "1" ]]; then
  record_verify_skip "skipping cargo test (VERIFY_SKIP_TEST=1)"
else
  # aibe-client は mock aibe 起動のため直列実行（並列だと socket / プロセスが競合しうる）
  if [[ -n "${VERIFY_TEST_JOBS:-}" ]]; then
    run cargo test --workspace --exclude aibe-client -j "${VERIFY_TEST_JOBS}" -- --test-threads=1
    run cargo test -p aibe-client -j "${VERIFY_TEST_JOBS}" -- --test-threads=1
  else
    run cargo test --workspace --exclude aibe-client -- --test-threads=1
    run cargo test -p aibe-client -- --test-threads=1
  fi

  if ! command -v zsh >/dev/null 2>&1; then
    VERIFY_FAILED_STEP="zsh availability check"
    echo "verify FAIL: zsh is required for 0055 zsh_supported_in_ci (install zsh package)" >&2
    exit 1
  fi
  run env AISH_0055_ZSH=1 cargo test -p aish --test 0055_minimal_human_handoff zsh_human_return_marker -- --exact
fi

run ./scripts/check-architecture.sh
run ./scripts/check-docs-consistency.sh
run ./scripts/check-feature-scope.py
run ./scripts/check-spec-acceptance.py
run ./scripts/check-minimal-human-handoff-scope.sh
run ./scripts/check-codex-tooling.sh

report_verify_timing
echo "verify: all checks passed"
