#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
export VERIFY_TARGETED_TEST_LOG="$TMP/commands.log"

for command in cargo docs-check architecture-check codex-check; do
  ln -s "$ROOT/scripts/tests/fixtures/record-command.sh" "$TMP/$command"
done

export VERIFY_TARGETED_CARGO="$TMP/cargo"
export VERIFY_TARGETED_DOCS_CHECK="$TMP/docs-check"
export VERIFY_TARGETED_ARCHITECTURE_CHECK="$TMP/architecture-check"
export VERIFY_TARGETED_CODEX_CHECK="$TMP/codex-check"

reset_log() {
  : > "$VERIFY_TARGETED_TEST_LOG"
}

assert_log() {
  local expected="$1"
  if [[ "$(cat "$VERIFY_TARGETED_TEST_LOG")" != "$expected" ]]; then
    echo "unexpected command log:" >&2
    cat "$VERIFY_TARGETED_TEST_LOG" >&2
    echo "expected:" >&2
    printf '%s\n' "$expected" >&2
    exit 1
  fi
}

reset_log
./scripts/verify-targeted.sh --package aibe --test agent_turn_loop >/dev/null
assert_log $'cargo fmt --all -- --check\ncargo clippy -p aibe -- -D warnings\ncargo test -p aibe --test agent_turn_loop -j 1'

reset_log
./scripts/verify-targeted.sh --package ai --test ask_integration --test history_cli >/dev/null
assert_log $'cargo fmt --all -- --check\ncargo clippy -p ai -- -D warnings\ncargo test -p ai --test ask_integration -j 1\ncargo test -p ai --test history_cli -j 1'

reset_log
./scripts/verify-targeted.sh --package aibe-client >/dev/null
assert_log $'cargo fmt --all -- --check\ncargo clippy -p aibe-client -- -D warnings\ncargo build -p aibe -q\ncargo test -p aibe-client -j 1 -- --test-threads=1'

reset_log
./scripts/verify-targeted.sh --docs --architecture --codex-tooling >/dev/null
assert_log $'docs-check\narchitecture-check\ncodex-check'

reset_log
if ./scripts/verify-targeted.sh --package unknown >/dev/null 2>&1; then
  echo "unknown package unexpectedly succeeded" >&2
  exit 1
fi
assert_log ''

if ./scripts/verify-targeted.sh >/dev/null 2>&1; then
  echo "empty invocation unexpectedly succeeded" >&2
  exit 1
fi

if ./scripts/verify-targeted.sh --test agent_turn_loop >/dev/null 2>&1; then
  echo "--test without --package unexpectedly succeeded" >&2
  exit 1
fi

if ./scripts/verify-targeted.sh --package aibe --test '../escape' >/dev/null 2>&1; then
  echo "invalid integration test name unexpectedly succeeded" >&2
  exit 1
fi

echo "test-verify-targeted: ok"
