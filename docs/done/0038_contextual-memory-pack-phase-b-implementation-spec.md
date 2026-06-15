# 0038 — Contextual Memory Pack Phase B 実装指示書

> **種別**: 実装指示書（`docs/done/`）  
> **設計正本**: [0038_contextual-memory-pack-phase-b-spec.md](../spec/0038_contextual-memory-pack-phase-b-spec.md)  
> **状態**: 実装済み（Phase B）  
> **起票**: 2026-06-15

## 目的

Phase A で導入した `[memory] enabled` の切替を、`aibe` 内部の **pack 境界** に昇格させる。

Phase B では次を実現する。

1. `agent_turn` の memory 注入を `TurnHook` trait に分離する
2. `memory_apply` / `memory_query` / `memory_kind_list` / `memory_recipe_run` / `memory_subscribe` の RPC 分岐を `RpcExtension` trait に分離する
3. `server.rs` で `memory.enabled` を見て `BasicPack` / `ContextualMemoryPack` を選ぶ

## 受け入れ条件

1. `server.rs` が `memory.enabled` を見て pack を組み立てる
2. `RequestService` から `memory_enabled: bool` が消え、memory RPC は `RpcExtension` 経由になる
3. `AgentTurnService` から `memory_enabled: bool` が消え、prompt 注入は `TurnHook` 経由になる
4. `enabled=false` では `BasicPack` が選ばれ、Phase A と同じ固定拒否メッセージ・no-op 注入が維持される
5. `enabled=true` では `ContextualMemoryPack` が選ばれ、既存の contextual memory 挙動が維持される
6. `memory_subscribe` は従来どおり専用接続契約を維持する
7. wire protocol と `aibe-protocol` は変更しない
8. `ai` クレートは Phase B 非対象として変更しない
9. `./scripts/verify.sh` と `./scripts/smoke-mock.sh` が通る
10. 必要な個別テストが追加され、既存の memory 回帰テストが壊れない

## 実装方針

Phase B の責務分離は次の 3 層で行う。

- `ports` に trait を置く
- `application/` に `BasicPack` / `ContextualMemoryPack` を置く
- `application/server.rs` を composition root にして、`RequestService` / `AgentTurnService` には pack trait だけを渡す

`memory.enabled` を参照するのは `server.rs` のみとする。`RequestService` / `AgentTurnService` は設定値を直接見ない。

## ファイル単位の実装手順

### 1. `aibe/src/ports/outbound/turn_hook.rs` を新規作成

- `TurnHook` trait を定義する
- `prepare_turn_messages` 相当の責務を、`AgentTurnContext` + `Vec<ChatMessage>` の入力で受けられる形にする
- `TurnHookError` を定義し、hook 側の失敗を表現する
- `BasicPack` が no-op を返せるようにする
- `mod.rs` から re-export する

### 2. `aibe/src/ports/outbound/rpc_extension.rs` を新規作成

- `RpcExtension` trait を定義する
- 対象 RPC は `memory_apply` / `memory_query` / `memory_kind_list` / `memory_recipe_run` / `memory_subscribe_begin`
- `memory_subscribe_begin` は `ClientResponse` と subscribe 用の実体を返せる形にする
- `BasicPack` が固定拒否を返せるようにする
- `mod.rs` から re-export する

### 3. `aibe/src/ports/outbound/mod.rs` を更新

- `TurnHook` / `TurnHookError`
- `RpcExtension`
- 必要なら pack 共有型
- を export する

### 4. `aibe/src/application/basic_memory_pack.rs` を新規作成

- `BasicPack` を定義する
- `TurnHook` 実装は no-op にする
- `RpcExtension` 実装は Phase A と同じ固定拒否メッセージを返す
- 既存の `memory_runtime.rs` の固定メッセージを再利用する
- `enabled=false` の契約をここに集約する

### 5. `aibe/src/application/contextual_memory_pack.rs` を新規作成

- `ContextualMemoryPack` を定義する
- 既存の `MemoryService` / `MemoryRecipeService` / `MemorySubscribeService` を内部で束ねる
- `TurnHook` 実装で current の `prepare_turn_messages` の memory 注入ロジックを移す
- `RpcExtension` 実装で current の `RequestService` 内 memory 分岐を移す
- `memory_subscribe` は初回応答までを extension が返し、その後の push loop は transport 側に残す
- capability policy と resolver / store / broker の配線はここで完結させる

### 6. `aibe/src/adapters/outbound/mod.rs` を更新

- `BasicPack` / `ContextualMemoryPack` を re-export する
- 既存の store / resolver / broker の export は維持する

### 7. `aibe/src/application/request_service.rs` を更新

- `memory_enabled` フラグを削除する
- `memory_store` / `memory_space_resolver` / `memory_kind_registry_loader` / `memory_broker` を直接保持しない形に整理する
- 新たに `Arc<dyn RpcExtension>` を保持する
- `handle_with_events` の `ClientRequest::MemoryApply` / `MemoryQuery` / `MemoryKindList` / `MemoryRecipeRun` / `MemorySubscribe` 分岐を `RpcExtension` へ委譲する
- `memory_subscribe` は `RpcExtension` が返した初回応答と subscription を使って、既存の transport push loop だけを継続する
- `AgentTurn` 分岐では `AgentTurnService` に `TurnHook` を渡す
- `RequestService::new*` 群は新しい依存形に合わせて整理する

### 8. `aibe/src/application/agent_turn.rs` を更新

- `memory_enabled` フラグを削除する
- `memory_store` / `memory_space_resolver` を直接保持しない形に整理する
- 新たに `Arc<dyn TurnHook>` を保持する
- `prepare_turn_messages` 関数は `TurnHook` 側へ移すか、薄いロジックだけ残して hook 呼び出しに置換する
- hook が失敗した場合は turn 全体を落とさず、元の messages を使う best-effort 方針を維持する
- 既存の system instruction / shell log tail / tool validation の順序は壊さない

### 9. `aibe/src/application/server.rs` を更新

- `memory_config.enabled` を読むのはここだけにする
- `enabled=false` なら `BasicPack` を組み立てる
- `enabled=true` なら `ContextualMemoryPack` を組み立てる
- `FilesystemContextualMemoryStore` / `EmptyContextualMemoryStore`
- `FilesystemMemorySpaceResolver`
- `InProcessMemorySubscriptionBroker`
- `shared_builtin_loader`
- をここで選び、pack のコンストラクタに渡す
- `RequestService` には `RpcExtension`、`AgentTurnService` には `TurnHook` を渡す

### 10. `aibe/src/application/mod.rs` と関連 export を整理

- 新しい pack 依存を public API から参照できるよう必要最小限の re-export を調整する
- 既存の application API 互換を壊さないようにする

### 11. テストを更新・追加する

- `aibe/src/adapters/outbound/basic_memory_pack.rs` に unit test を追加する
- `aibe/src/adapters/outbound/contextual_memory_pack.rs` に pack 結線の unit test を追加する
- `aibe/tests/memory_disabled.rs` は Phase A の拒否契約回帰として維持する
- `aibe/tests/contextual_memory.rs` は contextual memory の既存挙動回帰として維持する
- `aibe/tests/memory_subscribe.rs` は専用接続契約の回帰として維持する
- `aibe/tests/request_tool_validation.rs` / `aibe/tests/route_turn.rs` / `aibe/tests/agent_turn_loop.rs` / `aibe/tests/agent_turn_streaming.rs` / `aibe/tests/openai_compatible_llm.rs` / `aibe/tests/gemini_llm.rs` は、新しい constructor 形に合わせて更新する
- 既存テストで `RequestService::new_with_turns_and_registry_loader_and_memory` などを使っている箇所は、pack 注入形へ差し替える

### 12. docs を更新する

- `docs/architecture.md`
  - contextual memory の Phase B を pack 境界として明記する
  - `memory.enabled` を参照するのは `server.rs` のみ、という責務を追記する
  - `TurnHook` / `RpcExtension` / `BasicPack` / `ContextualMemoryPack` の位置づけを追記する
- `docs/testing.md`
  - pack boundary を担保するテストがどこにあるかを追記する
  - `verify.sh` / `smoke-mock.sh` と個別 `cargo test` の案内を必要なら補強する
- 手順や CLI 振る舞いが変わらない限り `docs/manual/contextual-memory.md` は原則更新しない

## 具体的な差し替え手順

### `RequestService` から `memory_enabled` を外す順序

1. `RequestService` のフィールドから `memory_enabled` を削除する
2. `memory_store` / `memory_space_resolver` / `memory_kind_registry_loader` / `memory_broker` の直接保持を削除する
3. コンストラクタ引数を `Arc<dyn RpcExtension>` 受け取りに変える
4. `MemoryApply` / `MemoryQuery` / `MemoryKindList` / `MemoryRecipeRun` を `rpc_extension` に委譲する
5. `MemorySubscribe` は `rpc_extension.memory_subscribe_begin` の結果を使って transport push loop を継続する
6. `AgentTurn` 分岐で `AgentTurnService` に `TurnHook` を渡す

### `AgentTurnService` から `memory_enabled` を外す順序

1. `AgentTurnService` のフィールドから `memory_enabled` を削除する
2. `memory_store` / `memory_space_resolver` の直接保持を削除する
3. コンストラクタ引数を `Arc<dyn TurnHook>` 受け取りに変える
4. `prepare_turn_messages` の memory 注入部分を `turn_hook` に移す
5. hook 失敗時は元の messages を使う best-effort を維持する
6. system instruction / shell log tail / tool validation の既存順序を回帰テストで固定する

### `server.rs` composition root の変更順序

1. `memory_config.enabled` の分岐で `BasicPack` / `ContextualMemoryPack` を生成する
2. memory store / resolver / broker / registry loader を pack にまとめる
3. `RequestService` には `RpcExtension` だけを渡す
4. `AgentTurnService` には `TurnHook` だけを渡す
5. `server.rs` 以外から `memory.enabled` を参照していないことを確認する

## 非対象

- `aibe-protocol` の wire schema 変更
- `ai` クレートの改修
- memory クレート分離
- 動的プラグインロード
- Phase C 以降の CLI / builtin kind 移行

## 正常系コマンド

```bash
./scripts/verify.sh
./scripts/smoke-mock.sh
cargo test -p aibe memory_disabled
cargo test -p aibe contextual_memory
cargo test -p aibe memory_subscribe
```

必要なら以下も個別に回す。

```bash
cargo test -p aibe request_tool_validation
cargo test -p aibe route_turn
cargo test -p aibe agent_turn_loop
cargo test -p aibe agent_turn_streaming
cargo test -p aibe openai_compatible_llm
cargo test -p aibe gemini_llm
```
