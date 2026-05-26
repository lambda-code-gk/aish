# 0012 — `command_start` ログサニタイズ指示書 — 仕様ドラフト

> **出典**: ChatGPT レビュー 4 代目の P0 / Sprint 1 第 1 項目。`docs/todo/chatgpt-review-4th-gen/concerns.md` §2、`p0-stabilization.md`、`implementation-order.md` を前提にした実装前仕様化。`verification.md` で突き合わせ済み。  
> **状態**: **実装済み**（2026-05-26。Codex レビュー指摘の直構築禁止・manual 更新を反映）

## 目的

aish の JSONL ログに記録される `command_start` から、コマンドラインに含まれうる秘密情報を取り除く。P0 の範囲では**新機能を足さず**、既存のログ契約を保ったままセキュリティだけを上げる。

この指示書は、実装前に以下を固定する。

- `command_start` の `command` と各 `args` に `sanitize_log_text` を必ず適用する
- ログ JSON の **フィールド名・イベント名・構造は維持**し、変えるのは値のみとする
- `stdout` / `stderr` の既存サニタイズと同じ安全水準に揃える
- `aish` のみを対象とし、`aibe` / `ai` の挙動や wire protocol は変更しない

## スコープ

### 対象

- `aish/src/domain/log_event.rs` の `LogEvent::command_start`
- `aish/src/main.rs` の対話シェル側 `CommandStart` 追記経路
- `aish/src/application/execute_and_record.rs` の `command_start` 追記経路
- `aish/tests/exec_log.rs` を含む JSONL 回帰テスト
- `docs/security.md` と `docs/architecture.md` のログ記述
- `docs/manual/aish-shell-log.md` の手動検証手順

### 対象外

- `stdout` / `stderr` のサニタイズ規則拡張
- `sanitize_log_text` の正規表現追加や別パターン拡張
- `aibe` / `ai` のログ形式変更
- ログ JSON への新フィールド追加
- コマンド実行の許可制・承認 UI・監査フォーマット変更

## 確定した設計判断

| 項目 | 方針 |
|------|------|
| **サニタイズの置き場所** | `LogEvent::command_start` のファクトリで `command` / `args` を `sanitize_log_text` した値に正規化する。呼び出し側ごとの後付けマスクは採らない。 |
| **理由** | `command_start` は複数箇所から生成されるため、ドメインの生成点で安全化した方が取りこぼしがない。`SessionLog` 実装や application 層にマスク責務を散らさない。 |
| **JSON 互換性** | `event: "command_start"` と `command` / `args` のフィールド名は維持する。変化はマスク後の値のみ。 |
| **適用範囲** | `exec` と `shell` の両方に適用する。`main.rs` の対話シェル側も同一方針で追記する。 |
| **既存のマスク** | `stdout` / `stderr` の `sanitize_log_text` は維持し、`command_start` を同じマスク基準に揃える。 |
| **境界** | この変更は aish 内部に閉じる。aibe の `command_start` 相当や ai の表示ロジックには波及させない。 |
| **テスト方針** | JSONL の回帰テストで、秘密文字列を含む `command` / `args` が raw のまま残らないことを確認する。既存の `exec_log` は更新対象に含める。 |
| **生成 API** | `CommandStart` の enum 直構築は禁止し、`LogEvent::command_start(&CommandSpec)` 経由に統一する（`main.rs` の shell 経路を含む）。 |

## ログ JSON 互換性

`command_start` のイベントは引き続き次の形を保つ。

```json
{
  "event": "command_start",
  "command": "echo",
  "args": ["hello"]
}
```

この変更で許容するのは値のマスクのみであり、イベント名やフィールド追加・削除は行わない。

## 受け入れ条件

### 1. `command_start` の値だけがマスクされる

- `command` に `sk-...`、`Bearer ...`、`KEY` / `TOKEN` / `SECRET` を含む文字列を与えたとき、ログには raw 値が残らない
- `args` の各要素も同じサニタイズを通る
- `stdout` / `stderr` の既存サニタイズ動作は壊さない

### 2. JSONL 互換性を維持する

- `event` 名とフィールド名は変えない
- 既存の JSONL パーサやテストが、値の更新だけで済む状態を維持する
- `command_start` が `SessionLog` の実装依存にされない

### 3. 対話シェル経路も同じ基準に揃える

- `aish/src/main.rs` の `shell` 経路でも、`LogEvent::command_start(&CommandSpec { ... })` を使う（`CommandStart` 直構築禁止）
- `exec` と `shell` の両経路で、ログの安全性が一致する

### 4. テストで回帰を防ぐ

- `aish/tests/exec_log.rs` を更新し、`command_start` に秘密文字列を含めても raw が出ないことを検査する
- `domain/log_event.rs` に `command_start` 生成点の単体テストを追加する
- `shell` 経路は `exec_log` と同様の自動テストが難しいため、`docs/manual/aish-shell-log.md` で `command_start` のマスクを手動確認する

### 5. docs を同時に更新する

- `docs/security.md` に `command_start` も `sanitize_log_text` を通すことを明記する
- `docs/architecture.md` の `aish ログ` 節に、`command_start` がマスク済みで保存される旨を追記する

## 実装手順の目安

1. `feature/v0.1-stabilization` 系の作業ブランチで着手する
2. `aish/src/domain/log_event.rs` の `command_start` をサニタイズ済み生成へ寄せる
3. `aish/src/main.rs` と `aish/src/application/execute_and_record.rs` の呼び出しを同方針で揃える
4. `aish/tests/exec_log.rs` を更新し、raw 秘密情報が残らない回帰テストを追加する
5. `docs/security.md` と `docs/architecture.md` を同期する
6. 実装 PR では `cargo fmt --all -- --check` / `cargo clippy --workspace -- -D warnings` / `cargo test --workspace` / `./scripts/check-architecture.sh` を通す

## docs 更新一覧

- `docs/security.md`
  - `aish` ログの「実装済みマスク」に `command_start` を追加する
  - `command_start` は `sanitize_log_text` を通してから追記することを明記する
- `docs/architecture.md`
  - `aish ログ` のイベント表に、`command_start` もマスク済み保存である旨を追記する
  - 既存の `command_start` / `stdout` / `stderr` の役割分担を維持する
- `docs/manual/aish-shell-log.md`
  - `aish shell` の期待結果に、`command_start` でも機微文字列が平文で残らない旨を追加する
- `docs/0000_spec-index.md` — 0012 を実装済みとして `done/` に登録（完了）

## 未確定・見送り

| 種別 | 内容 |
|------|------|
| **見送り** | `sanitize_log_text` の正規表現を広げることは別タスクとする。今回の P0 では既存パターンの適用漏れをなくすことを優先する。 |
| **見送り** | パスワードプロンプト直後の手入力や here-doc のような追加ソースは対象外とする。 |
| **見送り** | ログ出力先の暗号化、ローテーション、保持期間の設計変更は扱わない。 |
| **推測** | 実運用では `command_start` 以外の位置にも秘密が入りうるが、今回の P0 では `command_start` の取りこぼし修正を最優先にする。 |

## 残リスク

- `sanitize_log_text` はヒューリスティックなので、記法が変則的な秘密情報は取りこぼす可能性がある
- `command_start` を安全化しても、将来追加される別イベントで同種の漏洩が起きる余地は残る
- 手動で投入されるコマンド内容の全パターンは自動テストだけでは網羅しきれない
