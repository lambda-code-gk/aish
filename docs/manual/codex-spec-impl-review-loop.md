# Codex 設計書→実装指示→実装レビュー ループ手順

次回以降そのまま再利用できる 9 ステップ。ドキュメントの置き場所は [0000_spec-index.md](../0000_spec-index.md) を正とする。

| 種別 | 置き場所 | 移動 |
|------|----------|------|
| 設計書 | `docs/spec/00xx_<topic>-spec.md` | 完了後も残す |
| 実装指示書 | `docs/tasks/00xx_<topic>-implementation-spec.md`（**設計と同じ番号**）→ 完了後 `docs/done/` | **コミット時**に移動 |

## 目的

- 設計書（`docs/spec/`）と実装指示書（`docs/tasks/`）の品質を段階的に上げる
- 実装前後で Codex レビューを必ず通す
- 指摘がなくなるまでループする
- **完了前に mock 導通（`ai ask` 正常系）まで通す** — 手動テストの初回失敗を減らす
- Cursor スラッシュコマンド: `/codex-spec-impl-loop <タスク>`

## 標準フロー（固定）

```text
1. codexで設計書を書かせる（docs/spec/）
2. codexで設計書をレビューする（指摘があれば修正し 2 に戻る）
3. codexで実装指示書を書かせる（docs/tasks/。設計書を正本にする）
4. 実装する（Cursor側。docs/tasks/ に従う）
5. codexで実装をレビューする（指摘があれば修正し 5 に戻る）
6. 品質ゲート + 正常系導通を実行する
7. Step 6 が失敗したら修正し、6 が通るまでループする（大きな修正なら 5 も挟む）
8. コミット準備: 実装指示書を docs/tasks/ → docs/done/ へ移動（設計書は spec に残す）
9. 最終報告をする
```

## 実行ルール

- Step 2/5 のレビューは「重大 / 中 / 軽微」で出させる
- Step 2/5 は「指摘が 0 件になるまで」繰り返す
- Step 4 は Cursor 側で実装する（ユーザー指示を優先）
- **Step 6〜7 は完了条件**。verify と smoke が連続成功するまで Step 9 に進まない
- Step 8 はコミット時に実施（`git commit` / `push` はユーザー明示時のみ）

### Step 6（品質ゲート + 正常系導通）

```bash
./scripts/verify.sh
./scripts/smoke-mock.sh
```

`verify.sh` を `| tail` などで包むと、完了まで無出力に見えるので避ける。静的検査だけなら `VERIFY_SKIP_TEST=1 ./scripts/verify.sh` だが、**Step 7 完了判定では使わない**（test + smoke まで通す）。

個別に回す場合（verify 内訳）:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace -- -D warnings`
- `cargo test --workspace`
- `./scripts/check-architecture.sh`（クレート境界 + subprocess 方針 + hexagonal）
- `./scripts/check-docs-consistency.sh`（README / 仕様索引 / testing.md / review todo）

`smoke-mock.sh` は mock aibe を foreground 起動し、`ai ask "ping"` の round-trip と stdout/stderr 契約を検証する（実 API キー不要）。詳細は [testing.md](../testing.md) と [ai-ask-tools.md](ai-ask-tools.md)。

指示書に mock / ローカルで再現できる追加の正常系コマンドがあれば、同 Step 7 で続けて実行する。実 API が必要な手順は Step 9 の残リスクに回す。

### Step 7（導通失敗時）

1. 失敗ログを読み、修正する
2. `./scripts/verify.sh` → `./scripts/smoke-mock.sh` を再実行
3. 両方成功するまで 1〜2 を繰り返す

| 修正の性質 | 戻り先 |
|------------|--------|
| 配線・CLI 契約・起動順・設定参照 | Step 7 再実行（軽微なら Step 5 省略可） |
| 設計・境界・セキュリティ | Step 5 → Step 7 |

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

### A. Step1（設計書作成 → docs/spec/）

```text
タスク: <対象タスク> の設計書を docs/spec/ に作成してください。

要件:
1. 目的・背景・非目標を明確化
2. 信頼境界・既存ポリシーとの整合
3. スキーマ案・フロー（mermaid 可）
4. 未確定事項を列挙
5. docs/0000_spec-index.md を更新
6. 実装の受け入れ条件は概要のみ（詳細は docs/tasks/ に委ねる）

出力:
- 変更ファイル一覧
- 要点
- 未確定事項/残リスク
```

### B. Step2（設計書レビュー）

```text
作成済み設計書（docs/spec/）をレビューしてください。

観点:
1) 受け入れ条件の検証可能性
2) セキュリティ/境界
3) 実装可能性
4) docs/test 同期
5) mock smoke（./scripts/smoke-mock.sh）または同等の正常系で検証可能か

出力形式:
- 重大/中/軽微の順で指摘
- 修正不要なら「問題なし」
- 修正案は最小差分で
```

### C. Step3（実装指示書作成 → docs/tasks/）

```text
タスク: docs/spec/<設計書> を正本に、Cursor 実装用の指示書を docs/tasks/ に作成してください（ファイル名は設計と同じ 00xx、`-implementation-spec.md` サフィックス）。

要件:
1. 受け入れ条件を具体化（検証可能に）
2. レイヤー境界に沿った実装タスク分解
3. テスト計画（unit/integration/manual）
4. docs 更新対象を列挙
5. 仮実装禁止。本番経路前提
6. mock / ローカルで再現できる正常系コマンドを列挙（Step 6 で実行可能に）

出力:
- 変更ファイル一覧
- 要点
- 未確定事項/残リスク
```

### D. Step5（実装レビュー）

```text
未コミット差分を包括レビューしてください。

観点:
1) 指示書との整合
2) セキュリティ（危険経路、パス境界、権限）
3) アーキテクチャ境界
4) テスト妥当性・抜け
5) ./scripts/smoke-mock.sh が通る経路か（binary 起動・socket・ai ask 契約）

出力形式:
- 重大/中/軽微の順
- 各指摘に「ファイル名 + 根拠 + 影響 + 最小修正案」
- 問題なければ「重大なし」を明記
```

### E. Step6 失敗時（導通修正）

```text
Step 6 の ./scripts/verify.sh または ./scripts/smoke-mock.sh が失敗しました。

添付ログ:
<終了コード・stdout/stderr>

やること:
1. 根本原因を特定（起動順、socket、設定参照、CLI 契約、deps の向き）
2. 最小差分で修正
3. verify → smoke を再実行して成功を確認
4. 設計/境界に触れたら Step 5 レビュー観点で自己チェック

出力:
- 原因
- 変更ファイル
- 再実行結果
```

## 運用メモ

- Step5 で重大が出たら、修正後に必ず Step5 を再実行する
- Step6〜7 を省略しない（「test が通ったから OK」で終わらせない）
- コミット時に実装指示書を `docs/tasks/` → `docs/done/` へ移す（設計書は `docs/spec/` に残す）
- 最終報告では「実施ステップ」「verify / smoke 結果」「未確定・残リスク」を明記する
- `git commit` / `git push` はユーザー明示時のみ
