# 0037 Phase 4 — MemoryRecipe 実装指示書

> **種別**: 実装指示書（`docs/done/`）  
> **設計正本**: [0037_aibe-contextual-memory-runtime-v1-spec.md](../spec/0037_aibe-contextual-memory-runtime-v1-spec.md) §8 / §14 Phase 4 / §15.5 / §15.8  
> **状態**: 実装済み（Phase 4）  
> **起票**: 2026-06-13

## 目的

`clarify-goal` の MemoryRecipe を AIBE の正本機能として実装し、`MemoryRecipeRun` protocol、LLM 出力検証、apply 前確認、`ai mem run clarify-goal` CLI を本番経路で通す。

## 受け入れ条件

1. `MemoryRecipeRun` protocol が追加され、`MemoryRecipeProposalDto` / `MemoryRecipeStatus` を含めて roundtrip できる
2. `clarify-goal` が open idea / active goal / active now / active rule / active decision を材料にする
3. clarify-goal の LLM 出力は `{"summary": "...", "proposals": [...]}` の単一 JSON オブジェクトのみを受け付ける
4. markdown fence や unknown field を含む LLM 出力は error になる
5. `proposals[].operation` は registry validation を通り、`Add` 以外は error になる
6. `proposals[].rationale` は表示されるが store には保存されない
7. `apply=false` では store が変化しない
8. `apply=true` では validation 済み memory operation のみが適用される
9. recipe は shell operation を生成・実行しない
10. `ai mem run clarify-goal` が提案を表示する
11. `ai mem run clarify-goal --apply` は確認を要求し、非対話 stdin では fail-closed する
12. `./scripts/verify.sh` と `./scripts/smoke-mock.sh` が成功する

## 実装サマリ

| レイヤー | 内容 |
|----------|------|
| `aibe-protocol` | `MemoryRecipeRun` / `MemoryRecipeRunResult` / `MemoryRecipeProposalDto` / `MemoryRecipeStatus` |
| `memory_recipe.rs` | **新規** — 材料収集、LLM JSON 検証、prompt 生成 |
| `memory_recipe_service.rs` | **新規** — clarify-goal 実行、default LLM profile、apply 分岐 |
| `request_service.rs` | `MemoryRecipeRun` ディスパッチ |
| `memory_cli.rs` / `clap_cli.rs` | `ai mem run clarify-goal` / `--apply` / `--instruction` |
| `memory_recipe_approval_ui.rs` | **新規** — apply 前確認（shell_exec とは別） |
| テスト | `aibe/tests/memory_recipe.rs` 8 件 + `phase_a_cli` 2 件 + protocol roundtrip |

## 非対象（Phase 5+）

- MemorySubscribe / Capability / subscription broker

## 正常系コマンド

```bash
./scripts/verify.sh
./scripts/smoke-mock.sh
```
