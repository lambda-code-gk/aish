#!/usr/bin/env bash
# Codex MCP 用コンテキストパケット（オプション）。既定は codex-mcp-prompt.sh のみで Codex が repo 内を自律調査。
#
# 目的:
#   - 親が diff / 抜粋を先に渡してコンテキストを絞る（CODEX_USE_PACKET=1）
#   - Codex（別 LLM）への監査・仕様・調査の補助
#
# CODEX_TASK:
#   spec    — 仕様ドラフト（docs / 境界 / セキュリティを同梱）
#   review  — 実装後レビュー（git diff + 抜粋）
#   audit   — 横断監査（境界・セキュリティ・設計整合。diff 任意）
#   spike   — 限定調査（focus パスのみ）
#
# レビュー詳細: CODEX_REVIEW_MODE=fast|standard|deep（review / audit で diff があるとき）
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

CHANGED=()
TASK="${CODEX_TASK:-review}"
REVIEW_MODE="${CODEX_REVIEW_MODE:-standard}"
MAX_DIFF_LINES="${CODEX_REVIEW_MAX_DIFF_LINES:-800}"
MAX_SNIPPET_LINES="${CODEX_REVIEW_MAX_SNIPPET_LINES:-120}"
DEEP_CONTEXT_LINES="${CODEX_REVIEW_DEEP_CONTEXT_LINES:-40}"
MAX_DOC_LINES="${CODEX_MAX_DOC_LINES:-200}"
FOCUS_PATHS="${CODEX_FOCUS_PATHS:-}"

emit_header() {
  echo "## Codex パケット"
  echo "- task: ${TASK}"
  echo "- review_mode: ${REVIEW_MODE}"
  echo "- generated: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo
  echo "親エージェント（MCP・オプション）:"
  echo "  既定: タスク文 + ./scripts/codex-mcp-prompt.sh（パケットなし）→ Codex が repo 内を自律調査"
  echo "  親が diff を絞りたいときのみ: CODEX_USE_PACKET=1 CODEX_TASK=${TASK} ./scripts/codex-mcp-prompt.sh"
  echo "  続き: codex-reply + threadId。Codex 返答は Cursor に要約のみ"
  echo
}

emit_static_docs() {
  echo "## 静的参照（リポジトリ正本の抜粋）"
  echo
  for doc in docs/architecture.md docs/security.md; do
    [[ -f "$doc" ]] || continue
    echo "### ${doc}"
    echo "~~~"
    head -n "$MAX_DOC_LINES" "$doc"
    echo "~~~"
    echo
  done
  echo "### クレート境界 (.cursor/rules/10-boundaries.mdc)"
  echo "~~~"
  head -n 80 .cursor/rules/10-boundaries.mdc 2>/dev/null || true
  echo "~~~"
  echo
}

emit_focus_paths() {
  local paths=()
  if [[ -n "$FOCUS_PATHS" ]]; then
    IFS=',' read -ra paths <<<"$FOCUS_PATHS"
  fi
  if ((${#paths[@]} == 0)) && [[ -n "${CHANGED:-}" ]]; then
    paths=("${CHANGED[@]}")
  fi
  ((${#paths[@]} > 0)) || return 0
  echo "## フォーカスパス（許可される追加読取の範囲）"
  printf '%s\n' "${paths[@]}"
  echo
  echo "## フォーカス抜粋"
  local path max=80
  [[ "$TASK" == "spike" || "$REVIEW_MODE" == "deep" ]] && max=160
  for path in "${paths[@]}"; do
    path="${path#"${path%%[![:space:]]*}"}"
    path="${path%"${path##*[![:space:]]}"}"
    [[ -n "$path" && -f "$path" ]] || continue
    echo
    echo "### ${path}"
    echo "~~~"
    local line_count
    line_count="$(wc -l <"$path" | tr -d ' ')"
    if ((line_count <= max)); then
      cat "$path"
    else
      head -n "$max" "$path"
      echo "... (truncated, ${line_count} lines total)"
    fi
    echo "~~~"
  done
  echo
}

emit_git_diff() {
  echo "## レビュー対象（git）"
  echo
  git status -sb 2>/dev/null || true
  echo
  if ! git diff --quiet HEAD 2>/dev/null; then
    echo "### diff --stat"
    git diff --stat HEAD 2>/dev/null || true
    echo
    echo "### diff（先頭 ${MAX_DIFF_LINES} 行）"
    git diff HEAD 2>/dev/null | head -n "$MAX_DIFF_LINES" || true
    true
    mapfile -t CHANGED < <(git diff --name-only HEAD 2>/dev/null || true)
  else
    mapfile -t CHANGED < <(git ls-files 2>/dev/null | head -n 0 || true)
    echo "(作業ツリーに未コミット diff なし)"
  fi
  echo
}

emit_review_snippets() {
  [[ "$TASK" == "review" || "$TASK" == "audit" ]] || return 0
  [[ "$REVIEW_MODE" == "fast" ]] && return 0
  [[ ${#CHANGED[@]} -gt 0 ]] || return 0

  echo "## 変更ファイル（レビュー許可パス）"
  printf '%s\n' "${CHANGED[@]}"
  echo
  echo "## 変更ファイルの抜粋"
  local context_lines=15 max_per_file=$MAX_SNIPPET_LINES
  [[ "$REVIEW_MODE" == "deep" ]] && context_lines=$DEEP_CONTEXT_LINES && max_per_file=$((MAX_SNIPPET_LINES * 2))
  for path in "${CHANGED[@]}"; do
    [[ -f "$path" ]] || continue
    echo
    echo "### ${path}"
    echo "~~~"
    local line_count
    line_count="$(wc -l <"$path" | tr -d ' ')"
    if ((line_count <= max_per_file)); then
      cat "$path"
    else
      git diff HEAD "--unified=${context_lines}" -- "$path" 2>/dev/null | head -n "$max_per_file" || true
    fi
    echo "~~~"
  done
  echo
}

emit_arch_check() {
  echo "## 境界チェック（機械）"
  if [[ -x ./scripts/check-architecture.sh ]]; then
    ./scripts/check-architecture.sh 2>&1 || true
  else
    echo "(check-architecture.sh なし)"
  fi
  echo
}

emit_task_footer() {
  case "$TASK" in
    spec)
      echo "## 期待する Codex 出力（spec）"
      echo "- 目的 / スコープ外"
      echo "- 受け入れ条件（検証可能）"
      echo "- 影響クレート・プロトコル・設定"
      echo "- テスト方針・セキュリティ・未確定（推測と明記）"
      ;;
    review)
      echo "## 期待する Codex 出力（review）"
      echo "- 重大 / 中 / 低、通過項目、要確認"
      echo "- 受け入れ条件との対応"
      ;;
    audit)
      echo "## 期待する Codex 出力（audit）"
      echo "- 境界違反・セキュリティ・保守性の指摘（重大度付き）"
      echo "- ドキュメントと実装のずれ"
      ;;
    spike)
      echo "## 期待する Codex 出力（spike）"
      echo "- 調査結果・選択肢比較・推奨（未確定は明示）"
      ;;
  esac
}

emit_header

case "$TASK" in
  spec)
    emit_static_docs
    emit_focus_paths
    emit_task_footer
    ;;
  review)
    emit_git_diff
    emit_review_snippets
    emit_arch_check
    emit_task_footer
    ;;
  audit)
    emit_static_docs
    emit_git_diff
    emit_review_snippets
    emit_arch_check
    emit_focus_paths
    emit_task_footer
    ;;
  spike)
    emit_focus_paths
    emit_task_footer
    ;;
  *)
    echo "Unknown CODEX_TASK=${TASK} (spec|review|audit|spike)" >&2
    exit 2
    ;;
esac
