---
description: 実装後の品質ゲート（verify.sh）と差分要約
---

実装・修正後の確認を行ってください。

## 1. 検証（必須）

リポジトリルートで次を実行し、失敗時は修正して再実行する。

```bash
./scripts/verify.sh
```

`verify.sh` は `fmt` / `clippy` / `aibe` ビルド / `test` / `check-architecture.sh` / `check-docs-consistency.sh` を順に実行する。

**注意**: `| tail` で包まない（完了まで無出力に見える）。静的検査のみは `VERIFY_SKIP_TEST=1 ./scripts/verify.sh`。

## 2. 報告

- 実行したコマンドと結果（成功 / 失敗）
- 未コミット差分の要約（何を変えたか）
- 重大な問題が残っていれば列挙。なければ「重大なし」
- 手動検証が必要なら `docs/manual/` の該当手順と未実施の旨

## 3. Git

- `git commit` / `git push` はユーザー明示時のみ
