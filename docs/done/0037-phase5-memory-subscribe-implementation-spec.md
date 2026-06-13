# 0037 Phase 5 — MemorySubscribe 実装指示書

> **種別**: 実装指示書（`docs/done/`）  
> **設計正本**: [0037_aibe-contextual-memory-runtime-v1-spec.md](../spec/0037_aibe-contextual-memory-runtime-v1-spec.md) §9 / §14 Phase 5 / §15.1 / §15.6  
> **状態**: 実装済み（Phase 5）  
> **起票**: 2026-06-13

## 目的

`MemorySubscribe` / `MemoryChanged` protocol と in-process broker を実装し、同一 aibe process 内で MemoryApply / RecipeApply による memory 変更を subscribe 専用接続へ push する。

## 受け入れ条件

1. `MemorySubscribe` / `MemorySubscribeResult` / `MemoryChanged` / `MemoryChangeEventDto` / `MemoryChangeKind` / `MemorySubscribeStatus` が protocol に追加され roundtrip できる
2. `MemorySubscriptionBroker` port と `in_process_memory_subscription_broker` adapter がある
3. MemoryApply 成功時に broker へ publish する（Add→Added, ClearKind→StatusChanged, Archive→Archived）
4. MemoryRecipeRun `apply=true` 成功時に RecipeApplied を publish する
5. subscribe 専用接続: `MemorySubscribeResult` 後に `MemoryChanged` を push。他 RPC 混在は error
6. 接続切断で subscriber が broker から解放される
7. `memory_space_id` が一致する subscriber のみが event を受信する
8. request body の `kind` filter が効く
9. `./scripts/verify.sh` と `./scripts/smoke-mock.sh` が成功する

## 実装サマリ

| レイヤー | 内容 |
|----------|------|
| `aibe-protocol` | subscribe request/response DTO、roundtrip tests |
| `memory_subscription.rs` | domain: `MemoryChangeEvent`, `MemoryChangeKind`, filter |
| `memory_subscription_broker.rs` | port trait |
| `in_process_memory_subscription_broker.rs` | **新規** — mpsc + subscriber registry |
| `memory_subscribe_service.rs` | **新規** — subscribe RPC、socket push loop |
| `memory_service.rs` / `memory_recipe_service.rs` | apply 後 publish |
| `request_service.rs` / `server.rs` | broker 配線、subscribe handler |
| `unix_socket_server.rs` | subscribe 専用接続モデル |
| テスト | `aibe/tests/memory_subscribe.rs`（broker + socket） |

## 非対象（Phase 6+）

- Capability model / boundary check
- `ai mem subscribe` CLI（Phase 5 完了条件に含まれない）
- reconnect / replay / remote subscription

## 正常系コマンド

```bash
./scripts/verify.sh
./scripts/smoke-mock.sh
```
