#!/usr/bin/env bash
# クレート境界と禁止依存を静的に検査する。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

failures=0

fail() {
  echo "ARCHITECTURE FAIL: $*" >&2
  failures=$((failures + 1))
}

note() {
  echo "ARCHITECTURE: $*"
}

# Cargo.toml にパターンが含まれるか（コメント行は無視しないが、現状の小規模 Cargo では十分）
toml_has() {
  local file="$1"
  local pattern="$2"
  grep -qE "$pattern" "$file" 2>/dev/null
}

check_forbidden_deps() {
  local crate="$1"
  local toml="$ROOT/$crate/Cargo.toml"
  shift
  local forbidden=("$@")

  if [[ ! -f "$toml" ]]; then
    fail "missing $toml"
    return
  fi

  for dep in "${forbidden[@]}"; do
    # path/workspace 依存: name =  / name =
    if toml_has "$toml" "^[[:space:]]*${dep}[[:space:]]*=[[:space:]]*" ||
      toml_has "$toml" "^[[:space:]]*${dep}[[:space:]]*=[[:space:]]*\\{"; then
      fail "$crate must not depend on '$dep' (see docs/architecture.md)"
    fi
  done
}

# クレート内レイヤー（domain / ports / application / adapters）
if [[ -x "$ROOT/scripts/check-hexagonal.sh" ]]; then
  "$ROOT/scripts/check-hexagonal.sh"
else
  fail "missing or non-executable scripts/check-hexagonal.sh"
fi

note "checking Cargo.toml boundaries..."

# aish → aibe 禁止
if toml_has "$ROOT/aish/Cargo.toml" '^[[:space:]]*aibe[[:space:]]*='; then
  fail "aish must not depend on aibe"
fi

# aibe → aish 禁止
if toml_has "$ROOT/aibe/Cargo.toml" '^[[:space:]]*aish[[:space:]]*='; then
  fail "aibe must not depend on aish"
fi

# ai → aish 禁止（ログはファイル経由。クレート依存で結合しない）
if toml_has "$ROOT/ai/Cargo.toml" '^[[:space:]]*aish[[:space:]]*='; then
  fail "ai must not depend on aish (read logs via paths/API, not crate coupling)"
fi

# ai → aibe 本体禁止（0017: protocol + client のみ）
if toml_has "$ROOT/ai/Cargo.toml" '^[[:space:]]*aibe[[:space:]]*='; then
  fail "ai must not depend on aibe crate (use aibe-protocol and aibe-client)"
fi

# split crate 境界（0017）
check_forbidden_deps aibe-protocol aibe aibe-client aish ai
check_forbidden_deps aibe-client aibe aish ai

note "checking ai sources for direct aibe crate references..."
while IFS= read -r -d '' f; do
  if grep -qE '\baibe::|use[[:space:]]+aibe[[:space:]]*;' "$f"; then
    fail "ai must not reference aibe crate in $f (use aibe_protocol / aibe_client)"
  fi
done < <(find "$ROOT/ai/src" "$ROOT/ai/tests" -name '*.rs' -print0 2>/dev/null)

# HTTP / LLM SDK — aibe のみ許容（現状は未使用だが将来用）
LLM_HTTP_DEPS=(
  reqwest
  hyper
  ureq
  isahc
  surf
  awc
  async-openai
  openai
  rig-core
  genai
  google-generative-ai-rs
  gemini-rust
)

check_forbidden_deps aish "${LLM_HTTP_DEPS[@]}"
check_forbidden_deps ai "${LLM_HTTP_DEPS[@]}"

# ソース内の LLM 直叩きの匂い（ai / aish）
note "checking forbidden patterns in ai/ and aish/ sources..."
while IFS= read -r -d '' f; do
  if grep -qE 'api\.openai\.com|generativelanguage\.googleapis\.com|OPENAI_API_KEY' "$f"; then
    fail "forbidden LLM endpoint or key pattern in $f"
  fi
done < <(find "$ROOT/ai/src" "$ROOT/aish/src" -name '*.rs' -print0 2>/dev/null)

# aibe に API キー直書き（簡易）
note "checking aibe sources for inline API keys..."
while IFS= read -r -d '' f; do
  if grep -qE 'sk-[a-zA-Z0-9]{10,}|AIza[0-9A-Za-z_-]{10,}' "$f"; then
    fail "possible inline API key in $f"
  fi
done < <(find "$ROOT/aibe/src" -name '*.rs' -print0 2>/dev/null)

# ツールの外部プロセス: timeout + cmd.output() 直叩き禁止（run_subprocess 経由）
note "checking tool subprocess policy (run_subprocess)..."
TOOLS_DIR="$ROOT/aibe/src/adapters/outbound/tools"
if [[ -d "$TOOLS_DIR" ]]; then
  while IFS= read -r -d '' f; do
    case "$(basename "$f")" in
      subprocess.rs) continue ;;
    esac
    if grep -qE 'timeout[[:space:]]*\([^)]*\.output\(\)' "$f"; then
      fail "use run_subprocess instead of timeout(..., cmd.output()) in ${f#"$ROOT/"}"
    fi
  done < <(find "$TOOLS_DIR" -name '*.rs' -print0 2>/dev/null)
fi

if [[ "$failures" -gt 0 ]]; then
  echo "ARCHITECTURE: $failures check(s) failed" >&2
  exit 1
fi

note "all checks passed"
exit 0
