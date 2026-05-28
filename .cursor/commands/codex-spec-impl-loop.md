---
description: Codexの指示書→実装→レビューの7ステップを実行
---

次の 7 ステップをこの順で実行してください。対象タスクは「$ARGUMENTS」です。

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
- 変更後は次を実行する:
  - `cargo fmt --all -- --check`
  - `cargo clippy --workspace -- -D warnings`
  - `cargo test --workspace`
  - アーキテクチャに触れる変更では `./scripts/check-architecture.sh`
- `git commit` / `git push` はユーザー明示時のみ

参照ファイル:
- `docs/0018_safe-tools-policy-spec.md`
- `docs/codex-delegation.md`
- `docs/manual/codex-spec-impl-review-loop.md`
- `docs/architecture.md`
- `docs/testing.md`
- `docs/security.md`
