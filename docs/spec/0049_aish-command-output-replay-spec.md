# 0049 — aish command output replay 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **起票**: 2026-06-23  
> **関連**: [0012_command-start-log-sanitize-spec.md](../done/0012_command-start-log-sanitize-spec.md)、[0019_aish-session-log-integration-spec.md](../done/0019_aish-session-log-integration-spec.md)、[0045_pack-composition-spec.md](0045_pack-composition-spec.md)、[architecture.md](../architecture.md)、[security.md](../security.md)、[testing.md](../testing.md)

## 0. 目的

`aish` で過去に実行したコマンドの出力を、**再実行せず** に再表示できるようにする。

狙いは次のとおり。

1. `aish shell` の対話コマンドも、`aish exec` と同じく replay 対象にする
2. 過去コマンドを一覧し、対話ピッカーから選んで再表示できるようにする
3. replay では **ログに残った出力をそのまま** 出す。再実行しない
4. `| rg` などの後段フィルタに渡せるよう、stdout 契約を壊さない

この機能は `aish` のログとシェル統合を強めるが、`ai` や `aibe` の責務には持ち込まない。

## 1. 非目標

- 再表示時にコマンドを再実行すること
- `aish` ログから秘密情報を復元すること
- `aish` の replay を `ai history` と同一ドメインとして扱うこと
- `aibe` / `ai` の wire protocol 変更
- Windows 対応
- `fish` 等の `ChildShellKind::Other` を v1 で完全対応すること
- 動的プラグインロード

## 2. パック構成の適用

**No**

本機能は optional 機能束の脱着ではなく、`aish` の core である「シェル実行 + ログ記録 + ログ再表示」の延長にある。`aish` は LLM / `aibe` 非依存であり、Active Pack / Basic Pack を分ける対象ではない。replay は通常の command / log / CLI 境界で実装し、パック構成は採用しない。

## 3. 現状と制約

### 3.1 既存の記録方式

- `aish exec` は `command_start` → `stdout` / `stderr` → `exit` で 1 コマンドを記録している
- `aish shell` は PTY の出力を `stdout` イベントとして追記しているが、**コマンド境界は記録していない**
- `aish session` はメタデータ表示のみで、出力 replay の正本ではない
- `aish/src/adapters/outbound/shell_completion.rs` が bash / zsh の一時 rcfile を注入しているため、ここに shell hook を足すのが最短経路である

### 3.2 replay に必要な追加情報

replay には、少なくとも次の情報が必要である。

- どの出力がどのコマンドに属するか
- そのコマンドの表示順
- そのコマンドの開始時刻と終了時刻
- 終了コード
- shell / exec の種別

したがって、`aish shell` の生出力ログに **command index** と **command span** の概念を追加する。
ここでいう `command span` は、bash / zsh の 1 つの対話入力行に対応する 1 単位であり、個々の simple command や pipeline ではない。

## 4. 設計概要

### 4.1 1 本の log に command span を載せる

replay 用の正本は既存の `log.jsonl` のままとする。別 DB や別の永続ストアは作らない。

新しいログモデルは次の 3 層で考える。

1. **command span**: 1 コマンドの開始・終了・時刻・終了コードを表す
2. **stream event**: stdout / stderr の 1 連の出力を表す
3. **session boundary**: `interactive_shell` の開始・終了は session 全体の境界として残す

command span は monotonic な `command_index` を持つ。`command_index` は 1 から始まり、1 つの log source 内で増分する。

### 4.2 新しい LogEvent モデル

`LogEvent` は replay 用に次の性質を持つ。

- `command_start` は `command_index` を持つ
- `stdout` / `stderr` は `command_index` を持つ
- `command_end` は `command_index`, `ended_at`, `exit_code` を持つ
- `interactive_shell` の session start / end は維持するが、replay の主キーではない

`command_index` が付いていない古いログは、`aish replay` の対象外または部分対応として扱う。v1 では **新規ログから replay 可能** であることを正本にする。

### 4.3 shell の境界記録

`aish shell` は、bash / zsh の rcfile hook を使って command span の開始と終了を parent に通知する。

設計の要点は次のとおり。

- `aish` は child shell へ **PTY とは別の内部向け control channel** を渡す
- control channel は parent がセッション dir に作成する named FIFO（`control.fifo`）に限定する。hook は `AISH_CONTROL_FIFO` のパスへ emit ごとに 1 行 1 JSON の `start` / `end` を書く（継承 FD は使わない）。PTY stdout / stderr や shell の通常出力を control message とみなしてはいけない
- `start` / `end` は最低限の制御情報だけを持つ（`command_index` は parent が採番し、child は持たない）。malformed message は破棄し、該当 span を replay 不可にする
- bash / zsh の hook は、command 開始時に `start`、終了時に `end` をその channel に送る
- parent 側はその通知を受け、`command_index` を採番して `log.jsonl` に記録する
- shell の visible output は v1 では PTY 由来の stdout として記録する。shell span では stderr を別ストリームとして復元しない
- hook の install に失敗した shell は replay 対象外として明示し、`list` / `pick` では除外する。`show` は `--index` 指定時に「replay 不可」としてエラーにする

`ChildShellKind::Other` は v1 では replay 境界の記録対象外でよい。必要なら従来どおり shell は動かし、replay CLI では「境界がない」と明示する。

### 4.4 exec の境界記録

`aish exec` は既に 1 コマンド単位で実行しているため、`ExecuteAndRecord` の前後で command span を記録するだけでよい。

- `command_index = 1`
- `started_at` は実行直前
- `ended_at` と `exit_code` は実行後
- stdout / stderr は既存どおり記録する

### 4.5 timestamps

`list` で time を見せるため、command span には時刻を入れる。

- 表示・保存の基準は UTC の RFC3339
- shell hook から時刻文字列を組み立てるのではなく、parent 側が marker 受信時に時刻を採る
- これにより bash / zsh で時刻生成ロジックを分岐させない

## 5. CLI 設計

### 5.1 サブコマンド構成

`aish` に `replay` サブコマンドを追加し、その下に 3 つの操作を置く。

```text
aish replay list
aish replay show
aish replay pick
```

### 5.1.1 ログソース解決

`list` / `show` / `pick` は、同じログソース解決規則を使う。

1. `--log PATH` があればそれを使う
2. なければ `AISH_SESSION_DIR` が有効なときに `current_log` を使う
3. `AISH_SESSION_DIR` が無効なときは `--log PATH` を要求してエラーにする

`AISH_SESSION_DIR` 由来の `current_log` は、0019 と同じく symlink の実体が session dir 配下の通常ファイルであることを確認してから読む。

### 5.2 `list`

`list` は replay 可能な command span を一覧する。

#### 受けるオプション

- `--log PATH`
- `--index N` は単独コマンドの絞り込みに使う
- `--format tsv|json` を使う
- `env` は多行一覧と相性が悪いため v1 では対象外にする

#### 画面契約

- stdout に一覧だけを出す
- picker の UI や警告は stderr
- `| rg` に流しても壊れないよう、装飾や余分な見出しは入れない

#### 表示列

最低限、次を含める。

- `index`
- `started_at`
- `finished_at`
- `exit_code`
- `kind`（`shell` / `exec`）
- `command`

`command` はログに記録された redacted 済みの値をそのまま表示する。

### 5.3 `show`

`show` は指定した `command_index` の出力を再表示する。

#### 受けるオプション

- `--log PATH`
- `--index N`
- `--stderr` は stderr のみを出したい場合の明示オプション。v1 では exec span のみ有効にする

#### stdout 契約

- デフォルトは stdout のみを出す
- `stderr` は明示した場合だけ出す
- いずれもログに残った文字列をそのまま出す
- replay の前後に余計なヘッダや注釈を付けない
- `kind=shell` の span では stderr を持たないため、`--stderr` はエラーにする

#### `rg` との相性

`aish replay show --index N | rg ...` を基本導線にする。

### 5.4 `pick`

`pick` は対話ピッカーで command span を選んで `show` 相当の replay を行う。

#### 受けるオプション

- `--log PATH`
- `--index N` は初期選択または直行用
- `--stderr`

#### 期待動作

- TTY であれば対話選択を行う
- 選択後は `show` と同じ stdout 契約で replay する
- 非 TTY では fail-closed にし、`list` + `show --index` を案内する

## 6. 対話ピッカー

### 6.1 fzf 依存の扱い

**オプション依存** とする。

方針は次のとおり。

- `fzf` が PATH にあれば優先的に使う
- `fzf` が無い場合は、`aish` 内蔵の簡易セレクタへ fallback する
- `pick` が TTY でない場合は fallback せずエラーにする

これにより、外部依存が無い環境でも機能を使える一方で、利用可能な環境では fzf 相当の体験を提供できる。

### 6.2 TTY 要件

`pick` は次を満たすときだけ動作する。

- stdin が TTY
- stdout が TTY
- stderr が TTY

満たさない場合は、`pick` を無理に非対話化せず、`show --index` への切り替えを促す。

### 6.3 表示項目

fzf や内蔵セレクタで見せる行には、少なくとも次を含める。

- index
- started_at
- exit_code
- kind
- command preview

長い command は省略表示してよいが、`show` が参照する index は変えない。

## 7. replay セマンティクス

### 7.1 再実行しない

replay は `shell` / `exec` のどちらでも、**ログから読むだけ** である。

- 子プロセス起動しない
- コマンドを再度評価しない
- shell hook を再度走らせない

### 7.2 redacted ログをそのまま使う

replay は記録済みの redacted 内容をそのまま出す。

- `command_start` の command / args は 0012 のサニタイズ済み値を使う
- stdout / stderr も記録時点の値をそのまま出す
- replay 時に `sanitize_log_text` を再適用しない

これにより、replay で秘匿情報を復元しない。

### 7.3 stdout / stderr の扱い

#### shell

- PTY の visible output は stdout として replay する
- ANSI エスケープは保持する
- PTY 記録どおりの text をそのまま出す
- `--stderr` は shell span では使えない。shell の replay は stdout のみを対象にする

#### exec

- stdout と stderr は分離して扱う
- `show` の既定は stdout
- stderr が必要な場合だけ `--stderr` を使う
- 既定で synthetic な merge はしない

### 7.4 バイナリ / UTF-8 の扱い

現行の `aish` ログは text 前提であるため、replay も text 前提にする。

- 有効な UTF-8 としてログに残ったものをそのまま出す
- `String::from_utf8_lossy` などの既存ロスは復元しない
- invalid byte sequence を完全復元することは v1 の目標にしない

### 7.5 部分ログ

command span が `start` だけで `end` が無い、または output が途中で切れている場合は replay できるとはみなさない。

- `list` では `partial` として表示しないか、別扱いで除外する
- `pick` の候補からは外す
- `show --index` ではエラーにする

## 8. セキュリティ

### 8.1 ログ権限

`log.jsonl` と replay で参照する内部状態は、既存方針どおり 0600 相当で作成・保持する。

### 8.2 session dir 外アクセス拒否

`AISH_SESSION_DIR` 由来の replay は、以下を満たすことを必須とする。

- `AISH_SESSION_DIR/current_log` を解決する
- canonicalize 後の実体が `AISH_SESSION_DIR` 配下にある
- 通常ファイルである
- symlink escape を拒否する

`--log PATH` は明示ユーザー入力なので別扱いにできるが、少なくとも通常ファイルかつ read-only であることを確認する。

### 8.3 secrets redaction との整合

replay は 0012 / 0019 の redaction 方針を壊さない。

- command line は 0012 のマスク後の値だけを表示する
- output は log に入っている redacted 後の text をそのまま表示する
- replay で新たに secret を露出させない

### 8.4 新しい trust boundary を増やさない

`aish` が読むのは自分が作成した session dir か、ユーザーが明示した `--log PATH` だけに限る。

- `aish` はネットワークを使わない
- `aish` は LLM を呼ばない
- `aish` は session dir 外の意図しないファイル探索をしない

## 9. フェーズ分割

### Phase 1: command span 記録

- `aish exec` に command span の start / end / index / timestamp を追加する
- `aish shell` の bash / zsh hook から command span を通知できるようにする
- `stdout` / `stderr` に `command_index` を紐づける（shell は stdout のみ）
- partial / incomplete span を除外できるようにする

### Phase 2: CLI replay

- `aish replay list`
- `aish replay show`
- `--log PATH` と `--index N`
- `AISH_SESSION_DIR` 既定解決

### Phase 3: interactive picker

- `aish replay pick`
- fzf 優先
- 内蔵 fallback
- non-TTY fail-closed

## 10. docs 同期対象

実装時は次の docs を同時に更新する。

- `docs/architecture.md`
  - aish ログの event model に command span / command index を追加する
  - `aish replay` の CLI 契約を追記する
- `docs/security.md`
  - replay が redacted ログだけを読むこと
  - session dir 外アクセス拒否
  - 0600 前提
- `docs/testing.md`
  - replay の unit / integration / manual の置き場所を追記する
- `docs/manual/aish-shell-log.md`
  - 既存の shell log 手順に replay 導線を追記する
- `docs/manual/aish-command-output-replay.md`（新規推奨）
  - `list` / `show` / `pick` の手動確認手順を分離して記載する

## 11. 受け入れ条件

### ログ拡張

- [ ] `aish exec` が command span を記録し、`index`, `started_at`, `finished_at`, `exit_code` を持つ
- [ ] `aish shell` が bash / zsh で command span を記録し、`aish shell` のコマンドも replay 対象になる
- [ ] `stdout` / `stderr` へ `command_index` が付く
- [ ] `ChildShellKind::Other` は v1 で replay 境界が無くても壊れない
- [ ] 既存の `interactive_shell` session 境界は壊さない
- [ ] `list` / `show` / `pick` は `AISH_SESSION_DIR/current_log` を既定解決できる
- [ ] `kind=shell` の span では `--stderr` が明示的にエラーになる

### CLI

- [ ] `aish replay list` が command span を一覧できる
- [ ] `aish replay show --index N` が再実行なしで記録済み output を stdout に出す
- [ ] `aish replay show --index N | rg ...` が成立する
- [ ] `aish replay show --stderr` が exec span の stderr replay を明示的に出せる
- [ ] `--log PATH` が `AISH_SESSION_DIR` なしでも使える
- [ ] `AISH_SESSION_DIR` ありでは `current_log` を既定解決できる

### picker

- [ ] `aish replay pick` が TTY で動作する
- [ ] `fzf` があれば優先的に使う
- [ ] `fzf` が無くても内蔵 fallback で選べる
- [ ] non-TTY では fail-closed になる

### セキュリティ

- [ ] replay は redacted 済みログだけを出す
- [ ] replay 時に `sanitize_log_text` を再適用しない
- [ ] `AISH_SESSION_DIR` 由来の読み取りは session dir 外への symlink escape を拒否する
- [ ] `log.jsonl` は 0600 前提を維持する

### テスト

- [ ] unit test で command span の parse / group が正しい
- [ ] integration test で `aish exec` replay が成立する
- [ ] integration test で `aish shell` replay が成立する
- [ ] integration test で `pick` の TTY / non-TTY 挙動が固定される
- [ ] manual 手順が `docs/manual/` にある

## 12. 補足

`aish replay` は、`aish shell` の対話体験を壊さずに「あとでその出力だけ取り出す」ための道具である。
そのため、見栄えを整えるよりも、**記録済みの text をそのまま再提示すること** を優先する。
