#!/usr/bin/env bash
# ローカル・CI 共通の品質ゲート（fmt / clippy / test / 静的検査）。
#
# 進捗は行単位で即時表示する。`| tail` / `| head` で包むと完了まで無出力に見えるので使わない。
#
# 環境変数:
#   VERIFY_SKIP_TEST=1     テストを省略（fmt / clippy / 静的検査のみ）
#   VERIFY_PARALLEL=1      ビルド・テストを並列化（RAM 余裕時のみ）
#   VERIFY_PACKAGES=ai     テスト対象クレートを限定（clippy は workspace 全体のまま）
#   VERIFY_PROGRESS=1      進捗を .verify-progress へ追記（別 terminal で tail -f 可）
#   VERIFY_PROGRESS_FILE=… 進捗ファイルのパスを明示
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# shellcheck source=scripts/verify-common.sh
source "$ROOT/scripts/verify-common.sh"

VERIFY_START_SEC=$SECONDS
verify_init_progress_file

# メモリ不足環境では並列ビルド・並列テストを避ける（既定: 直列）。
# 十分な RAM があるマシンだけ VERIFY_PARALLEL=1 で並列に戻せる。
if [[ "${VERIFY_PARALLEL:-0}" != "1" ]]; then
  export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"
  VERIFY_TEST_JOBS=1
else
  VERIFY_TEST_JOBS="${VERIFY_TEST_JOBS:-}"
fi

if [[ -n "${VERIFY_PACKAGES:-}" ]]; then
  verify_validate_packages "$VERIFY_PACKAGES"
  verify_progress "scoped tests: $VERIFY_PACKAGES (full gate still runs fmt/clippy/static checks)"
fi

run() {
  verify_run "$@"
}

run cargo fmt --all -- --check
run ./scripts/test-verify-targeted.sh
run cargo clippy --workspace -- -D warnings

# aibe-client 統合テストが spawn するバイナリ（workspace と同じ target を使う）
run cargo build -p aibe -q

if [[ "${VERIFY_SKIP_TEST:-0}" == "1" ]]; then
  verify_progress "skipping cargo test (VERIFY_SKIP_TEST=1)"
elif [[ -n "${VERIFY_PACKAGES:-}" ]]; then
  for pkg in $VERIFY_PACKAGES; do
    if [[ "$pkg" == "aibe-client" ]]; then
      if [[ -n "${VERIFY_TEST_JOBS:-}" ]]; then
        run cargo test -p aibe-client -j "${VERIFY_TEST_JOBS}" -- --test-threads=1
      else
        run cargo test -p aibe-client -- --test-threads=1
      fi
    elif [[ -n "${VERIFY_TEST_JOBS:-}" ]]; then
      run cargo test -p "$pkg" -j "${VERIFY_TEST_JOBS}" -- --test-threads=1
    else
      run cargo test -p "$pkg" -- --test-threads=1
    fi
  done
else
  # aibe-client は mock aibe 起動のため直列実行（並列だと socket / プロセスが競合しうる）
  if [[ -n "${VERIFY_TEST_JOBS:-}" ]]; then
    run cargo test --workspace --exclude aibe-client -j "${VERIFY_TEST_JOBS}" -- --test-threads=1
    run cargo test -p aibe-client -j "${VERIFY_TEST_JOBS}" -- --test-threads=1
  else
    run cargo test --workspace --exclude aibe-client -- --test-threads=1
    run cargo test -p aibe-client -- --test-threads=1
  fi
fi

run ./scripts/check-architecture.sh
run ./scripts/check-docs-consistency.sh
run ./scripts/check-spec-acceptance.py
run ./scripts/check-codex-tooling.sh

verify_print_total "verify"
echo "verify: all checks passed"
