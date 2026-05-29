#!/usr/bin/env bash
# README・仕様索引・testing.md・todo の整合を静的に検査する。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

failures=0

fail() {
  echo "DOCS FAIL: $*" >&2
  failures=$((failures + 1))
}

note() {
  echo "DOCS: $*"
}

# workspace members（Cargo.toml [workspace] members のみ）
readarray -t WORKSPACE_MEMBERS < <(
  awk '/^members = \[/,/\]/ {
    while (match($0, /"[^"]+"/)) {
      print substr($0, RSTART + 1, RLENGTH - 2)
      $0 = substr($0, RSTART + RLENGTH)
    }
  }' Cargo.toml
)

member_count="${#WORKSPACE_MEMBERS[@]}"
if [[ "$member_count" -lt 1 ]]; then
  fail "could not parse workspace members from Cargo.toml"
fi

note "workspace has $member_count crate(s): ${WORKSPACE_MEMBERS[*]}"

# README: 古い 3 クレート表記・依存の向きの誤り
README="$ROOT/README.md"
if [[ ! -f "$README" ]]; then
  fail "missing README.md"
else
  if grep -qE '3[[:space:]]*つのクレート' "$README"; then
    fail "README.md still says '3 つのクレート' (workspace has $member_count)"
  fi
  if grep -qE '^ai[[:space:]]+→[[:space:]]+aibe[[:space:]]+のみ' "$README"; then
    fail "README.md dependency line still says 'ai → aibe のみ' (expected aibe-protocol / aibe-client)"
  fi
  for crate in "${WORKSPACE_MEMBERS[@]}"; do
    if ! grep -q "$crate" "$README"; then
      fail "README.md does not mention workspace crate: $crate"
    fi
  done
fi

# 仕様索引のリンク先が存在する（docs/ 相対）
INDEX="$ROOT/docs/0000_spec-index.md"
if [[ ! -f "$INDEX" ]]; then
  fail "missing docs/0000_spec-index.md"
else
  while IFS= read -r rel; do
    [[ -z "$rel" ]] && continue
    [[ "$rel" == \#* ]] && continue
    # ディレクトリリンク（todo/ 等）はスキップ
    [[ "$rel" == */ ]] && continue
    target="$ROOT/docs/$rel"
    if [[ ! -f "$target" ]]; then
      fail "spec-index link missing file: docs/$rel"
    fi
  done < <(
    grep -oE '\]\(([^)#][^)]*)\)' "$INDEX" | sed 's/^](//;s/)$//' | sort -u
  )
fi

# ルート直下に残った 00xx 指示書（done 移動漏れ）
while IFS= read -r -d '' f; do
  fail "stale spec at docs root (move to docs/done/): ${f#"$ROOT/"}"
done < <(find "$ROOT/docs" -maxdepth 1 -name '[0-9][0-9][0-9][0-9]_*-spec.md' -print0 2>/dev/null)

# testing.md「0018 safe-tools-policy」表の .rs パスが実在する
TESTING="$ROOT/docs/testing.md"
if [[ -f "$TESTING" ]]; then
  in_section=0
  while IFS= read -r line; do
    if [[ "$line" == "### 0018 safe-tools-policy"* ]]; then
      in_section=1
      continue
    fi
    if [[ $in_section -eq 1 && "$line" == "## "* && "$line" != "### "* ]]; then
      break
    fi
    if [[ $in_section -eq 1 ]]; then
      while IFS= read -r path; do
        [[ -z "$path" ]] && continue
        # モジュール内 unit（ファイル自体を参照）
        if [[ ! -f "$ROOT/$path" ]]; then
          fail "testing.md 0018 references missing path: $path"
        fi
      done < <(printf '%s\n' "$line" | grep -oE '\`[^`]+\.rs[^`]*\`' | tr -d '`' | sed 's/（.*//')
    fi
  done < "$TESTING"
fi

# 4 代目レビュー todo: 完了済み Sprint の「着手待ち」が残っていない
REVIEW_DIR="$ROOT/docs/todo/chatgpt-review-4th-gen"
if [[ -d "$REVIEW_DIR" ]]; then
  if grep -rqE 'Sprint 2.*着手待ち' "$REVIEW_DIR" 2>/dev/null; then
    fail "chatgpt-review-4th-gen still marks Sprint 2 (P1) as 着手待ち"
  fi
  if grep -rq '次.*Sprint 2（P1）' "$REVIEW_DIR" 2>/dev/null; then
    fail "chatgpt-review-4th-gen still says next step is Sprint 2 (P1)"
  fi
fi

if [[ "$failures" -gt 0 ]]; then
  echo "DOCS: $failures check(s) failed" >&2
  exit 1
fi

note "all checks passed"
exit 0
