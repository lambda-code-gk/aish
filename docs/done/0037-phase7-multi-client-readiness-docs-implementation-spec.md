# 0037 Phase 7 — Multi-client readiness docs 実装指示書

> **種別**: 実装指示書（`docs/done/`）  
> **設計正本**: [0037_aibe-contextual-memory-runtime-v1-spec.md](../spec/0037_aibe-contextual-memory-runtime-v1-spec.md) §9 / §10 / §14 Phase 7  
> **状態**: 実装済み（Phase 7）  
> **起票**: 2026-06-13

## 目的

Contextual Memory Runtime v1 の **multi-client 拡張に向けた docs/manual 整備** を行う。コード変更は行わない（Phase 6 までの実装を前提に、運用者・将来クライアント実装者が参照できる説明を追加する）。

## 受け入れ条件

1. 複数 `AI_SESSION_ID` から同一 `memory_space_id` の memory が共有される **具体例** がある（手順付き）
2. `AIBE_CONTEXT_ID` は **client-side selection** であり、サーバ `aibe` は読まないと明記されている
3. mobile profile は **shell execute を持たない** 設計と明記されている
4. 現在は **local runtime** であり **remote security は未実装** と明記されている
5. VSCode / TUI / mobile 等の **将来接続モデル**（Unix socket + stdio JSON、subscribe 専用接続）が説明されている
6. **capability 分離**（memory 操作 vs shell execute）が説明されている
7. **MemorySubscribe の v1 制限**（in-process broker、reconnect/replay 非対象、専用接続）が説明されている
8. `docs/manual/README.md` / `docs/architecture.md` / `docs/security.md` が Phase 7 内容と整合
9. `./scripts/verify.sh` と `./scripts/smoke-mock.sh` が成功する

## 実装サマリ

| ファイル | 内容 |
|----------|------|
| `docs/manual/contextual-memory-multi-client.md` | **新規** — multi-client readiness 正本 |
| `docs/manual/contextual-memory.md` | multi-client リンク、capability/subscribe 参照 |
| `docs/manual/README.md` | manual 一覧追記 |
| `docs/architecture.md` | capability boundary / multi-client 注記 |
| `docs/security.md` | capability 分離 / local runtime 注記 |

## 非対象

- mobile / VSCode extension 本体
- remote authentication / OAuth / token issue
- capability wire 追加
- subscribe の network transport

## 正常系コマンド

```bash
./scripts/verify.sh
./scripts/smoke-mock.sh
```
