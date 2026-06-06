# 0029 — `ai` UX 仕上げ 実装指示書

> **設計書**: [0029_ai-ux-polish-spec.md](../spec/0029_ai-ux-polish-spec.md)  
> **状態**: 進行中

## 実装順

1. history GC（domain / ports / local_history / record_turn）
2. `--yes-exec` integration tests
3. aibe multi-delta streaming test
4. docs 同期（testing.md、example config、0000 index）

## 1. history GC

### 変更ファイル

- `ai/src/adapters/outbound/toml_config.rs` — `history_max_entries`（default 500）
- `ai/src/ports/outbound/history_store.rs` — `prune_to_max`
- `ai/src/adapters/outbound/local_history.rs` — 実装 + unit test
- `ai/src/application/history.rs` — `record_turn` 後に prune
- `ai/src/main.rs` — `AiConfig.history_max_entries` を渡す

### 仕様

- `history_max_entries == 0` → prune スキップ
- 保持: 新しい `created_at_ms` 優先。同値は `history_id` 降順

## 2. yes-exec integration

### 新規

- `ai/tests/yes_exec_integration.rs`

### ケース

1. seed `yes-exec/global.json` → mock approval server → `ai ask --yes-exec` → `approved=true`、stderr に `non-interactive stdin` なし
2. 空 cache → 同上 → denied
3. ai.toml preset `shell_exec_approval=never` → cache seed あっても denied（yes_exec_effective false）

Mock server: `aibe-client/tests/agent_turn_approval.rs` と同型の approval 往復。

環境: `AIBE_CONFIG` で `shell_exec_approval = "ask"`、`AI_CONFIG` で socket/history。

## 3. streaming

### 変更

- `aibe/src/adapters/outbound/scripted_mock_llm.rs` — `StreamingScriptedMockLlm` または既存に `with_streaming_deltas`
- `aibe/tests/agent_turn_streaming.rs`（新規）— delta 回数 assert

## 4. docs

- `docs/0000_spec-index.md` — 0029 追加
- `docs/testing.md` — yes_exec / GC テスト追記
- example: `docs/aibe.config.example.toml` は変更最小

## 受け入れ条件チェックリスト

- [x] yes-exec seeded cache integration
- [x] yes-exec empty cache denied
- [x] preset never blocks yes-exec
- [x] history prune unit/integration
- [x] aibe multi-delta streaming test
- [x] verify.sh + smoke-mock.sh
