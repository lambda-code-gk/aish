---
description: Codexの指示書→実装→レビューの7ステップを実行
---

次の 7 ステップをこの順で実行してください。

**対象タスク**: 「$ARGUMENTS」  
（未指定なら `docs/0000_spec-index.md` の進行中指示書、またはユーザーが開いている `docs/00xx_*` / `docs/done/00xx_*` を確認して確認する）

```text
1. codexで指示書を書かせる
2. codexで書かせた指示書をレビューする
3. レビューに修正点があれば対応し、2に戻る
4. 実装する（Cursor側で実装）
5. codexで実装をレビューする
6. レビューに修正点があれば対応し、5に戻る
7. 最終報告をする
```

実行ルール:
- Step 2/5 は「重大 / 中 / 軽微」でレビュー結果を出す
- Step 3/6 は指摘がなくなるまでループする
- Step 4 の実装と修正は Cursor 側で行う
- 変更後は `./scripts/verify.sh` を実行する（個別に回す場合は下記と同等）
  - `cargo fmt --all -- --check`
  - `cargo clippy --workspace -- -D warnings`
  - `cargo test --workspace`
  - `./scripts/check-architecture.sh`
  - `./scripts/check-docs-consistency.sh`
- 完了時: 指示書を `docs/done/` へ移す場合は `docs/0000_spec-index.md` と `docs/todo/README.md` を同じ変更で更新
- `git commit` / `git push` はユーザー明示時のみ

参照:
- `docs/0000_spec-index.md` — 指示書一覧
- `docs/codex-delegation.md`
- `docs/manual/codex-spec-impl-review-loop.md`
- `docs/architecture.md` / `docs/testing.md` / `docs/security.md`
