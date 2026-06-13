# 0037 Phase 2 — Add defaulting + MemoryKindList RPC 実装指示書

> **種別**: 実装指示書（`docs/done/`）  
> **設計正本**: [0037_aibe-contextual-memory-runtime-v1-spec.md](../spec/0037_aibe-contextual-memory-runtime-v1-spec.md) §6.7 / §6.6 / §14 Phase 2  
> **状態**: 実装済み（Phase 2）  
> **起票**: 2026-06-13

## 目的

`MemoryOperationAdd` の optional 化と registry defaulting を実装し、`MemoryKindList` RPC と `ai mem kinds` CLI を追加する。

## 受け入れ条件

1. `{"op":"add","kind":"rule","text":"..."}` が registry default で追加できる
2. registered kind は `kind + text` のみで server 側 default 補完される
3. registered kind で client 指定が registry と矛盾する場合は error
4. unregistered kind は省略時 server 既定（`project/manual/open`）で補完、explicit 指定も可
5. `make_active`: SingleEffective → default true、Multiple → default false。明示 `make_active=false` on SingleEffective は error
6. 既存 explicit DTO（scope/inject/status/make_active 指定）も動作する
7. unknown field rejection は維持
8. `MemoryKindList` RPC が built-in 6 kind を返す
9. `ai mem kinds` が kind 一覧を表示する（tsv / json / env）
10. `ai mem add` は `kind + text` のみ送信（defaulting は AIBE 正本）
11. `./scripts/verify.sh` と `./scripts/smoke-mock.sh` が成功する

## 実装サマリ

| レイヤー | 内容 |
|----------|------|
| protocol | `MemoryOperationAdd` optional 化、`MemoryKindList` RPC、`MemoryKindDefinitionDto` |
| domain | `resolve_memory_operation_add`（registered / unregistered server defaulting） |
| application | `MemoryService` normalize + `kind_list` |
| ai CLI | `ai mem kinds`、`ai mem add` は kind+text のみ |
| テスト | protocol / domain / integration / `phase_a_cli` E2E |

## 設計メモ（レビュー反映）

- unregistered kind の CLI 既定値は **AIBE server** に集約（`ai` は policy を持たない）
- `SingleEffective` + 明示 `make_active=false` は cardinality 保護のため error（§6.7 設計判断）
- `ai mem kinds --format env` は `kinds[N].field='value'` 形式

## 非対象（Phase 2）

- filesystem `kinds.toml` 読み込み
- `MemoryResolverPolicy` 本格化（Phase 3）
- MemoryRecipe / Subscribe / Capability（Phase 4+）

## 正常系コマンド

```bash
./scripts/verify.sh
./scripts/smoke-mock.sh
```
