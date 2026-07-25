# Codex 独立レビュー

Codex レビューは、実装履歴を持たない **短命な新規 thread** で行う。Cursor 親のコンテキストを節約するため、差分はスクリプト側で packet 化し、親は差分全文を会話へ取り込まない。

全体: [codex-delegation.md](./codex-delegation.md)

## 固定契約

- MCP は毎回新規 `codex`。実装 thread や前回 review の `threadId` を `codex-reply` しない
- `CODEX_TASK=review` は packet を既定で同梱する
- Codex は **指摘のみ**返し、ファイル編集・テスト・`verify.sh` を行わない
- 親は重大 / 中 / 軽微の指摘一覧だけ保持する
- Pass は **重大・中が 0 件**。軽微だけなら親が修正判断と品質ゲートを行い、再レビューしない

## パケット生成

```bash
CODEX_TASK=review ./scripts/codex-mcp-prompt.sh
# または
CODEX_USE_PACKET=1 CODEX_TASK=review ./scripts/codex-context.sh
```

`codex-mcp-prompt.sh` は review 時に packet を既定で同梱する。出力の **後ろに短いタスク文**（目的、正本、重点観点）を足し、新規 MCP `codex` の prompt に渡す。親は packet の diff / 抜粋を再読込しない。

## レビューモード（`CODEX_REVIEW_MODE`）

`codex-context.sh` 内での抜粋量のみ変わる。Codex は **引き続き repo 内を読んでよい**（permission profile の範囲内）。

| モード | パケット |
|--------|----------|
| `fast` | diff + arch-check |
| `standard`（既定） | + 変更ファイル抜粋 |
| `deep` | + 広い抜粋 |

```bash
CODEX_REVIEW_MODE=deep CODEX_USE_PACKET=1 CODEX_TASK=review ./scripts/codex-mcp-prompt.sh
```

## 再レビュー

重大・中の修正後だけ再レビューする。前回 thread は継続せず、**新規 `codex`** に前回指摘の短い要約と修正 diff を渡す。

```bash
CODEX_REVIEW_MODE=fast CODEX_TASK=review ./scripts/codex-mcp-prompt.sh
```

## 出力形式

- **重大** / **中** / **軽微**
- 各指摘: 分類タグ、ファイル、問題、根拠、推奨修正
- **Pass**: 重大・中が 0 件
- **Fail**: 重大または中が 1 件以上

## 禁止

- リポジトリ外への無許可アクセス（`workspace_roots` 外）
- 実 API キーのコミット
- review thread でのファイル編集、テスト、`verify.sh`
- 実装 thread / 前回 review thread の `codex-reply`
- Codex の全文や packet の diff 全文を Cursor 親の履歴へ複製

## レビュー指摘の分類（0056 以降）

各指摘に次のいずれかを付ける。

| 分類 | 現在の PR 修正に含めてよいか |
|------|---------------------------|
| `BLOCKER_ORIGINAL_AC` | はい |
| `REGRESSION` | はい |
| `SAFETY_WITHIN_FAULT_MODEL` | はい |
| `NEW_REQUIREMENT` | いいえ（別 spec 候補） |
| `HARDENING` | いいえ |
| `OUT_OF_FAULT_MODEL` | いいえ |

詳細: [feature-development-policy.md](./feature-development-policy.md)
