# Codex 指示書→実装レビュー ループ手順

今回使った 7 ステップを、次回以降そのまま再利用できる形で固定する。

## 目的

- 仕様（指示書）と実装の品質を段階的に上げる
- 実装前後で Codex レビューを必ず通す
- 指摘がなくなるまでループする
- Cursor スラッシュコマンド: `/codex-spec-impl-loop <タスク>`

## 標準フロー（固定）

```text
1. codexで指示書を書かせる
2. codexで書かせた指示書をレビューする
3. レビューに修正点があれば対応し、2に戻る
4. 実装する（Cursor側で実装）
5. codexで実装をレビューする
6. レビューに修正点があれば対応し、5に戻る
7. 最終報告をする
```

## 実行ルール

- Step 2/5 のレビューは「重大 / 中 / 軽微」で出させる
- Step 3/6 は「指摘が 0 件になるまで」繰り返す
- Step 4 は Cursor 側で実装する（ユーザー指示を優先）
- 検証コマンドは必ず実行する（推奨: 一括）

```bash
./scripts/verify.sh
```

`verify.sh` を `| tail` などで包むと、完了まで無出力に見えるので避ける。静的検査だけなら `VERIFY_SKIP_TEST=1 ./scripts/verify.sh`。

個別に回す場合:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace -- -D warnings`
- `cargo test --workspace`
- `./scripts/check-architecture.sh`（クレート境界 + subprocess 方針 + hexagonal）
- `./scripts/check-docs-consistency.sh`（README / 仕様索引 / testing.md / review todo）

## 外部レビュー（ZIP 等）の突き合わせ

ChatGPT 等の外部レビューを取り込むときは、次を機械確認してから着手する。

```bash
./scripts/check-docs-consistency.sh
./scripts/check-architecture.sh
```

手動で見る観点:

- README のクレート数・依存の向きが `docs/architecture.md` と一致しているか
- `aibe` ツールの外部プロセスが `run_subprocess` 経由か（`check-architecture.sh` が検出）
- P3 前提（`ai ask --log` のみ等）が未実装なら、スコープを指示書に明記するか

## 使い回し用プロンプト雛形

### A. Step1（指示書作成）

```text
タスク: <対象タスク> の正式指示書を作成してください。

要件:
1. 受け入れ条件を具体化
2. 対象範囲/非対象を明確化
3. レイヤー境界に沿って実装タスク分解
4. テスト計画（unit/integration/manual）を明記
5. docs 更新対象を列挙
6. 関連 index/todo のリンク状態を更新
7. 仮実装禁止。本番経路前提

出力:
- 変更ファイル一覧
- 要点
- 未確定事項/残リスク
```

### B. Step2（指示書レビュー）

```text
作成済み指示書をレビューしてください。

観点:
1) 受け入れ条件の検証可能性
2) セキュリティ/境界
3) 実装可能性
4) docs/test 同期

出力形式:
- 重大/中/軽微の順で指摘
- 修正不要なら「問題なし」
- 修正案は最小差分で
```

### C. Step5（実装レビュー）

```text
未コミット差分を包括レビューしてください。

観点:
1) 指示書との整合
2) セキュリティ（危険経路、パス境界、権限）
3) アーキテクチャ境界
4) テスト妥当性・抜け

出力形式:
- 重大/中/軽微の順
- 各指摘に「ファイル名 + 根拠 + 影響 + 最小修正案」
- 問題なければ「重大なし」を明記
```

## 運用メモ

- Step5 で重大が出たら、Step6で修正後に必ず Step5 を再実行する
- 最終報告では「実施ステップ」「検証結果」「未確定・残リスク」を明記する
- `git commit` / `git push` はユーザー明示時のみ
