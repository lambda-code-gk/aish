# 0022 — AI_FILTER 正式指示書

> **出典**: `AGENTS.md`、`.cursor/rules/10-boundaries.mdc`、`docs/architecture.md`、`docs/security.md`、`docs/testing.md`、既存実装（`ai/src/adapters/outbound/stdout_presenter.rs`、`ai/src/ports/outbound/presenter.rs`、`ai/src/adapters/outbound/toml_config.rs`、`ai/src/application/ask.rs`、`ai/src/main.rs`）。
>
> **状態**: **実装済み**（2026-05-31）。本書は実装前の正式指示書であり、仮実装・サンプル止まりを許可しない。

## 目的

`ai ask` の assistant 本文に、ユーザーが指定したローカル filter コマンドを適用できるようにする。対象は `AgentTurnResult.assistant_message.content` のみであり、`ai` の stderr 系出力はそのまま保つ。

この機能は `ai` クレート内だけで完結させる。`aibe` と `aish` は今回変更しない。

## スコープ

### 対象

| 対象 | 範囲 |
|------|------|
| 環境変数 | `AI_FILTER` |
| 設定 | `~/.config/ai/config.toml` の `[ask].filter` |
| 実行対象 | `ai ask` の assistant 本文 |
| 表示契約 | `stdout` / `stderr` の分離、warning 文言、フォールバック挙動 |
| docs | `docs/architecture.md`、`docs/security.md`、`docs/testing.md`、`docs/ai.config.example.toml`、`docs/manual/ai-ask-tools.md`、`docs/0000_spec-index.md` |

### 非対象

| 非対象 | 理由 |
|--------|------|
| `aibe` / `aish` の挙動変更 | レイヤー境界上、今回の変更点ではない |
| CLI フラグ追加 | 確定仕様にない |
| `stderr` 系の filter | tools 起動行、warning、`--verbose-tools`、エラー、`shell_exec` 承認 UI は対象外 |
| タイムアウト導入 | 確定仕様にない |
| `ai ask` 以外のモード | 今回は `ai ask` のみ |

## 確定仕様

### 1. フィルタ対象

- フィルタを適用するのは `AgentTurnResult.assistant_message.content` のみとする。
- `StdoutPresenter` が出す `stderr` 系メッセージは変換しない。

### 2. 設定優先順位

- 優先順位は `非空 AI_FILTER` > `非空 [ask].filter` > なし とする。
- 空文字は未設定扱いとする。
- CLI フラグによる上書きは **導入しない**。

### 3. 実行方法

- filter は `/bin/sh -c "$FILTER"` で起動する。
- assistant 本文は stdin に pipe する。
- filter の stdout はユーザー stdout に `write_all` でそのまま流す。
- 末尾改行は filter 任せとし、`println!` 相当の自動改行は追加しない。
- filter の stderr は常にユーザー stderr に透過する。
- cwd と env は `ai` プロセスを継承する。
- タイムアウトは設けない。

### 4. 空 assistant

- assistant 本文が空のときは filter を起動しない。
- stdout も出力しない。

### 5. filter 非ゼロ終了

- filter の stdout があれば表示する。
- stderr に `warning: ai: filter exited with status N` を出す。
- `ai` の終了コードは 0 とする。

### 6. spawn 失敗

- filter 起動に失敗した場合は、未加工の assistant 本文をフォールバック表示する（`println!` 契約）。
- stderr に `warning: ai: filter failed: ...` を出す。
- `ai` の終了コードは 0 とする。

### 7. スコープの将来互換

- この env/config は将来の対話モード等でも再利用する前提にする。
- 本変更では `ai ask` 以外に広げない。
- `aish` 連携は今回の対象外である。

### 8. フィルタなし時

- フィルタが未設定なら、現状どおり `println!` による末尾改行付き表示を維持する。

## 受け入れ条件

- [x] `AI_FILTER` が非空なら、`[ask].filter` より優先される。
- [x] `AI_FILTER=""` は未設定扱いになり、`[ask].filter` へフォールバックする。
- [x] `[ask].filter` が非空なら、`AI_FILTER` 未設定時に適用される。
- [x] フィルタは `AgentTurnResult.assistant_message.content` にのみ適用される。
- [x] tools 起動行、warning、`--verbose-tools`、エラーは変換されない。
- [x] フィルタ stdout は `write_all` でそのまま出力され、余計な改行が追加されない。
- [x] フィルタ stderr はユーザー stderr に透過される。
- [x] フィルタが空の assistant に対して起動しない。
- [x] フィルタが非ゼロ終了しても `ai` の終了コードは 0 のままである。
- [x] spawn 失敗時は未加工の assistant 本文が表示され、`ai` の終了コードは 0 のままである。
- [x] `aibe` / `aish` の実装変更を伴わない。
- [x] `./scripts/verify.sh` が成功する。

## 実装概要

| ファイル | 内容 |
|----------|------|
| `ai/src/domain/output_filter.rs` | `resolve_output_filter` |
| `ai/src/adapters/outbound/output_filter.rs` | `/bin/sh -c` 実行、stdin/stdout/stderr 処理 |
| `ai/src/adapters/outbound/stdout_presenter.rs` | filter 適用、空 assistant 処理 |
| `ai/src/adapters/outbound/toml_config.rs` | `[ask].filter` 読み込み |
| `ai/src/main.rs` | filter 解決と `StdoutPresenter::new` |

## 残リスク

- filter はローカル shell command なので、ユーザー端末上で任意コードとして実行される。
- タイムアウトなしのため、filter がハングすると `ai ask` も待ち続ける。
- manual D 系（`docs/manual/ai-ask-tools.md`）は未実施の場合がある。
