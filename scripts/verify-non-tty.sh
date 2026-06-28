#!/usr/bin/env bash
# `./scripts/verify.sh` を非TTY条件で実行するためのラッパー。
#
# TTY 依存のテストを避けたい場合はこのラッパーを使う。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG_FILE="${VERIFY_LOG_FILE:-$ROOT/target/verify-non-tty.log}"

mkdir -p "$(dirname "$LOG_FILE")"
echo "verify-non-tty: capturing output in $LOG_FILE"

exec </dev/null >"$LOG_FILE" 2>&1
cd "$ROOT"
exec ./scripts/verify.sh "$@" &
tail -f $LOG_FILE &
wait
