# 0027 — `ai` コマンド UX 改善 実装指示書

> **種別**: 実装指示書（`docs/done/`）  
> **状態**: 完了  
> **設計の正本**: [0027_ai-ux-spec.md](../spec/0027_ai-ux-spec.md)  
> **関連**: [architecture.md](../architecture.md)、[security.md](../security.md)、[testing.md](../testing.md)、[0000_spec-index.md](../0000_spec-index.md)

## 目的

`ai` を日常利用の入口として使いやすくする 0027 を、本番経路として段階導入するための実装指示書である。  
Phase A / B / C の順で実装し、**Phase C の wire 変更は `aibe-protocol` から開始**する。既存の回帰を壊さず、`./scripts/verify.sh` を最終到達点とする。

## 実装順

1. Phase A: `aish` の環境注入と `ai` の CLI/診断/出力制御を追加する
2. Phase B: preset、`history`、`retry`、`rerun`、`--session`、`--log-tail` を追加する
3. Phase C: `chat`、progress、streaming、cancel、`--timeout`、`--yes-exec` を追加する

## ファイル単位の変更リスト

| クレート | 変更対象ファイル | 役割 |
|----------|------------------|------|
| `aish` | `aish/src/adapters/outbound/pty_shell.rs` | `aish shell` の child shell に `AI_ASK_LOG=session` を自動 export する |
| `aish` | `aish/src/main.rs` | shell 起動の制御点。必要なら環境注入の結果表示とテスト用フックを追加する |
| `aish` | `aish/tests/*`（新規） | `AI_ASK_LOG=session` と `AISH_SESSION_DIR` の注入を固定する統合テストを追加する |
| `ai` | `ai/src/clap_cli.rs` | `ai <message>` の default ask、`status` / `doctor` / `ping` / `chat` / `history` / `retry` / `rerun`、`--quiet` / `--preset` / `--format` / `--dry-run` / `--session` / `--log-tail` を追加する |
| `ai` | `ai/src/main.rs` | subcommand dispatch、dry-run short-circuit、診断系出力、履歴操作の orchestrator を接続する |
| `ai` | `ai/src/domain/ask.rs`、`ask_arg_order.rs`、`shell_log_resolve.rs`、`llm_profile.rs`、`output_filter.rs` | default ask、入力ソース優先順、`--session` 解決、preset 既定値、出力フィルタの解決を整理する |
| `ai` | `ai/src/domain/*`（新規） | history レコード、diagnostics view、replay payload の domain を追加する |
| `ai` | `ai/src/application/ask.rs`、`ask_launch.rs`、`history.rs`、`chat.rs`（新規/更新） | local store、retry/rerun、dry-run、chat REPL のユースケースを分離する |
| `ai` | `ai/src/adapters/outbound/toml_config.rs` | `[presets.*]`、`history_dir`、`log_tail_bytes` を読む |
| `ai` | `ai/src/adapters/outbound/aibe_client.rs` | ping、通常 turn、Phase C の stream/cancel を呼ぶ接続層 |
| `ai` | `ai/src/adapters/outbound/stdout_presenter.rs` | `--format json|tsv|env`、`--quiet`、診断系の表示分岐を整理する |
| `ai` | `ai/src/adapters/outbound/dynamic_completion.rs` | 新 subcommand / flags の補完を必要に応じて更新する |
| `ai` | `ai/tests/*`（新規/更新） | status/doctor/ping、default ask、history/retry/rerun、dry-run、preset、session 解決の統合テストを追加する |
| `aibe-client` | `aibe-client/src/lib.rs`、`transport.rs`、`unix_connect.rs` | ping、turn、Phase C の stream/cancel を扱える transport に拡張する |
| `aibe-client` | `aibe-client/tests/*`（新規/更新） | socket 往復、ping、streaming、cancel、ensure_running の回帰を固定する |
| `aibe-protocol` | `aibe-protocol/src/request.rs`、`response.rs`、`executed_tool.rs`、`lib.rs` | progress event、assistant streaming event、cancel request を wire DTO として追加する |
| `aibe-protocol` | `aibe-protocol/src/*` の unit tests | serde 互換と既存 DTO の後方互換を固定する |
| `aibe` | `aibe/src/application/agent_turn.rs` | progress、streaming、cancel、`--timeout` の turn 制御を実装する |
| `aibe` | `aibe/src/application/request_service.rs`、`server.rs`、`protocol_convert.rs` | new wire との変換と server 側の request/response orchestration を更新する |
| `aibe` | `aibe/src/adapters/outbound/openai_compatible.rs`、`gemini.rs`、`mock_llm.rs`、`scripted_mock_llm.rs` | provider stream / buffered fallback を aibe 側で吸収する |
| `aibe` | `aibe/src/application/tool_round/*`、`aibe/src/ports/outbound/*`（必要最小限） | turn 中の状態遷移と cancel 伝播を保持する |
| `aibe` | `aibe/tests/*`（新規/更新） | socket protocol、agent turn loop、streaming/cancel、`ai ask` E2E の回帰を固定する |

## Phase A

### 対象

`aish` の child shell 環境注入と、`ai` の default ask / 診断 / 出力制御の基礎を入れる。  
この段階では **aibe プロトコル変更をしない**。

### 実装手順

1. `aish shell` の fork 後に `AI_ASK_LOG=session` を child shell にのみ export する
2. `ai` root を default ask にし、先頭の非 flag token が未知でも ask として扱う
3. `status` と `doctor` を実装し、`ping` で socket 生存確認だけを行う
4. `--quiet`、`-f/--file`、`-`、`--format json|tsv|env`、`--dry-run` を `ask` / `status` / `doctor` / `ping` に導入する
5. `status` / `doctor` の診断出力は、config、preset、socket、`AISH_SESSION_DIR`、implicit session、log tail を含める
6. `dry-run` は aibe に接続しないことを保証し、raw message / raw log tail / filter body を表示しない

### Phase A の test gate

```bash
cargo test -p aish --tests
cargo test -p ai --tests
cargo test -p aibe-client --tests
```

### Phase A の受け入れ条件

- `aish shell` の child shell でのみ `AI_ASK_LOG=session` が有効になる
- `ai "hello"` が `ai ask "hello"` と同義になる
- `ai status` / `ai doctor` / `ai ping` が local 診断として動く
- `--quiet` が非エラー診断を抑制する
- `--format json|tsv|env` が診断系で壊れない
- `--dry-run` が aibe 接続なしで完了する

## Phase B

### 対象

local history、preset、`--session` の implicit 解決、`--log-tail` を入れる。  
この段階も **aibe プロトコル変更なし** で完結させる。

### 実装手順

1. `~/.config/ai/config.toml` の `[presets.*]`、`history_dir`、`log_tail_bytes` を読む
2. CLI の明示値 > preset > `[ask]` > hardcoded default の優先順位を固定する
3. `AISH_SESSION_DIR` があるときは basename を implicit `--session` として扱い、明示 `--session` と不一致ならエラーにする
4. `--log-tail` の上限を protocol ceiling に揃え、`0` で無効化できるようにする
5. local history の `index.jsonl` と payload vault を分離する
6. `history` / `retry` / `rerun` を実装し、redacted index と replay payload の役割を分離する
7. `history` / `retry` / `rerun` でも `--quiet` と `--format json|tsv|env` を共通の出力規則として扱う

### Phase B の test gate

```bash
cargo test -p ai --tests
cargo test -p aibe-client --tests
cargo test -p aish --tests
```

### Phase B の受け入れ条件

- preset が CLI > preset > config > default の順で解決される
- `history` は redacted index のみを読む
- `retry` は現在の既定値で再送し、`rerun` は保存済み envelope をそのまま再生する
- `--session` の implicit 解決が `AISH_SESSION_DIR` basename に一致する
- `--log-tail` が ceiling を超えない

## Phase C

### 対象

`chat` REPL、progress、assistant streaming、cancel、`--timeout`、`--yes-exec` を入れる。  
**wire の変更はここでのみ行い、最初に `aibe-protocol` を更新する。**

### 実装手順

1. `aibe-protocol` に progress event、assistant streaming event、cancel request を追加する
2. `aibe-client` の transport を event stream と cancel に対応させる
3. `aibe` 側で progress / streaming / cancel / timeout を turn loop に流し込み、provider ごとの streaming fallback を揃える
4. `ai chat` を client-side transcript の REPL として実装する
5. `--progress` を stderr に出し、`--quiet` で抑制する
6. `Ctrl+C` と `--timeout` で clean cancel する
7. `--yes-exec` の session 限定記憶を追加し、`shell_exec_approval=never` を越えない fail-closed を維持する

### Phase C の test gate

```bash
cargo test -p aibe-protocol --tests
cargo test -p aibe-client --tests
cargo test -p aibe --tests
cargo test -p ai --tests
```

### Phase C の受け入れ条件

- progress が turn 中の phase を stderr に出す
- assistant streaming が chunk 単位で表示される
- `Ctrl+C` と `--timeout` が turn cancel として扱われる
- `chat` が multi-turn を client-side で保てる
- `--yes-exec` が session 限定の承認記憶として働く
- wire 変更が `aibe-protocol` 起点で実装される

## `scripts/smoke-mock.sh` の更新方針

既存の smoke は `ai ask` 1 回中心なので、0027 では **Phase A / B の安定経路を追加**する。  
追加対象は次を優先する。

- `ai "hello"` が default ask として動くこと
- `ai ping` が socket 生存確認だけを行うこと
- `ai status --format json` が機械可読に出ること
- `ai ask --dry-run` が aibe に接続しないこと

`history` / `retry` / `rerun` / `chat` / progress / cancel は、smoke の一発実行よりも統合テストと manual に寄せる。  
`aish shell` の対話的な `AI_ASK_LOG=session` は smoke ではなく integration / manual で担保する。

## docs 更新対象

- `docs/architecture.md`  
  `ai` / `aish` / `aibe` / `aibe-client` / `aibe-protocol` の責務、Phase C wire、`history` / `retry` / `rerun` の境界を追記する
- `docs/security.md`  
  `history` の redaction、payload vault、`--dry-run` の masking、`--yes-exec` の危険性、`aish shell` の child-only export を追記する
- `docs/testing.md`  
  0027 の unit / integration / smoke / manual の追加、`scripts/smoke-mock.sh` の役割変更を追記する
- `docs/ai.config.example.toml`  
  `[presets.*]`、`history_dir`、`log_tail_bytes`、`quiet` / `shell_exec_approval` の例示を追加する
- `docs/manual/README.md`  
  新規 manual を追加した場合に索引へ載せる
- `docs/manual/ai-ux.md`（新規）  
  `status` / `doctor` / `ping` / `history` / `retry` / `rerun` / `chat` / `--progress` / `--timeout` / `--yes-exec` の手動検証をまとめる
- `docs/manual/aish-shell-log.md`  
  `AI_ASK_LOG=session` の自動 export を明記する
- `docs/manual/ai-ask-tools.md`  
  `ai` root の default ask と既存の tool 表示/承認確認の差分を必要に応じて更新する
- `docs/manual/tab-completion.md`  
  新 subcommand と flags の補完を追記する

## 受け入れ条件チェックリスト

### unit

- [ ] `ai` root が default ask と既知 subcommand を正しく分岐する
- [ ] `--quiet` が非エラー診断を抑制する
- [ ] `-f` / `-` / stdin の入力優先順が正しい
- [ ] `--preset` が CLI > preset > config > default の順で解決される
- [ ] `--format json|tsv|env` が各 command で壊れない
- [ ] `history` の redacted index と replay payload が分離される
- [ ] `retry` と `rerun` の意味差が固定される
- [ ] `--session` 省略時に `AISH_SESSION_DIR` basename が使われる
- [ ] `--log-tail` が protocol ceiling を超えない

### integration

- [ ] `aish shell` 起動時に `AI_ASK_LOG=session` が child shell に入る
- [ ] `ai "..."` が `ai ask "..."` と同じ request を組む
- [ ] `status` / `doctor` / `ping` が aibe の起動有無を診断できる
- [ ] `dry-run` が aibe に接続しない
- [ ] `history` が local store から一覧できる
- [ ] `retry` / `rerun` が過去 record を再生できる
- [ ] `chat` が multi-turn を保てる
- [ ] `--yes-exec` が session 限定の承認記憶として働く

### smoke

- [ ] `ai ping`
- [ ] `ai status --format json`
- [ ] `ai ask --dry-run`
- [ ] `ai "hello"`
- [ ] `ai history --format tsv`
- [ ] `ai retry <history_id>`
- [ ] `ai rerun <history_id>`
- [ ] `aish shell` 内で `AI_ASK_LOG=session` が効く

### manual

- [ ] `aish shell` 内で `ai "..."` が current session log を自動参照する
- [ ] `ai status` / `ai doctor` が connection 診断を返す
- [ ] `ai ping` が socket の生存確認だけを行う
- [ ] `ai chat` が multi-turn の REPL として使える
- [ ] `Ctrl+C` が turn cancel として働く
- [ ] `--progress` が stderr に進行を出す
- [ ] `--yes-exec` の危険性と session 限定性が確認できる

## 実装時の禁止事項

- スタブ、仮実装、サンプル実装のまま完了扱いにしない
- `ai` から `aibe` / `aish` への path 依存を追加しない
- `ai` から LLM を直接呼ばない
- Phase C 以外で wire を増やさない
- `history` の index に raw message、raw log tail、秘密情報を載せない
- `--dry-run` で raw payload をそのまま表示しない
- `--yes-exec` を config default にしない
- 既存の `shell_exec` 承認の fail-closed を緩めない
- 既存テストの回帰を壊したまま Phase 完了としない

## 最終到達点

0027 の実装完了時は、Phase A / B / C の順で関連テストを通し、最後に `./scripts/verify.sh` を実行してから `docs/done/` への移動を行う。
