#!/usr/bin/env bash
# 開発中の短いフィードバックループ用。完了ゲートは ./scripts/verify.sh を使う。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# shellcheck source=scripts/verify-common.sh
source "$ROOT/scripts/verify-common.sh"
VERIFY_START_SEC=$SECONDS
verify_init_progress_file

CARGO="${VERIFY_TARGETED_CARGO:-cargo}"
DOCS_CHECK="${VERIFY_TARGETED_DOCS_CHECK:-./scripts/check-docs-consistency.sh}"
ARCHITECTURE_CHECK="${VERIFY_TARGETED_ARCHITECTURE_CHECK:-./scripts/check-architecture.sh}"
CODEX_CHECK="${VERIFY_TARGETED_CODEX_CHECK:-./scripts/check-codex-tooling.sh}"

usage() {
  cat <<'EOF'
Usage:
  ./scripts/verify-targeted.sh --package <name> [--test <integration-test> ...]
  ./scripts/verify-targeted.sh [--docs] [--architecture] [--codex-tooling]

Options:
  --package <name>  Check and test one workspace package.
  --test <name>     Run only the named Cargo integration test target. Repeatable.
  --docs            Run documentation consistency checks.
  --architecture    Run architecture and effect-boundary checks.
  --codex-tooling   Run Codex integration script checks.
  -h, --help        Show this help.

This command is for development feedback only. Run ./scripts/verify.sh before
reporting a task complete.
EOF
}

fail() {
  echo "verify-targeted: $*" >&2
  exit 2
}

run() {
  verify_run "$@"
}

package=""
tests=()
run_docs=0
run_architecture=0
run_codex=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --package)
      [[ $# -ge 2 ]] || fail "--package requires a value"
      [[ -z "$package" ]] || fail "--package may be specified only once"
      package="$2"
      shift 2
      ;;
    --test)
      [[ $# -ge 2 ]] || fail "--test requires a value"
      tests+=("$2")
      shift 2
      ;;
    --docs)
      run_docs=1
      shift
      ;;
    --architecture)
      run_architecture=1
      shift
      ;;
    --codex-tooling)
      run_codex=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      fail "unknown argument: $1"
      ;;
  esac
done

if [[ -z "$package" && ${#tests[@]} -gt 0 ]]; then
  fail "--test requires --package"
fi

if [[ -z "$package" && "$run_docs" == "0" && "$run_architecture" == "0" && "$run_codex" == "0" ]]; then
  fail "specify --package or at least one static check"
fi

case "$package" in
  ""|aibe|aibe-client|aibe-protocol|aish|aish-replay|ai) ;;
  *) fail "unknown workspace package: $package" ;;
esac

for test_name in "${tests[@]}"; do
  [[ "$test_name" =~ ^[A-Za-z0-9_-]+$ ]] || fail "invalid integration test name: $test_name"
done

export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"

if [[ -n "$package" ]]; then
  run "$CARGO" fmt --all -- --check
  run "$CARGO" clippy -p "$package" -- -D warnings

  if [[ "$package" == "aibe-client" ]]; then
    run "$CARGO" build -p aibe -q
  fi

  if [[ ${#tests[@]} -eq 0 ]]; then
    test_command=("$CARGO" test -p "$package" -j 1)
    if [[ "$package" == "aibe-client" ]]; then
      test_command+=(-- --test-threads=1)
    fi
    run "${test_command[@]}"
  else
    for test_name in "${tests[@]}"; do
      test_command=("$CARGO" test -p "$package" --test "$test_name" -j 1)
      if [[ "$package" == "aibe-client" ]]; then
        test_command+=(-- --test-threads=1)
      fi
      run "${test_command[@]}"
    done
  fi
fi

if [[ "$run_docs" == "1" ]]; then
  run "$DOCS_CHECK"
fi
if [[ "$run_architecture" == "1" ]]; then
  run "$ARCHITECTURE_CHECK"
fi
if [[ "$run_codex" == "1" ]]; then
  run "$CODEX_CHECK"
fi

verify_print_total "verify-targeted"
echo "verify-targeted: selected checks passed; run ./scripts/verify.sh before completion"
