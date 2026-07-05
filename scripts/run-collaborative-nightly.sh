#!/usr/bin/env bash
# 0055 Phase 6: nightly / 手動の縦切り統合テスト（#[ignore] 付き pending AC）。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"

echo "==> building aish + ai for collaborative vertical slices"
cargo build -p aish -p ai -q

echo "==> collaborative vertical slice nightly tests (including #[ignore])"
cargo test -p ai --test 0055_collaborative_vertical_slice -j 1 -- --test-threads=1 --include-ignored

echo "run-collaborative-nightly: ok"
