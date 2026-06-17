# 0043 — Feature Pack Boundary Hardening Phase 2 実装指示書

> **種別**: 実装指示書（`docs/done/`）  
> **設計正本**: [0043_feature-pack-boundary-hardening-spec.md](../spec/0043_feature-pack-boundary-hardening-spec.md)  
> **状態**: 実装済み（Phase 2）  
> **起票**: 2026-06-17  
> **前提**: Phase 1 完了（`memory.enabled=false` ゲート / `log_tail_bytes` clamp）

## 0. 目的

Phase 2 では次を実装する。

1. `kind_files=[]` かつ `recipe_files=[]` かつ `feature_files=None` のとき baseline feature を読まない（generic memory 設定罠の解消）
2. feature 定義に `priority` / `requires_memory` / `requires_recipe` を導入し、trigger 候補を eligibility で落とす
3. `RoutePlan.recommended_tools` と `FeatureAction::SetRecommendedTools` を read-only tool のみに統一する

## 1. 受け入れ条件

| 条件 | 期待結果 |
|------|----------|
| generic memory 設定 | `kind_files=[]` + `recipe_files=[]` + `feature_files=None` で feature registry が empty |
| full AISH pack | `feature_files=None`（互換）で baseline feature が読み込まれる |
| eligibility | `requires_memory=true` で kinds 無効時は route_turn に feature が出ない |
| eligibility | `requires_recipe=true` で recipes 無効時は route_turn に feature が出ない |
| recommended_tools | top-level / `SetRecommendedTools` とも `shell_exec` を通さない |
| `./scripts/verify.sh` | 通過 |
| `./scripts/smoke-mock.sh` | 通過 |

## 2. 変更ファイル

| 区分 | パス | 内容 |
|------|------|------|
| protocol | `aibe-protocol/src/tool_name.rs` | read-only advisory tool の共有サニタイズ |
| loader | `aibe/src/adapters/outbound/filesystem_feature_registry.rs` | generic memory 時の baseline 抑止 |
| domain | `aibe/src/domain/feature_registry.rs` | eligibility フィールドと `match_eligible` |
| pack | `aibe/memory/packs/aish-memory/features.toml` | `requires_*` / `priority` 注釈 |
| route_turn | `aibe/src/application/route_turn.rs` | eligibility context / read-only tools |
| composition | `aibe/src/application/server.rs` | eligibility context 組み立て |
| request | `aibe/src/application/request_service.rs` | eligibility を route_turn へ渡す |
| ai | `ai/src/main.rs` | advisory tools を read-only に |
| ai | `ai/src/application/feature_executor.rs` | protocol 共有関数へ寄せる |
| tests | `aibe/...` / `ai/...` | Phase 2 unit / integration |
| docs | `docs/architecture.md`, `docs/testing.md`, `docs/manual/ai-smart-entry.md` | Phase 2 同期 |

## 3. 実装手順

### 3.1 generic memory 設定罠

`FilesystemFeatureRegistryLoader::load()` で `feature_files=None` のとき:

- `kind_files=Some([])` **かつ** `recipe_files=Some([])` なら `FeatureRegistry::empty()` を返す
- それ以外は従来どおり baseline pack

### 3.2 eligibility

- `FeatureDefinition` に `priority: u32`（既定 100）、`requires_memory: bool`、`requires_recipe: bool` を追加
- `FeatureEligibilityContext { memory_kinds_enabled, recipes_enabled }` を domain に定義
- `match_eligible_actions(query, ctx)` で trigger 一致後に eligibility を適用し、priority 昇順で actions を収集
- `features.toml`: `memory_context` → `requires_memory=true`、`clarify_goal` → `requires_recipe=true`

### 3.3 read-only recommended_tools

- `aibe-protocol` に `sanitize_readonly_advisory_tools` を追加（`read_file` / `list_dir` / `grep` / `git_diff` / `git_status` のみ。`shell_exec` と未知 tool は除外）
- `route_turn::finalize_route_plan` の top-level と `feature_actions` 内 `SetRecommendedTools` に適用
- `ai::sanitize_recommended_tools` も同関数を使う（defense-in-depth）

### 3.4 composition root

- `server.rs` で `FeatureEligibilityContext` を `MemoryConfig` から導出し `RequestService` 経由で `RouteTurnService` に渡す

## 4. Step 6

```bash
./scripts/verify.sh
./scripts/smoke-mock.sh
```
