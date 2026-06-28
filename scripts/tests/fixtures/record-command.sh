#!/usr/bin/env bash
set -euo pipefail
: "${VERIFY_TARGETED_TEST_LOG:?}"
printf '%s' "$(basename "$0")" >> "$VERIFY_TARGETED_TEST_LOG"
if [[ $# -gt 0 ]]; then
  printf ' %s' "$@" >> "$VERIFY_TARGETED_TEST_LOG"
fi
printf '\n' >> "$VERIFY_TARGETED_TEST_LOG"
