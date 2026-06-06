# 0028 — `ai` UX ギャップ解消 実装指示書

> **種別**: 実装指示書（`docs/tasks/` → 完了後 `docs/done/`）  
> **状態**: 進行中  
> **設計の正本**: [0028_ai-ux-gap-closure-spec.md](../spec/0028_ai-ux-gap-closure-spec.md)  
> **起票**: 2026-06-06  
> **関連**: [architecture.md](../architecture.md)、[security.md](../security.md)、[testing.md](../testing.md)、[ai-ux.md](../manual/ai-ux.md)、[0000_spec-index.md](../0000_spec-index.md)

## 目的

0028 の残ギャップを、本番経路・正しい境界・既存テスト方針のまま閉じる。  
実装は **Phase 1 → Phase 4** の順で進め、`ai` のクライアント責務と `aibe` のプロバイダ責務を混ぜない。  
最終到達点は `./scripts/verify.sh` の成功と、指示書を `docs/done/` へ移すこと。

## 実装順

1. Phase 1: `ai chat` の client-side transcript と exit code 分岐を固定する
2. Phase 2: preset の `shell_exec_approval` と `--dry-run` の秘匿ルールを確定する
3. Phase 3: provider streaming と synthetic delta の収束を実装する
4. Phase 4: `turn_cancel` の `Ctrl+C` handler が 1 回だけ登録されることを再確認する

## ファイル単位の変更リスト

| クレート | 変更対象ファイル | 役割 |
|----------|------------------|------|
| `ai` | `ai/src/main.rs` | `chat` の transcript 管理、exit code 変換、`--dry-run` の最終分岐 |
| `ai` | `ai/src/application/ask.rs` | turn 実行の共通化、`chat` / `retry` / `rerun` の接続点 |
| `ai` | `ai/src/application/history.rs` | `conversation_id` 付き history record / replay envelope の整形 |
| `ai` | `ai/src/application/turn_cancel.rs` | `Ctrl+C` handler の singleton 維持と回帰確認 |
| `ai` | `ai/src/domain/history.rs` | local history の schema。`conversation_id`、redacted index、replay payload の定義 |
| `ai` | `ai/src/domain/reports.rs` | `status` / `doctor` / `ping` / `dry-run` の秘匿済み view |
| `ai` | `ai/src/domain/ask.rs` / `ai/src/domain/llm_profile.rs` | preset と CLI の解決順、`shell_exec_approval` の最終解決値 |
| `ai` | `ai/src/adapters/outbound/toml_config.rs` | `history_dir`、`log_tail_bytes`、`[presets.*]`、`[ask].filter`、`shell_exec_approval` の読み込み |
| `ai` | `ai/src/adapters/outbound/local_history.rs` | `index.jsonl` と payload vault の分離、`conversation_id` の保存 |
| `ai` | `ai/src/adapters/outbound/stdout_presenter.rs` | `--dry-run` / 診断系の表示、filter / log tail のマスク |
| `ai` | `ai/src/adapters/outbound/yes_exec_cache.rs` | `--yes-exec` の session scoped 記憶 |
| `ai` | `ai/src/adapters/outbound/aibe_client.rs` | `chat` / `ask` / `retry` / `rerun` の送信経路。必要なら exit code 分岐用のエラー変換も整理する |
| `ai` | `ai/tests/*`（主に `ask_integration.rs`、`history_cli.rs`） | transcript、history、dry-run、exit code、`--yes-exec` の統合回帰 |
| `aibe` | `aibe/src/application/agent_turn.rs` | assistant streaming、cancel、max-round fallback の orchestration |
| `aibe` | `aibe/src/application/request_service.rs` | wire request の turn / cancel entrypoint と event sink の結線 |
| `aibe` | `aibe/src/application/server.rs` | `TurnEventSink` への progress / assistant streaming の転送 |
| `aibe` | `aibe/src/application/protocol_convert.rs` | stream / final の response 変換を維持する。必要時のみ最小差分 |
| `aibe` | `aibe/src/ports/outbound/llm.rs` | provider streaming を支える必要がある場合のみ trait を拡張する |
| `aibe` | `aibe/src/ports/outbound/turn_events.rs` | progress / assistant streaming の event 契約を維持・確認する |
| `aibe` | `aibe/src/adapters/outbound/openai_compatible.rs` | OpenAI-compatible の真の streaming / fallback delta |
| `aibe` | `aibe/src/adapters/outbound/gemini.rs` | Gemini の streaming / fallback delta |
| `aibe` | `aibe/src/adapters/outbound/mock_llm.rs` | mock の streaming 模擬、最低限の synthetic delta |
| `aibe` | `aibe/src/adapters/outbound/scripted_mock_llm.rs` | scripted mock の streaming fixture 追加 |
| `aibe` | `aibe/src/adapters/outbound/llm_backend.rs` | streaming HTTP consume helper の共通化 |
| `aibe` | `aibe/tests/*`（主に `openai_compatible_llm.rs`、`gemini_llm.rs`、`agent_turn_loop.rs`、`ai_ask_e2e.rs`） | provider streaming、assistant delta 順序、cancel、final response の回帰 |
| `aibe-protocol` | `aibe-protocol/src/response.rs` / `aibe-protocol/src/request.rs` | 既存の `assistant_streaming` / `CancelTurn` / `ErrorCode` を正本として維持する。必要になったときのみ最小差分 |
| `aibe-client` | `aibe-client/src/transport.rs` / `aibe-client/src/lib.rs` | もし event stream / cancel の整流に差分が必要ならここで吸収する |
| `docs` | `docs/architecture.md`、`docs/security.md`、`docs/testing.md`、`docs/ai.config.example.toml`、`docs/manual/ai-ux.md`、`docs/0000_spec-index.md` | 実装と同じ変更で同期する |

## Phase 1: client-side transcript と exit code

### 対象

`ai chat` の transcript を client 側に保持し、`history` に `conversation_id` を残し、終了コードを `0/2/3/4/5/130` に分解する。

### 実装手順

1. `ai/src/main.rs` で `chat` 起動時に空の transcript と 1 つの `conversation_id` を生成する
2. `ai/src/main.rs` の `run_chat` / `execute_turn` 相当の経路で、前回までの user/assistant のみを次 turn の `messages` に積む
3. `ai/src/main.rs` で `AgentTurnResult` 完了時のみ transcript を append し、`Error` / `Cancelled` では更新しない
4. `ai/src/application/history.rs` と `ai/src/domain/history.rs` で `conversation_id` を history record / replay payload に追加する
5. `ai/src/adapters/outbound/local_history.rs` で `conversation_id` を index と payload の両方に保持し、`retry` / `rerun` で失わないようにする
6. `ai/src/main.rs` の top-level error mapping を整理し、`invalid_request` / local validation を `2`、transport / decode / timeout / 非 SIGINT cancel を `3`、provider error を `4`、tool error / timeout / not allowed を `5`、SIGINT を `130` に固定する
7. `ai/src/application/history.rs` の unit と `ai/tests/ask_integration.rs` で、chat の複数 turn が同一 `conversation_id` のまま保存されることを固定する

### Phase 1 の test gate

```bash
cargo test -p ai --tests
cargo test -p aibe-client --tests
```

### Phase 1 の受け入れ条件

- `ai chat` が client-side transcript を使う
- transcript は model-visible な user / assistant のみを保持する
- tool の中間 message は aibe 側に閉じる
- `history` に `conversation_id` が残る
- exit code が `0/2/3/4/5/130` に分岐する

## Phase 2: preset と dry-run の秘匿

### 対象

`shell_exec_approval` の最終優先順位を固定し、`--dry-run` の report から raw filter / raw log tail / raw replay payload を除去する。

### 実装手順

1. `ai/src/adapters/outbound/toml_config.rs` で preset の `shell_exec_approval` と `ask.filter` を読み込む
2. `ai/src/domain/ask.rs` / `ai/src/domain/llm_profile.rs` で、CLI 明示値 > preset > aibe config の順序を最終解決する
3. `ai/src/adapters/outbound/yes_exec_cache.rs` で session scoped の `--yes-exec` 記憶を使い、`shell_exec_approval=never` を越えないことを維持する
4. `ai/src/domain/reports.rs` と `ai/src/adapters/outbound/stdout_presenter.rs` で `dry-run` / diagnostics の表示から raw message、raw log tail、raw filter、raw replay payload を除去する
5. `ai/src/adapters/outbound/local_history.rs` と `ai/src/domain/history.rs` で、`index.jsonl` は redacted metadata のみ、payload vault は replay 用最小情報のみ、という分離を維持する
6. `docs/ai.config.example.toml` には `shell_exec_approval` の例を維持し、`ask.filter` / `history_dir` / `log_tail_bytes` の更新点を反映する
7. `ai/tests/ask_integration.rs` と `ai/tests/history_cli.rs` で、`--dry-run` が aibe に接続せず、filter / log tail を漏らさないことを固定する

### Phase 2 の test gate

```bash
cargo test -p ai --tests
```

### Phase 2 の受け入れ条件

- `shell_exec_approval` の最終値が CLI / preset / config の順で決まる
- `--yes-exec` は `shell_exec_approval=ask` の場合だけ意味を持つ
- `--dry-run` が raw filter を表示しない
- `--dry-run` が raw shell log tail を表示しない
- `--dry-run` が raw replay payload を表示しない

## Phase 3: provider streaming

### 対象

OpenAI-compatible / Gemini / mock の provider で、真の streaming がある場合は delta を順次 emit し、非対応時は synthetic delta を 1 回だけ emit する。

### 実装手順

1. `aibe/src/ports/outbound/llm.rs` を確認し、既存の `complete` / `complete_with_tools` だけでは足りない場合のみ streaming 用の最小拡張を入れる
2. `aibe/src/application/agent_turn.rs` で assistant streaming の event sink を turn の進行に接続し、final response だけでなく delta を表示できるようにする
3. `aibe/src/application/server.rs` と `aibe/src/application/request_service.rs` で、streaming と cancel の経路を request/response orchestration に結線する
4. `aibe/src/adapters/outbound/openai_compatible.rs` で、backend が stream 対応なら真の streaming を使い、非対応なら synthetic delta 1 回で収束する
5. `aibe/src/adapters/outbound/gemini.rs` でも同じ方針を適用し、provider 差を「stream を本当に受けるか」に閉じる
6. `aibe/src/adapters/outbound/mock_llm.rs` と `aibe/src/adapters/outbound/scripted_mock_llm.rs` で、テストが streaming / fallback delta を再現できるようにする
7. `aibe/src/adapters/outbound/llm_backend.rs` に、streaming HTTP の consume / parse helper が必要なら共通化する
8. `aibe/tests/openai_compatible_llm.rs`、`aibe/tests/gemini_llm.rs`、`aibe/tests/agent_turn_loop.rs`、`aibe/tests/ai_ask_e2e.rs` で、delta 順序と final response の意味が崩れないことを固定する

### Phase 3 の test gate

```bash
cargo test -p aibe --tests
cargo test -p ai --tests
```

### Phase 3 の受け入れ条件

- streaming 対応 provider は生成中に delta を emit する
- streaming 非対応 provider は synthetic delta を 1 回だけ emit する
- final `AgentTurnResult` の本文は従来どおり canonical な assistant message である
- client 側の可視結果が provider ごとに壊れない

## Phase 4: 既存 `Ctrl+C` handler の再確認

### 対象

`turn_cancel` の handler が process 全体で 1 回だけ登録されることを確認し、`chat` や `execute_turn` 側で新しい SIGINT handler を重ねない。

### 実装手順

1. `ai/src/application/turn_cancel.rs` を起点に、`Once` ベースの singleton registration が残っていることを確認する
2. `ai/src/main.rs` の `chat` / `ask` / `retry` / `rerun` 経路に、新しい SIGINT handler 登録を追加しない
3. もし現状が仕様を満たしていれば、新規コードは追加せず、既存実装の参照確認と回帰テストのみで閉じる
4. 必要であれば `ai/tests/ask_integration.rs` または `ai/src/application/turn_cancel.rs` の unit で、重複登録が起きないことを固定する
5. `docs/manual/ai-ux.md` に `Ctrl+C` の手動確認手順が既にあるため、必要なら Phase 4 の期待結果だけ追記する

### Phase 4 の test gate

```bash
cargo test -p ai --tests
```

### Phase 4 の受け入れ条件

- `turn_cancel` の handler は 1 回だけ登録される
- `chat` の REPL ループや `execute_turn` から handler を追加しない
- `Ctrl+C` の意味が transport failure や timeout と混同されない

## docs 更新対象

実装と同じ変更で、以下を更新する。

- `docs/architecture.md`  
  client-side transcript、`conversation_id`、exit code の意味分け、preset の `shell_exec_approval` 優先順位、provider streaming の収束点を追記する
- `docs/security.md`  
  transcript は client process 内に閉じること、`--dry-run` の秘匿、`--yes-exec` の危険性、SIGINT と timeout の区別を追記する
- `docs/testing.md`  
  Phase 1-4 のテスト配置、`ai` / `aibe` / `aibe-client` の責務分担、manual で確認する項目を追記する
- `docs/ai.config.example.toml`  
  `history_dir`、`log_tail_bytes`、`[ask].filter`、`[presets.*]`、`shell_exec_approval` の例示を維持・更新する
- `docs/manual/ai-ux.md`  
  `ai chat` の transcript、`--dry-run` の秘匿、`Ctrl+C`、`--yes-exec` の確認項目を更新する
- `docs/0000_spec-index.md`  
  本指示書の `docs/tasks/` 版を追加し、完了時に `docs/done/` へ移す

## 受け入れ条件チェックリスト

### unit

- [x] `chat` の transcript が turn ごとに蓄積される
- [x] `conversation_id` が local history に記録される
- [x] exit code が `0/2/3/4/5/130` に分岐する
- [x] preset / `--yes-exec` / aibe config の優先順位が固定される
- [x] streaming 非対応 provider が synthetic delta 1 回を emit する
- [x] `--dry-run` の report が filter の生文字列を含まない
- [x] `turn_cancel` の handler 二重登録が起きない

### integration

- [x] `ai chat` で複数 turn の transcript が同じ `conversation_id` で history に残る
- [x] `ai ask --dry-run` が raw filter / raw log tail / raw replay payload を出さない
- [x] `--yes-exec` が `shell_exec_approval=ask` のときだけ効く
- [ ] streaming 対応 provider では delta が逐次表示される
- [x] streaming 非対応 provider でも 1 回の synthetic delta 後に final response が出る

### manual

- [ ] `Ctrl+C` で 130 が返る
- [ ] `ai chat` で turn を跨いだ会話が維持される
- [ ] `ai ask --dry-run` で filter の中身が見えない

## 実装時の禁止事項

- スタブ、仮実装、サンプル実装のまま完了扱いにしない
- `ai` から `aibe` / `aish` への path 依存を追加しない
- `ai` から LLM を直接呼ばない
- `chat` の transcript を aibe 側に永続化しない
- `history` の index に raw message、raw log tail、秘密情報を載せない
- `--dry-run` で raw payload をそのまま表示しない
- `--yes-exec` を config default にしない
- 既存の `shell_exec` 承認の fail-closed を緩めない
- `turn_cancel` の singleton registration を壊して handler を二重登録しない
- 既存テストの回帰を壊したまま Phase 完了としない

## 最終到達点

0028 の実装完了時は、Phase 1 / 2 / 3 / 4 の順で関連テストを通し、最後に `./scripts/verify.sh` を実行する。  
すべて成功したら、この指示書を `docs/done/` へ移し、`docs/0000_spec-index.md` の実装指示書欄を更新する。
