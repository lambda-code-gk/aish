# 0034 — AIBE Contextual Memory MVP 実装指示書（完了）

> **設計書**: [spec/0034_aibe-contextual-memory-spec.md](../spec/0034_aibe-contextual-memory-spec.md)  
> **状態**: 実装済み

## 実装サマリ

1. `aibe-protocol` — `MemoryApply` / `MemoryQuery` / DTO / `MemoryApplyResult` / `MemoryQueryResult`
2. `aibe` domain — `contextual_memory.rs`（validation, resolve, format block）
3. `aibe` port/adapter — `ContextualMemoryStore`, JSONL filesystem store
4. `aibe` — `MemoryService`, `RequestService` dispatch, `AgentTurn` 注入
5. `ai` — `goal` / `now` / `idea` / `mem` CLI, `AibeUnixClient` memory methods
6. docs — architecture, security, manual

## 受け入れ条件

- `ai goal set` / `now set` / `idea add` / `show` / `list` が動作
- 同一 `AI_SESSION_ID` で store 共有
- 通常 ask で goal/now 注入、idea は on-demand のみ
- `./scripts/verify.sh` 成功
