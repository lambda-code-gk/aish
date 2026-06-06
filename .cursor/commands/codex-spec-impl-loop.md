---
description: Codexの設計書→実装指示→実装→レビュー→導通確認の9ステップを実行
---

次の 9 ステップをこの順で実行してください。

**対象タスク**: 「$ARGUMENTS」  
（未指定なら `docs/0000_spec-index.md` の進行中項目、またはユーザーが開いている `docs/spec/*` / `docs/tasks/*` / `docs/done/*` を確認する）

```text
1. codexで設計書を書かせる（docs/spec/）
2. codexで設計書をレビューする（指摘があれば修正し 2 に戻る）
3. codexで実装指示書を書かせる（docs/tasks/。設計書 docs/spec/ を正本にする）
4. 実装する（Cursor側。docs/tasks/ の指示書に従う）
5. codexで実装をレビューする（指摘があれば修正し 5 に戻る）
6. 品質ゲート + 正常系導通を実行する（下記「Step 6」）
7. Step 6 が失敗したら修正し、6 が通るまでループする（大きな修正なら 5 も挟む）
8. コミット準備: 実装指示書を docs/tasks/ → docs/done/ へ移し、docs/0000_spec-index.md を更新（設計書 docs/spec/ は移動しない）
9. 最終報告をする
```

実行ルール:
- Step 2/5 は「重大 / 中 / 軽微」でレビュー結果を出す
- Step 2/5 の指摘は 0 件になるまでループする
- Step 4 は Cursor 側で実装する
- **Step 6〜7 は完了条件**。verify と smoke が連続成功するまで Step 9 に進まない
- Step 6/7 の修正後も `./scripts/verify.sh` を再実行する
- Step 8 はコミット時に実施（`git commit` / `push` はユーザー明示時のみ）

## Step 6（品質ゲート + 正常系導通）

```bash
./scripts/verify.sh
./scripts/smoke-mock.sh
```

`verify.sh` を `| tail` などで包むと、完了まで無出力に見えるので避ける。

指示書に mock / ローカルで再現できる追加の正常系コマンドがあれば、同 Step 6 で続けて実行する。実 API が必要な手順は Step 9 の残リスクに回す。

## Step 7（導通失敗時）

1. 失敗ログを読み、修正する
2. `./scripts/verify.sh` → `./scripts/smoke-mock.sh` を再実行
3. 両方成功するまで 1〜2 を繰り返す

| 修正の性質 | 戻り先 |
|------------|--------|
| 配線・CLI 契約・起動順・設定参照 | Step 6 再実行（軽微なら Step 5 省略可） |
| 設計・境界・セキュリティ | Step 5 → Step 6 |

参照:
- `docs/0000_spec-index.md` — 設計書・指示書一覧
- `docs/spec/` — 設計書（Codex 作成、完了後も残す）
- `docs/tasks/` — 進行中の実装指示書
- `docs/done/` — 実装済み指示書（コミット時に tasks から移動）
- `docs/codex-delegation.md`
- `docs/manual/codex-spec-impl-review-loop.md`
