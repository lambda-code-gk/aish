---
description: 独立CodexレビューとCursor修正を重大・中がなくなるまで繰り返す
---

Codex MCP の **短命な新規 thread** で独立レビューし、Cursor が修正する。重大・中が 0 件になるまで必要な場合だけ再レビューする。

**対象**: 「$ARGUMENTS」  
（未指定なら未コミット差分 `git diff` / `git diff --stat`。ブランチ全体なら `git diff main...HEAD` も可）

## フロー

```text
1. **新規 Codex** に実装レビューを依頼（review / effort low / packet 必須 / 指摘のみ）
2. 指摘を 重大 / 中 / 軽微 に整理して報告
3. 重大・中があれば Cursor 側で修正（軽微だけなら修正判断後 Step 4 へ）
4. ./scripts/verify.sh（必要なら ./scripts/smoke-mock.sh）を実行
5. 重大・中を修正した場合だけ、**別の新規 Codex** に前回指摘要約 + fast packet で再レビュー
6. 重大・中が 0 件（Pass）になるまで 3〜5 を繰り返す。軽微だけなら再レビューしない
7. 最終報告（修正概要・verify 結果・残リスク）
```

## Codex 呼び出し

```bash
{
  CODEX_TASK=review ./scripts/codex-mcp-prompt.sh
  printf '\n%s\n' '（レビュー対象の説明。未指定なら未コミット差分など）'
}
# 推奨 config 確認: CODEX_TASK=review CODEX_PRINT_CONFIG_HINT=1 ./scripts/codex-mcp-prompt.sh
```

MCP `codex` 引数:

| 引数 | 値 |
|------|-----|
| `cwd` | リポジトリルート |
| `sandbox` | `workspace-write` |
| `approval-policy` | `never` |
| `config` | `{"approval_policy":"never","model_reasoning_effort":"low"}` |
| `developer-instructions` | `.cursor/rules/50-codex-subagent.mdc` の共通 + review 骨子（指摘のみ、編集・verify 禁止） |

レビュー出力形式（Codex に必須指定）:

- **重大** / **中** / **軽微**（0 件なら「なし」）
- 各指摘: ファイル・問題・推奨修正
- **Pass**（重大・中が 0 件）または **Fail**

再レビューも **新規 `codex`**。`codex-reply` で実装 thread や前回 review thread を継続しない。前回指摘の短い要約と `CODEX_REVIEW_MODE=fast` の修正 diff packet を渡す。

## 修正後の品質ゲート

```bash
./scripts/verify.sh
./scripts/smoke-mock.sh
```

`| tail` で包まない。失敗時は修正して再実行。

## ルール

- **Pass 判定**: 重大・中が 0 件。軽微だけなら親が対応判断し、品質ゲートで完了
- Codex review はファイル編集・テスト・`verify.sh` を行わない
- 設計変更が必要な指摘は spec / 実装指示書への追記を検討（黙って仕様を変えない）
- `git commit` / `push` はユーザー明示時のみ
- 親（Cursor）が Codex 実行中に同じファイルを競合編集しない

参照:
- `docs/codex-delegation.md`
- `docs/codex-review.md`（`CODEX_USE_PACKET=1` 時）
- `docs/manual/codex-spec-impl-review-loop.md`（Step 5 相当）
