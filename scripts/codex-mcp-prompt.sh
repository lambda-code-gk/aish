#!/usr/bin/env bash
# Codex MCP の prompt 用テキストを stdout に出す。
#
# 既定: サブエージェント向け短いヘッダのみ（タスク本文は親が prompt に続けて渡す）。
# オプション: CODEX_USE_PACKET=1 で codex-context.sh のパケットを同梱（親がコンテキストを絞りたいとき）。
#
# 使い方:
#   ./scripts/codex-mcp-prompt.sh
#   あなたのタスク説明…
#   → 上記を連結して MCP codex の prompt に渡す
#
#   CODEX_USE_PACKET=1 CODEX_TASK=review ./scripts/codex-mcp-prompt.sh
#
# 権限: .codex/config.toml の aish-subagent（cwd 内読書き + workspace_roots）
set -euo pipefail

TASK="${CODEX_TASK:-subagent}"
EXTRA_ROOTS="${CODEX_EXTRA_ROOTS:-}"

cat <<EOF
Role: ${TASK} for aish workspace (Codex subagent). You may read and edit within allowed paths.

## 境界（プロジェクト \`.codex/config.toml\`）

- **cwd**（このリポジトリ）内は **読取・編集可**（\`sandbox_mode = workspace-write\`）。shell / 検索で広く調べてよい。
- **リポジトリ外**は原則触らない（将来 \`workspace_roots\` で明示許可 — 例: \`docs/codex.config.example.toml\`）。
- \`git commit\` / \`git push\` は **ユーザーが明示したときのみ**。
- API キー・実設定をリポジトリに書かない。\`.env\` / credentials は触らない。
- 出力は **日本語**。推測は「推測」と明記。
- ドキュメント（既定）: **設計書** → \`docs/spec/00xx_<topic>-spec.md\`、**実装指示書** → \`docs/tasks/00xx_<topic>-implementation-spec.md\`（設計と**同じ番号**）。完了コミット時に実装指示書のみ \`docs/done/\` へ移動。一覧は \`docs/0000_spec-index.md\`。
EOF

if [[ -n "$EXTRA_ROOTS" ]]; then
  echo
  echo "## 追加許可パス（このターン）"
  echo "$EXTRA_ROOTS" | tr ',' '\n'
fi

if [[ "${CODEX_USE_PACKET:-0}" == "1" ]]; then
  echo
  echo "---"
  echo
  "$(dirname "${BASH_SOURCE[0]}")/codex-context.sh"
fi
