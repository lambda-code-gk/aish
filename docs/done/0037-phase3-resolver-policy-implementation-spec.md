# 0037 Phase 3 — ResolverPolicy 実装指示書

> **種別**: 実装指示書（`docs/done/`）  
> **設計正本**: [0037_aibe-contextual-memory-runtime-v1-spec.md](../spec/0037_aibe-contextual-memory-runtime-v1-spec.md) §7 / §14 Phase 3  
> **状態**: 実装済み（Phase 3）  
> **起票**: 2026-06-13

## 目的

Phase 1 の簡易 `resolve_entries_for_prompt` を `MemoryResolverPolicy` に置き換え、pinned / explicit / related / recent の選択順と registry priority 順 prompt block を実装する。

## 受け入れ条件

1. `MemoryResolveInput` / `MemoryResolverPolicy` が `aibe/src/domain/memory_resolver_policy.rs` に存在する
2. 通常 query で goal/now/rule が注入され、idea は出ない
3. idea 系 query（keywords / alias）で open idea が注入される（goal 言及含む §0.2）
4. decision は明示 query（alias「決定」「方針」等）で active decision が注入される
5. 「方針」query で rule（pinned）と decision（on-demand）の両方が priority 順で出うる
6. inactive / archived は prompt block に入らない
7. Open status は explicit 要求時のみ（related でも Open は explicit kind 必須）
8. prompt block footer / budget / truncation 既存挙動を維持
9. §15.4 resolver tests が domain に追加される
10. `./scripts/verify.sh` と `./scripts/smoke-mock.sh` が成功する

## 実装サマリ

| レイヤー | 内容 |
|----------|------|
| `memory_resolver_policy.rs` | **新規** — 5 段選択 + dedup + `enforce_kind_limits` + priority sort |
| `memory_kind_registry.rs` | `kind_explicitly_requested`（id / alias / on_demand keywords） |
| `contextual_memory.rs` | `resolve_entries_for_prompt` → policy 委譲 |
| テスト | `memory_resolver_policy` 11 件 + registry alias テスト |

## 設計メモ（レビュー反映）

- related フェーズ後に `enforce_kind_limits` で cardinality / max_entries を再適用
- `query_matches_on_demand` は `kind_explicitly_requested` へ委譲（alias 対応）

## 非対象（Phase 3）

- MemoryRecipe / Subscribe / Capability（Phase 4+）
- vector search / fallback summary 実装
- filesystem `kinds.toml`

## 正常系コマンド

```bash
./scripts/verify.sh
./scripts/smoke-mock.sh
```
