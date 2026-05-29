# 0019 — aish セッションログ連携の正式指示書

> **出典**: `docs/todo/chatgpt-review-4th-gen/p3-log-integration.md`、`docs/architecture.md`、`docs/done/0005_request-context-domain-spec.md`、既存実装（`aish/`、`ai/src/main.rs`、`ai/src/application/ask.rs`、`aish/src/adapters/outbound/toml_config.rs`）。
> **状態**: **実装済み**（2026-05-29）。

## 目的

`aish shell` と `ai ask` の導線をつなぎ、`ai ask` が **現在の aish セッションログを明示操作なしで読める**ようにする。

この指示書の狙いは次の 2 点に絞る。

1. `aish shell` で開始したセッションに対して、`ai ask` がログを自動利用できるようにする
2. セッションログの保存・発見・参照ルールを、`aish` と `ai` で分離したまま正規化する

## 確定仕様

ユーザーが確定した仕様は、この指示書の中でも **確定** として扱う。推測で上書きしない。

### 導線

1. メイン導線は `aish shell` 内で `ai ask`
2. 別ペイン導線は `ai ask --session <id>` とする
3. `aish` 終了後に親シェルから直近ログを読む導線は対象外
4. グローバル `sessions/current` は作らない

### セッション layout

- ベースパスは `<aish config log_dir>/<id>/`（環境変数 `AISH_SESSIONS_DIR` は **使わない**）
- ログ本体は `log.jsonl`
- シンボリックリンク `current_log` は `log.jsonl` を指す
- 対象は `aish shell` のみ
- `aish exec` は現状維持

### セッション ID

- セッション ID は `2020-01-01T00:00:00Z` からの経過ミリ秒を `u64` とする
- 表示・ディレクトリ名は **12 桁小文字 hex** のゼロ埋めとし、辞書順と時系列順を一致させる
- Base64 は使わない
- 起動時に `<id>/` が既に存在する場合は `+1ms` して再生成する
- 起動時に `stderr` へ session id を表示する

### aish の sessions 親パス解決

1. `~/.config/aish/config.toml` の `log_dir`
2. 既定 `~/.local/share/aish/sessions`

### ai の `--session` 解決

- **`AISH_SESSION_DIR` のみ**を使う（`AISH_SESSIONS_DIR` は **定義しない・読まない**）
- `--session <id>` は `AISH_SESSION_DIR/current_log` を読む
- `basename(AISH_SESSION_DIR)` と `<id>` が **一致しない**ときはエラー
- `aish` の config.toml は読まない
- id は **単一パスセグメント** に限定（`/`, `\`, `..` 等を拒否）

### 環境変数

- `aish shell` は `AISH_SESSION_DIR` を export する
- `AISH_SESSION_DIR` はセッション dir の絶対パスとする
- `AI_ASK_LOG` は export しない

### ログを載せる条件（`ai ask`）

- デフォルトではログを載せない
- `AISH_SESSION_DIR` のみでは載せない
- `AI_ASK_LOG=session` かつ `AISH_SESSION_DIR` が有効なら `current_log` を読む
- `AI_ASK_LOG` が設定されていて `session` 以外ならエラー
- `AI_ASK_LOG=session` だが `AISH_SESSION_DIR` が無効または不可読ならエラー
- `--log PATH` を与えたときは常にそのパスを読む
- `--session <id>` を与えたときは `AISH_SESSION_DIR/current_log` を読む（id 一致必須）
- 優先順は `--no-log` が最優先でログなし、次に `--log`、その次に `--session`、最後に env の `AI_ASK_LOG=session`
- ログを使うときは、常に `stderr` へ使用パスを 1 行表示する

### aibe コンテキスト（P3）

- 現状維持とする
- `shell_log_tail` は 16 KiB の tail
- `cwd` のみを渡す
- JSONL パースの追加や `RequestContext` 拡張は対象外

### クリーンアップ

- `aish shell` の起動時のみ実施する
- `max_sessions` を `aish` config.toml に追加する
- 未設定時の既定値は `50`
- 超過分はディレクトリ名の辞書順で古いものから削除する

### クレート境界

- `ai` は `aish` クレートに依存しない
- `aibe-protocol` の変更は P3 の対象外

## 受け入れ条件

### チェックリスト

- [ ] `aish shell` 起動時にセッション dir が生成される
- [ ] セッション dir の命名が 12 桁小文字 hex で、時系列順と辞書順が一致する
- [ ] 既存ディレクトリ衝突時に `+1ms` で再生成される
- [ ] `aish shell` 起動時に session id が `stderr` へ表示される
- [ ] `aish shell` が `AISH_SESSION_DIR` を export する
- [ ] `AI_ASK_LOG` は `aish shell` から export されない
- [ ] `ai ask` が `AI_ASK_LOG=session` のとき `current_log` を読む
- [ ] `AI_ASK_LOG=session` 以外はエラーになる
- [ ] `AI_ASK_LOG=session` だが session dir が無効または不可読ならエラーになる
- [ ] `--log PATH` が `--session <id>` より優先される
- [ ] `--no-log` が最優先でログを使わない
- [ ] `--session <id>` は `AISH_SESSION_DIR/current_log` を参照する（id は dir 名と一致）
- [ ] `AISH_SESSIONS_DIR` を参照しない
- [ ] ログ利用時は使用パスが `stderr` に 1 行で出る
- [ ] `aibe` へ渡す context は `shell_log_tail` + `cwd` のみである
- [ ] `aish shell` 起動時に `max_sessions` を超える古いセッションを削除する
- [ ] `aish exec` の挙動は変えない
- [ ] `ai` は `aish` クレートに依存しない
- [ ] `aibe-protocol` 変更なしで P3 を完結する

## 対象 / 非対象

### 対象

- `aish shell` のセッション生成・表示・export
- `aish` config の `log_dir` 参照と `max_sessions` 追加
- `ai ask` のログソース選択
- `ai ask --session <id>` の path 解決
- `stderr` の使用パス表示
- `aish` のセッション cleanup
- `docs/architecture.md` と手動検証文書の更新

### 非対象

- `aish exec` のセッション方式変更
- `aish` 終了後に親シェルからログを読む導線
- `aibe-protocol` の wire 変更
- `RequestContext` の JSONL パース拡張
- `agent_turn context` の構造化拡張
- `ai` から `aish` クレートを呼ぶ実装
- `sessions/current` のようなグローバル状態

## aish / ai 別の実装タスク分解

hexagonal の境界を維持し、`aish` はログとセッションの供給、`ai` はログの選択と読み込みに閉じる。

### aish

#### 1. セッション ID と layout

- セッション ID 生成ロジックを追加する
- epoch 起点は `2020-01-01T00:00:00Z`
- 12 桁 hex のゼロ埋め出力にする
- `AISH_SESSION_DIR` を export する
- `log.jsonl` と `current_log` の生成を行う

#### 2. 親パス解決

- `~/.config/aish/config.toml` の `log_dir` を読む
- それも無いときは `~/.local/share/aish/sessions` を使う

#### 3. 起動時の cleanup

- `aish shell` 起動時にだけ cleanup を走らせる
- `max_sessions` の既定値は `50`
- 超過分はディレクトリ名の辞書順で削除する
- cleanup は `shell` に限定し、`exec` には入れない

#### 4. stderr 出力

- 起動時に session id を 1 行出す
- 可能なら session dir も併記するが、必須表示は session id とする

#### 5. 実装境界

- セッション管理は `aish` の adapters / application に閉じる
- `ai` の内部構造に依存しない

### ai

#### 1. ログソース決定

- `--no-log` を最優先にする
- 次に `--log PATH`
- 次に `--session <id>`
- 最後に `AI_ASK_LOG=session`
- `AISH_SESSION_DIR` が有効でない状態で `AI_ASK_LOG=session` を受けたらエラーにする

#### 2. `--session` 解決

- `AISH_SESSION_DIR` を必須にする
- `aish` config は参照しない
- `--session <id>` は `basename(AISH_SESSION_DIR) == id` を検証し、`current_log` を読む

#### 3. ログ読み込み

- `current_log` を canonicalize し、実体が `AISH_SESSION_DIR` 配下の通常ファイルとして `File::open` できることを検証する
- 使用パス（解決後の実ファイル）は `stderr` に 1 行出す
- デフォルトは無効のまま維持する

#### 4. 実装境界

- `ai` は `aish` クレートを直接参照しない
- ログの存在確認やパス連結は `ai` 側で完結させる

## テスト計画

### unit

#### aish

- session ID の生成が 12 桁小文字 hex になること
- 既存ディレクトリ衝突時に `+1ms` で再生成すること
- 親パス解決が config `log_dir` → default の順になること
- `max_sessions` の既定値が `50` であること
- cleanup が辞書順で古いセッションを削除すること

#### ai

- `AI_ASK_LOG=session` のみを受け付け、それ以外はエラーになること
- `--session <id>` が `AISH_SESSION_DIR/current_log` を指すこと
- `--log` が `--session` より優先されること
- `--no-log` が最優先であること
- `AI_ASK_LOG=session` だが `AISH_SESSION_DIR` 無効時にエラーになること

### integration

#### aish

- `aish shell` 起動で session dir と `current_log` が作られること
- `AISH_SESSION_DIR` が子プロセスへ渡ること
- cleanup 後に `max_sessions` を超える古い dir が消えること

#### ai

- `AI_ASK_LOG=session` 時に `ai ask` が `current_log` の tail を使って `agent_turn` を組み立てること
- `ai ask --session <id>` が session 指定のログを読むこと
- ログ使用時に `stderr` へパスが 1 行出ること

### manual

- `aish shell` 内で `export AI_ASK_LOG=session` したうえで `ai ask` が `current_log` を読むこと
- 別ペインで `ai ask --session <id>` を実行し、指定セッションのログを読むこと
- `AI_ASK_LOG=session` で自動読み込みが成立すること
- `AI_ASK_LOG=other` でエラーになること
- `AISH_SESSION_DIR` が無い状態では自動読み込みしないこと

## docs 更新一覧

実装 PR では、次を同時に更新する。

1. `docs/architecture.md`
   - `aish shell` の session layout
   - `AISH_SESSION_DIR` / `AI_ASK_LOG` の関係（`AISH_SESSIONS_DIR` は無し）
   - `aibe` に渡す context は `shell_log_tail` + `cwd` のみであること
2. ルートの `README.md`
   - 実装体験の導線が `aish shell` -> `ai ask` の形であることを明示する
3. 手動検証文書
   - 既存の `docs/manual/aish-shell-log.md` を更新するか、必要なら新規 manual を追加する
   - **推測**: 既存 manual に追記する方が、`aish` の shell / log の文脈を分散させにくい
4. `docs/todo/chatgpt-review-4th-gen/README.md`
   - `p3-log-integration.md` へのリンクを維持し、P3 の着手先であることを明示する
5. `docs/0000_spec-index.md`
   - `0019` の **進行中** 行を追加する
   - `done/` へはまだ移さない
6. `docs/todo/chatgpt-review-4th-gen/p3-log-integration.md`
   - 0019 へのリンクを追加するか、0019 を正本として参照し直す

## エラーメッセージ方針

エラーは、利用者が次の行動をすぐ決められる程度に具体化する。

### `ai ask`

- `AI_ASK_LOG must be \"session\" when set`
- `AI_ASK_LOG=session requires AISH_SESSION_DIR to be set and readable`
- `--session requires AISH_SESSION_DIR to be set`
- `invalid session id: <id>`
- `--session <id> does not match AISH_SESSION_DIR (<dir>)`
- `session log not found: <path>`
- `session log unreadable: <path>`

### `aish shell`

- `failed to create session directory: <path>`
- `failed to create current_log symlink: <path>`
- `session id collision after regeneration`
- `failed to prune old sessions: <reason>`

### 方針

- 形式エラーと I/O エラーを分ける
- パスを含める
- ユーザーに `AISH_SESSION_DIR` の設定漏れが分かるようにする
- `AI_ASK_LOG` が `session` 以外のときは、値の比較をエラー文に含める

## 仮実装禁止

この指示書に対する実装では、次を禁止する。

- `current_log` を使わずに pid ベースの旧セッションログへ戻すこと
- `AI_ASK_LOG` を無条件に export すること
- `AISH_SESSIONS_DIR` 環境変数を導入すること
- `--session` の id で `..` や `/` を許すこと
- `sessions/current` のようなグローバル状態を追加すること
- `aibe-protocol` の変更を回避するために context を別経路で拡張すること
- `aish` と `ai` の境界を壊して相互依存を入れること

## 未確定・推測・指示外

- **推測**: 手動検証文書は既存 `docs/manual/aish-shell-log.md` の更新を第一候補にする。もし手順が肥大化するなら新規 manual に分割する。
- ルート `README.md` の更新を想定するが、実装時に現行導線へ合わせて最終確認する。
- `AISH_SESSION_DIR` の readable 判定や不可読時の扱いは実装で I/O エラーとして明示する前提だが、厳密な errno 文字列は未確定。

## 残リスク

- 手動検証は `aish shell` を含むため、TTY 実行での確認が必要になる可能性が高い
- cleanup の削除対象が本当に「古いセッション」であることは、辞書順と実時刻の一致を前提にしているため、実装でその前提を壊さない必要がある
- `AI_ASK_LOG=session` のエラー文は、利用者向け UX と実装都合の両立が必要
