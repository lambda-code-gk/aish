# 0021 — CLI Tab 補完正式指示書

> **出典**: `AGENTS.md`、`.cursor/rules/10-boundaries.mdc`、`.cursor/rules/20-rust.mdc`、`docs/architecture.md`、`docs/testing.md`、`docs/security.md`、`docs/manual/aish-shell-log.md`、`docs/manual/ai-ask-tools.md`、現行実装（`aish/src/main.rs`、`aish/src/adapters/inbound/cli.rs`、`ai/src/main.rs`、`ai/src/domain/tools.rs`、`ai/src/domain/shell_log_resolve.rs`、`aibe/src/main.rs`）。
>
> **状態**: **実装済み**（2026-05-30）。本書は実装前の正式指示書であり、仮実装・サンプル止まりを許可しない。

## 目的

`aish` / `ai` / `aibe` の CLI を、bash / zsh で Tab 補完できるようにする。補完対象は、サブコマンド、フラグ、列挙値、一部の動的値まで含む。実行経路は `clap` + `clap_complete` に移行し、手書きパーサを廃止する。

本指示書の狙いは 3 点である。

1. CLI 契約を `clap` に集約し、補完と usage を同じ正本にする
2. 実行時設定に依存する値を、読み取り専用で補完できるようにする
3. `aish shell` 内、PATH 直起動、`cargo run` の 3 入口で同じ補完体験を提供する

## スコープ

### 対象

| 対象 | 範囲 |
|------|------|
| `aish` | `exec`, `shell`, `session`, `complete`。`--format`、`exec` の `--log` と `--` 境界を含む |
| `ai` | `ask`, `complete`。`ask` の全オプション、メッセージ引数、補完停止条件を含む |
| `aibe` | `--foreground` / `-f`, `complete` |
| 補完経路 | bash / zsh、PATH 直起動、`cargo run` 委譲、`aish shell` 内の一時 rcfile |
| docs | `README.md`, `docs/architecture.md`, `docs/manual/tab-completion.md`, `docs/0000_spec-index.md` |

### 非対象

| 対象外 | 理由 |
|--------|------|
| Windows 対応 | 本ワークスペースは Unix 専用 |
| Fish / PowerShell 補完 | 要件に含まれない |
| CI での自動 shell 実行検証 | 本件は manual 検証を正とする |
| `ai` のメッセージ本文への補完 | 誤入力・情報漏洩のリスクが高く、補完しない |
| `aish` / `ai` / `aibe` の既存挙動を残すための互換レイヤー | 後方互換より正しい CLI 契約を優先する |

## 現状との差分

現行実装は、`aish` が手書きパース、`ai` が独自ループ、`aibe` が `--foreground` だけの簡易判定で動いている。本指示書は次を正式に破壊的変更として扱う。

- `aish` / `ai` / `aibe` に `complete bash|zsh` サブコマンドを追加する
- `aish` / `ai` / `aibe` の CLI を `clap` ベースへ移行する
- `ai ask` は「オプションがメッセージより前」という `clap` 標準の順序に固定する
- `ai ask hello --log x` のような「メッセージ後のオプション」は即エラーにする
- `aish exec --` 以降は `aish` の補完を停止し、シェル標準の補完へ委譲する

## 確定仕様

### 1. CLI 契約

#### `aish`

- サブコマンドは `exec`, `shell`, `session`, `complete` とする。
- 共通オプション `--format tsv|json|env` は、`exec`, `shell`, `session` のすべてで受理する。
- `exec` は `--log PATH` と `-- <program> [args...]` を受理する。
- `shell` は `--log` を受理しない。補完でも候補に出さない。
- `session` は現在セッション情報の表示に専念する。
- `complete bash|zsh` は、対象 shell の補完スクリプトを stdout に出力する。

#### `ai`

- サブコマンドは `ask`, `complete` とする。
- `ask` のオプション: `--log`, `--session`, `--no-log`, `--socket`, `--no-start`, `--tools`, `--profile`, `--verbose-tools`
- `ask` は上記オプションをすべて `message` より前に置く。
- `ask` の positional message は補完しない。
- `complete bash|zsh` は、対象 shell の補完スクリプトを stdout に出力する。
- `ask` の全オプションは `clap` の正規化に従う。短縮オプションの自前実装はしない。

#### `aibe`

- `--foreground` / `-f` を受理する。
- `complete bash|zsh` を受理する。
- 補完生成以外の CLI は、既存の `aibe` 起動挙動を壊さない範囲で `clap` に載せる。

### 2. 実装方針

- `clap` + `clap_complete` を正本とする。
- 既存の手書きパースは廃止する。
- `-h/--help`、`-V/--version` を含む短縮オプションは `clap` の自動生成に任せる。
- 補完に必要な CLI 定義は、`main.rs` ではなく `clap` の型定義へ寄せる。
- `complete` サブコマンドは、実行時 CLI と同じ情報から生成する。別実装の補完ロジックを持たない。

### 3. 補完スコープ

#### 静的補完

- サブコマンド
- フラグ
- `--format` の列挙値 `tsv`, `json`, `env`
- `--tools` のカンマ区切りトークン

#### 動的補完

- `--profile` は `aibe` 設定の `[profiles.*]` から候補を読む
- `--session` は `aish` 設定の `log_dir` 配下にある 12 桁小文字 hex のディレクトリ名を候補にする
- `--log` と `--socket` はシェル標準のパス補完に委譲する

#### 補完停止条件

- `ai ask` の message 本文は補完しない
- `aish exec --` の `--` 以降は `aish` 補完を停止する
- 読み取り不能な config、欠損ディレクトリ、壊れた設定は空候補で返し、補完全体をエラーにしない

### 4. `--tools` の補完契約

- `--tools` は 1 個ずつのトークン補完とする
- 候補の正本は `ai/src/domain/tools.rs` と `aibe_protocol::KNOWN_TOOLS`（0009 方針）。tool 名・カテゴリ（`@read-only`, `@exec`, `@full`, `none`, `@none` 等）を含む
- `,` で区切られた前方の入力は保持し、現在入力中の token だけを補完する
- `none` は単独では候補に出してよいが、他トークンと組み合わせる入力は拒否対象である

### 5. `ai ask` の破壊的変更

- `ai ask` は、オプションが message より後ろに来た場合を受け付けない
- `ai ask hello --log x` は、移行期間なしで即エラーとする
- message の補完をしないことにより、completion が誤って本文を挿入しないようにする

### 6. 有効化環境

#### 6-1. PATH バイナリ

- `eval "$(aish complete bash)"`
- `eval "$(ai complete bash)"`
- `eval "$(aibe complete bash)"`
- zsh も同様に `complete zsh` を使う

#### 6-2. `cargo run` 委譲

`scripts/` に bash / zsh 向け補完スクリプトを **2 層** で提供する。

1. **登録用** — 補完定義の読み込み（PATH 直起動と同等）:
   - `eval "$(cargo run -q -p aish -- complete bash)"` 等
2. **実行時委譲** — 開発中の Tab 補完本体:
   - `cargo run -p ai|aish|aibe --` **以降**の引数列を、対応バイナリの動的補完（`clap_complete` の `_CLAP_COMPLETE` / `COMPLETE` 環境変数経路）へ委譲する
   - 例: `cargo run -p ai -- ask --pro<Tab>` が `--profile` を補完する
   - `-p` の crate 名（`ai` / `aish` / `aibe`）から委譲先を判定する

開発時は、ビルド済みバイナリが PATH に無くても `cargo run` 経由で **登録・実行時の両方** の補完が動くこと。

#### 6-3. `aish shell` 内

- 子 `$SHELL` が bash / zsh の場合に、補完を一時 rcfile で有効化する
- rcfile は永続化しない
- rcfile はユーザー既存 rc を先に読み、その後で補完定義を source する

### 7. 動的補完の設定参照

- `ai complete` は、`aibe` 設定の `profiles` を読み取り専用で参照する
- `ai complete` は、`aish` 設定の `log_dir` を読み取り専用で参照する
- `AIBE_CONFIG`, `AISH_CONFIG`, `AI_CONFIG` は、本番実行時と同じ優先順位で尊重する
- 読めない場合は、候補なしで黙って失敗する
- 補完のために設定ファイルを更新しない
- 動的補完の実行経路は `clap_complete` の標準（生成スクリプトが `_CLAP_COMPLETE` / shell 別 env を用いて同一バイナリを再実行）を正本とする。`complete bash|zsh` は静的生成、`--profile` 等の動的候補は実行時 env 経路で返す

### 8. レイヤー

- `aish`, `ai`, `aibe` の各 `main` を `clap` 化する
- workspace に `clap`, `clap_complete` を追加する
- `scripts/` に `cargo run` 委譲 completion を追加する
- `docs/` は実装と同じ PR / コミットで同期する

## 受け入れ条件

### CLI

- `aish complete bash` と `aish complete zsh` が成功し、補完スクリプトを出力する
- `ai complete bash` と `ai complete zsh` が成功し、補完スクリプトを出力する
- `aibe complete bash` と `aibe complete zsh` が成功し、補完スクリプトを出力する
- `ai ask` は `message` 以降にオプションが来た場合にエラーになる
- `aish exec --` 以降で `aish` の補完が止まる

### 補完

- `aish` の `--format` が `tsv/json/env` に補完される
- `ai ask` の `--profile` が `profiles.*` 由来で補完される
- `ai ask` の `--session` が 12 桁小文字 hex の session dir 名で補完される
- `ai ask` の `--tools` がカンマ区切りトークン単位で補完される
- `--log` と `--socket` はシェル標準のパス補完を使う
- 設定ファイルが読めない場合は候補なしで終わる

### 経路

- PATH 直起動、`cargo run` 委譲、`aish shell` 内の 3 経路で同じ補完契約になる
- 永続 rc の改変を残さない
- 既存の `--help` / `--version` は `clap` 自動生成で表示される

## レイヤー別タスク分解

### `aish`

- `clap` 定義を追加し、`exec`, `shell`, `session`, `complete` を表現する
- `--format` と `exec --log` を `clap` の value parser に載せる
- `exec --` の境界を `clap` ベースで扱う
- `complete` 生成を `clap_complete` に委譲する

### `ai`

- `clap` で `ask` を定義し、全オプションを positional message より前に揃える
- `--profile`, `--session`, `--log`, `--socket`, `--tools` の補完候補を分離する
- `ask` の message を completion 対象から外す
- `complete` 生成を `clap_complete` に委譲する

### `aibe`

- `--foreground` / `-f` と `complete` を `clap` に載せる
- `complete` 生成を `clap_complete` に委譲する

### `scripts/`

- `cargo run -p <crate> -- complete <shell>` に委譲する補完スクリプトを追加する
- `aish shell` 用の一時 rcfile 生成・注入の補助を追加する

### `docs/`

- `README.md` に CLI 補完の導線を追記する
- `docs/architecture.md` に `complete` と補完契約、`clap` 移行、動的補完参照を追記する
- `docs/manual/tab-completion.md` を新規作成し、bash / zsh / PATH / `cargo run` / `aish shell` の検証手順を書く
- `docs/0000_spec-index.md` に本書を進行中として追加する

## テスト計画

### unit

| 対象 | 期待 |
|------|------|
| `aish` / `ai` / `aibe` の CLI 定義 | `complete` を含む subcommand 体系が `clap` で表現できる |
| `ai ask` の parser | オプションが message より後ろに来た入力を拒否する |
| `--tools` 補完ロジック | カンマ区切りの現在 token のみを補完する |
| `--profile` / `--session` 補完ロジック | 設定ファイル・session dir 由来の候補が読める |
| 読み取り不能 config | 空候補で終わり、エラーを表に出さない |

### integration

| 対象 | 期待 |
|------|------|
| `aish complete bash|zsh` | 生成物が shell に読み込める |
| `ai complete bash|zsh` | 生成物が shell に読み込める |
| `aibe complete bash|zsh` | 生成物が shell に読み込める |
| `cargo run` 委譲スクリプト | 登録用 `complete bash|zsh` と、実行時 `_CLAP_COMPLETE` 委譲の両方が動く |
| `aish shell` 内補完 | 一時 rcfile 経由で有効化され、永続 rc が汚れない |

### manual

| 文書 | 期待 |
|------|------|
| `docs/manual/tab-completion.md` | bash / zsh / PATH / `cargo run` / `aish shell` の有効化と確認手順を実機で確認できる |

## docs 更新対象

| ファイル | 変更内容 |
|----------|----------|
| `README.md` | CLI 補完の導線と有効化コマンドを追記 |
| `docs/architecture.md` | `clap` 移行、`complete` 契約、補完スコープ、動的補完参照を追記 |
| `docs/manual/tab-completion.md` | manual 検証手順を新規作成 |
| `docs/manual/ai-ask-tools.md` | `ai ask` の引数順変更（オプション先行）を追記 |
| `docs/manual/aish-shell-log.md` | 必要なら `--session` 補完・CLI 変更の参照を追記 |
| `docs/0000_spec-index.md` | 本書を進行中として追加 |

## 未確定・推測・指示外

- `scripts/` に追加する補完スクリプトのファイル名は、実装時に既存命名規約へ合わせて決める。**推測**としては `scripts/complete-<shell>.sh` 系が自然だが、本書では固定しない。
- `aish shell` の一時 rcfile をどの一時領域に置くかは実装詳細であり、本書では固定しない。
- `--tools` にどの tool 名・カテゴリを補完候補として並べるかの詳細は、現行 allowlist 定義に合わせて実装時に確定する。
- `aibe` の `complete` に追加で別オプションを載せるかどうかは、現状の要件外である。

## 残リスク

- `aish shell` の rcfile 注入は shell 種別ごとの差異が大きく、bash / zsh で実機確認が必要である
- `ai ask` の破壊的変更は、既存の手打ち運用に影響する
- 動的補完は設定ファイルと session dir に依存するため、空候補時のユーザー体験を manual で確認する必要がある
