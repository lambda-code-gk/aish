#!/usr/bin/env bash
# mock aibe + ai ask の end-to-end 導通スモーク（実 API 不要）。
# 仕様: docs/done/0014_ci-smoke-stabilization-spec.md
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

SMOKE_DIR=""
AIBE_PID=""

cleanup() {
  if [[ -n "${AIBE_PID:-}" ]] && kill -0 "$AIBE_PID" 2>/dev/null; then
    kill "$AIBE_PID" 2>/dev/null || true
    wait "$AIBE_PID" 2>/dev/null || true
  fi
  if [[ -n "${SMOKE_DIR:-}" && -d "$SMOKE_DIR" ]]; then
    rm -rf "$SMOKE_DIR"
  fi
}
trap cleanup EXIT

fail() {
  echo "smoke-mock: $*" >&2
  exit 1
}

# 非空行のみを配列に読み、行数と各行の内容を厳密に検証する。
assert_nonempty_lines_exact() {
  local label="$1"
  local file="$2"
  shift 2
  local -a expected=("$@")
  local -a actual=()
  local line

  while IFS= read -r line || [[ -n "$line" ]]; do
    [[ -z "$line" ]] && continue
    actual+=("$line")
  done <"$file"

  if [[ ${#actual[@]} -ne ${#expected[@]} ]]; then
    echo "smoke-mock: unexpected $label (${#actual[@]} non-empty lines, want ${#expected[@]}):" >&2
    cat "$file" >&2
    fail "$label line count mismatch"
  fi

  local i
  for i in "${!expected[@]}"; do
    if [[ "${actual[$i]}" != "${expected[$i]}" ]]; then
      echo "smoke-mock: unexpected $label line $((i + 1)):" >&2
      printf '  got:  %q\n' "${actual[$i]}" >&2
      printf '  want: %q\n' "${expected[$i]}" >&2
      cat "$file" >&2
      fail "$label content mismatch"
    fi
  done
}

SMOKE_DIR="$(mktemp -d)"
export AIBE_CONFIG="$SMOKE_DIR/aibe.toml"
export AIBE_SOCKET_PATH="$SMOKE_DIR/aibe.sock"
export AI_CONFIG="$SMOKE_DIR/ai.toml"

cat >"$AIBE_CONFIG" <<'EOF'
[llm]
provider = "mock"
EOF

cat >"$AI_CONFIG" <<'EOF'
# [ask] 省略 → tools []
EOF

rm -f "$AIBE_SOCKET_PATH"

echo "smoke-mock: building aibe and ai..."
cargo build -q -p aibe -p ai

AIBE_BIN="$ROOT/target/debug/aibe"
AI_BIN="$ROOT/target/debug/ai"
[[ -x "$AIBE_BIN" ]] || fail "missing $AIBE_BIN"
[[ -x "$AI_BIN" ]] || fail "missing $AI_BIN"

echo "smoke-mock: starting mock aibe (foreground)..."
"$AIBE_BIN" -f &
AIBE_PID=$!

ready=0
for _ in $(seq 1 100); do
  if [[ -S "$AIBE_SOCKET_PATH" ]]; then
    ready=1
    break
  fi
  if ! kill -0 "$AIBE_PID" 2>/dev/null; then
    fail "aibe exited before socket was ready"
  fi
  sleep 0.1
done
[[ "$ready" -eq 1 ]] || fail "timed out waiting for socket $AIBE_SOCKET_PATH"

STDOUT_FILE="$SMOKE_DIR/stdout"
STDERR_FILE="$SMOKE_DIR/stderr"

echo "smoke-mock: ai ask round-trip..."
set +e
timeout 180s "$AI_BIN" ask "ping" --socket "$AIBE_SOCKET_PATH" --no-start \
  >"$STDOUT_FILE" 2>"$STDERR_FILE"
ask_status=$?
set -e

if [[ "$ask_status" -eq 124 ]]; then
  fail "ai ask timed out (GNU timeout exit 124)"
fi
[[ "$ask_status" -eq 0 ]] || fail "ai ask failed with exit $ask_status"

if grep -q '^warning:' "$STDERR_FILE"; then
  echo "smoke-mock: unexpected stderr (warning prefix):" >&2
  cat "$STDERR_FILE" >&2
  fail "stderr must not contain warning: prefix"
fi

assert_nonempty_lines_exact stdout "$STDOUT_FILE" '[mock] received: ping'
assert_nonempty_lines_exact stderr "$STDERR_FILE" 'ai: tools enabled: none'

echo "smoke-mock: ok"
