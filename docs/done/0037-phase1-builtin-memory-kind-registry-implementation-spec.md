# 0037 Phase 1 — Builtin MemoryKindRegistry 実装指示書

> **種別**: 実装指示書（`docs/done/`）  
> **設計正本**: [0037_aibe-contextual-memory-runtime-v1-spec.md](../spec/0037_aibe-contextual-memory-runtime-v1-spec.md) §6 / §14 Phase 1  
> **状態**: 実装済み（Phase 1）  
> **起票**: 2026-06-13

## 目的

`goal` / `now` / `idea` の固定分岐を AIBE domain の **MemoryKindRegistry** に集約し、built-in kind 定義（goal/now/rule/decision/idea/note）を正本化する。Phase 1 では filesystem 設定読み込みは行わず built-in のみ。

## 受け入れ条件

1. `MemoryKindDefinition` / `MemoryKindRegistry` が domain に存在し、6 種 built-in が定義されている
2. `validate_standard_kind_operation` が registry 参照で registered kind を検証する
3. `clear_kind_status_transition`（store）が registry の `clear_from` / `clear_to` を使う
4. `query_matches_idea_on_demand` が idea の registry `prompt.keywords` を参照する
5. `resolve_entries_for_prompt` が registry priority 順で pinned auto-inject（goal/now/rule）を解決する
6. **通常 query で active `rule` が prompt block に含まれる**
7. `goal` / `now` / `idea` の既存テストが互換（idea は通常 query で出ない）
8. registry 単体テストが 6 kind の属性を検証する
9. `./scripts/verify.sh` と `./scripts/smoke-mock.sh` が成功する

## 変更ファイル

| ファイル | 変更内容 |
|----------|----------|
| `aibe/src/domain/memory_kind_registry.rs` | **新規** — 型定義・built-in registry |
| `aibe/src/domain/mod.rs` | module 追加・export |
| `aibe/src/domain/contextual_memory.rs` | registry 参照へ置換、rule 注入、テスト追加 |
| `aibe/src/adapters/outbound/contextual_memory_store.rs` | clear/validate を registry 参照 |

## 実装手順

### 1. domain: `memory_kind_registry.rs`

- §6.3 の enum/struct を実装
- `MemoryKindRegistry::builtin()` で 6 kind を登録（§6.4 の TOML 相当）
- API: `get`, `is_registered`, `validate_operation`, `clear_transition`, `query_matches_on_demand`, `pinned_auto_inject_definitions`, `on_demand_definitions`
- `builtin_memory_kind_registry()` で `OnceLock` シングルトン

### 2. `contextual_memory.rs`

- `validate_standard_kind_operation`: registered kind は registry default と一致必須
- `is_standard_kind`: dedicated_cli あり built-in（goal/now/idea）— 既存 CLI hint 互換
- `query_matches_idea_on_demand`: idea definition の keywords 使用
- `resolve_entries_for_prompt`:
  - pinned auto-inject kinds を priority 昇順
  - SingleEffective: updated_at desc 先頭 1 件
  - Multiple: updated_at desc、max_entries 上限（0=None は無制限）
  - on-demand: Phase 1 は idea のみ（keywords マッチ時 open entries）

### 3. `contextual_memory_store.rs`

- `clear_kind_status_transition` を registry `clear_transition` に置換
- Add 時の `is_standard_kind` → `is_registered` + `validate_operation`

### 4. テスト

- `memory_kind_registry.rs`: 各 built-in の scope/inject/status/lifecycle/cardinality/priority
- `contextual_memory.rs`: `normal_query_includes_rule`, 既存 idea/goal/now テスト維持

## 正常系コマンド（Step 6）

```bash
./scripts/verify.sh
./scripts/smoke-mock.sh
```

## 非対象（Phase 1）

- filesystem `kinds.toml` 読み込み（Phase 2）
- `MemoryKindList` RPC / `ai mem kinds`（Phase 2）
- `MemoryOperationAdd` optional 化（Phase 2）
- decision/note の on-demand resolver（Phase 3）
- `MemoryResolverPolicy` 本格化（Phase 3）

## 完了時

- 本ファイルを `docs/done/0037-phase1-builtin-memory-kind-registry-implementation-spec.md` へ移動
- `docs/0000_spec-index.md` の tasks → done を更新
