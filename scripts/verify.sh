#!/usr/bin/env bash
# ローカル・CI 共通の品質ゲート（fmt / clippy / test / 静的検査）。
#
# 進捗は行単位で即時表示する（`| tail` 等で包むと完了まで無出力に見える）。
# 静的検査のみ: VERIFY_SKIP_TEST=1 ./scripts/verify.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

run() {
  echo "==> $*"
  "$@"
}

run cargo fmt --all -- --check
run cargo clippy --workspace -- -D warnings

# aibe-client 統合テストが spawn するバイナリ（workspace と同じ target を使う）
run cargo build -p aibe -q

if [[ "${VERIFY_SKIP_TEST:-0}" == "1" ]]; then
  echo "==> skipping cargo test (VERIFY_SKIP_TEST=1)"
else
  # aibe-client は mock aibe 起動のため直列実行（並列だと socket / プロセスが競合しうる）
  run cargo test --workspace --exclude aibe-client
  run cargo test -p aibe-client -- --test-threads=1
fi

run ./scripts/check-architecture.sh
run ./scripts/check-docs-consistency.sh

echo "verify: all checks passed"
