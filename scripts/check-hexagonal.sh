#!/usr/bin/env bash
# クレート内の六角形レイヤー依存を静的に検査する。
# 正本: docs/architecture.md の Hexagonal 節。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

failures=0

fail() {
  echo "HEXAGONAL FAIL: $*" >&2
  failures=$((failures + 1))
}

note() {
  echo "HEXAGONAL: $*"
}

# レイヤー L の .rs が forbidden パターンにマッチする use を含むか（行番号付き）
layer_has_forbidden_use() {
  local dir="$1"
  shift
  local patterns=("$@")
  local f line

  [[ -d "$dir" ]] || return 0

  while IFS= read -r -d '' f; do
    for pattern in "${patterns[@]}"; do
      while IFS= read -r line; do
        fail "${f#"$ROOT"/}: ${line}  (${pattern})"
      done < <(grep -nE "^[[:space:]]*use[[:space:]].*${pattern}" "$f" 2>/dev/null || true)
    done
  done < <(find "$dir" -name '*.rs' -print0 2>/dev/null)
}

# application から adapters への use（composition root を除く）
check_application_layer() {
  local crate="$1"
  shift
  local allowed_basenames=("$@")
  local app_dir="$ROOT/$crate/src/application"
  local f base allowed skip

  [[ -d "$app_dir" ]] || return 0

  while IFS= read -r -d '' f; do
    base=$(basename "$f")
    skip=0
    for allowed in "${allowed_basenames[@]}"; do
      if [[ "$base" == "$allowed" ]]; then
        skip=1
        break
      fi
    done
    [[ "$skip" -eq 1 ]] && continue

    while IFS= read -r line; do
      fail "${f#"$ROOT"/}: ${line}  (application must not use adapters; wire in composition root only)"
    done < <(grep -nE '^[[:space:]]*use[[:space:]].*::adapters::' "$f" 2>/dev/null || true)
  done < <(find "$app_dir" -name '*.rs' -print0 2>/dev/null)
}

# adapters が application を参照していないか
check_adapters_no_application() {
  local crate="$1"
  local adapters_dir="$ROOT/$crate/src/adapters"
  local f line

  [[ -d "$adapters_dir" ]] || return 0

  while IFS= read -r -d '' f; do
    while IFS= read -r line; do
      fail "${f#"$ROOT"/}: ${line}  (adapters must not use application)"
    done < <(grep -nE '^[[:space:]]*use[[:space:]].*::application::' "$f" 2>/dev/null || true)
  done < <(find "$adapters_dir" -name '*.rs' -print0 2>/dev/null)
}

check_crate() {
  local crate="$1"
  shift
  local -a app_allow=("$@")

  note "checking $crate layers..."

  layer_has_forbidden_use "$ROOT/$crate/src/domain" '::adapters::' '::application::'
  layer_has_forbidden_use "$ROOT/$crate/src/ports" '::adapters::' '::application::'
  check_application_layer "$crate" "${app_allow[@]}"
  check_adapters_no_application "$crate"
}

note "hexagonal layer dependency checks (aibe, aish, ai)..."

# composition root: 唯一 adapters を組み立ててよい application ファイル（basename）
check_crate aibe server.rs
check_crate aish
check_crate ai

if [[ "$failures" -gt 0 ]]; then
  echo "HEXAGONAL: $failures check(s) failed" >&2
  echo "HEXAGONAL: see docs/architecture.md (Hexagonal)" >&2
  exit 1
fi

note "all hexagonal checks passed"
exit 0
