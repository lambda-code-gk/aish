# 0037 Phase 6 — Capability model 実装指示書

> **種別**: 実装指示書（`docs/done/`）  
> **設計正本**: [0037_aibe-contextual-memory-runtime-v1-spec.md](../spec/0037_aibe-contextual-memory-runtime-v1-spec.md) §10 / §14 Phase 6 / §15.7  
> **状態**: 実装済み（Phase 6）  
> **起票**: 2026-06-13

## 目的

`Capability` を AIBE 側のドメイン境界として導入し、memory 操作権限と shell 実行権限を application service boundary で分離する。`ai` 側には capability model を持ち込まず、現在の CLI 互換を壊さずに将来の multi-client 拡張の土台だけを入れる。

## 受け入れ条件

1. `Capability` domain type が `aibe` 側に存在し、`ai` 側には置かれない
2. `CapabilityPolicy` が存在し、application service boundary で capability check を行う
3. 既定 `local_full` profile は現行 CLI 互換で、全 capability を許可する
4. `MemoryRead` は `MemoryQuery` を通す
5. `MemoryWrite` は `MemoryApply(Add)` を通す
6. `MemoryArchive` は `MemoryApply(Archive)` と `MemoryApply(ClearKind)` を通す
7. `MemoryRecipeRun` は recipe 実行を通す（材料収集のため `MemoryRead` も要求）
8. `MemorySubscribe` は subscribe を通す
9. `ShellExecute` は memory capabilities と独立して判定される
10. `memory_read_only` profile は write/archive を拒否する
11. `memory_only` profile は `shell_exec` 実行を拒否する
12. capability check は adapter ではなく application boundary にある
13. `./scripts/verify.sh` と `./scripts/smoke-mock.sh` が成功する

## 実装サマリ

| レイヤー | 内容 |
|----------|------|
| `capability.rs` | **新規** — `Capability` enum、operation → capability helper |
| `capability_policy.rs` (port) | **新規** — `CapabilityPolicy` trait、`CapabilityDenied` |
| `capability_policy.rs` (adapter) | **新規** — `StaticCapabilityPolicy`、`local_full` / `memory_read_only` / `memory_only` |
| `memory_service.rs` | `MemoryRead` / `MemoryWrite` / `MemoryArchive` gate |
| `memory_recipe_service.rs` | `MemoryRecipeRun` + `MemoryRead` gate、apply 時 write/archive gate |
| `memory_subscribe_service.rs` | `MemorySubscribe` gate |
| `agent_turn.rs` | `AgentAsk` / `ShellPropose` gate、`ToolExecutionContext` へ policy 伝播 |
| `shell_exec.rs` | `ShellExecute` gate（approval とは独立） |
| `request_service.rs` | composition root から policy を各 service へ注入 |
| テスト | `aibe/tests/capability.rs`（8 件）+ domain/adapter unit tests |

## 非対象（Phase 7+）

- multi-client readiness docs
- remote authentication / token / OAuth
- capability wire 追加

## 正常系コマンド

```bash
./scripts/verify.sh
./scripts/smoke-mock.sh
```
