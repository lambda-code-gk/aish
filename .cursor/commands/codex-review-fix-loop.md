---
description: Codexで実装をレビューし、指摘がなくなるまで修正→再レビューを繰り返す
---

Codex MCP で **実装レビュー → 修正 → 再レビュー** を、指摘が 0 件になるまで繰り返してください。

**対象**: 「$ARGUMENTS」  
（未指定なら未コミット差分 `git diff` / `git diff --stat`。ブランチ全体なら `git diff main...HEAD` も可）

## フロー

```text
1. Codex に実装レビューを依頼（review プロファイル相当）
2. 指摘を 重大 / 中 / 軽微 に整理して報告
3. 指摘が 1 件でもあれば Cursor 側で修正
4. ./scripts/verify.sh（必要なら ./scripts/smoke-mock.sh）を実行
5. Codex に再レビュー依頼（codex-reply または新規 thread）
6. 重大・中・軽微 すべて 0 件（Pass）になるまで 3〜5 を繰り返す
7. 最終報告（修正概要・verify 結果・残リスク）
```

## Codex 呼び出し

```bash
{
  CODEX_USE_PACKET=1 CODEX_TASK=review ./scripts/codex-mcp-prompt.sh
  printf '\n%s\n' '（レビュー対象の説明。未指定なら未コミット Smart Preprocessor 差分など）'
}
```

MCP `codex` 引数:

| 引数 | 値 |
|------|-----|
| `cwd` | リポジトリルート |
| `sandbox` | `workspace-write` |
| `approval-policy` | `never` |
| `config` | `{"approval_policy":"never","model_reasoning_effort":"low"}` |
| `developer-instructions` | `.cursor/rules/50-codex-subagent.mdc` の骨子 |

レビュー出力形式（Codex に必須指定）:

- **重大** / **中** / **軽微**（0 件なら「なし」）
- 各指摘: ファイル・問題・推奨修正
- **Pass**（全セクション 0 件）または **Fail**

再レビューは `codex-reply` + 前回の `threadId`。修正内容と `./scripts/verify.sh` 結果を渡す。

## 修正後の品質ゲート

```bash
./scripts/verify.sh
./scripts/smoke-mock.sh
```

`| tail` で包まない。失敗時は修正して再実行。

## ルール

- **Pass 判定**: 重大・中・軽微 すべて 0 件
- 設計変更が必要な指摘は spec / 実装指示書への追記を検討（黙って仕様を変えない）
- `git commit` / `push` はユーザー明示時のみ
- 親（Cursor）が Codex 実行中に同じファイルを競合編集しない

参照:
- `docs/codex-delegation.md`
- `docs/codex-review.md`（`CODEX_USE_PACKET=1` 時）
- `docs/manual/codex-spec-impl-review-loop.md`（Step 5 相当）
