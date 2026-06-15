# 0038 — Contextual Memory Pack Phase A 実装指示書

> **種別**: 実装指示書（`docs/done/`）  
> **設計正本**: [0038_contextual-memory-pack-phase-a-spec.md](../spec/0038_contextual-memory-pack-phase-a-spec.md)  
> **状態**: 実装済み（Phase A）  
> **起票**: 2026-06-15

## 目的

contextual memory を **同一バイナリのまま設定で無効化** し、basic ランタイム（agent loop / route_turn / tools / conversation のみ）として動かせるようにする。

## 受け入れ条件

1. `[memory] enabled = false`（aibe / ai 各 config）で memory 注入・RPC・CLI が設計どおり無効化される
2. 省略時は従来どおり memory 有効（`MemoryConfig::default().enabled == true`）
3. 環境変数 `AIBE_MEMORY_ENABLED` / `AI_MEMORY_ENABLED` で config を上書きできる（`0`/`false`/`no`/`off` と `1`/`true`/`yes`/`on`）
4. memory 無効時は `BuiltinMemoryKindRegistryLoader` のみ使用し、`<AIBE_ROOT>/memory/*.toml` を読まない
5. `aibe/tests/memory_disabled.rs` / `ai/tests/memory_disabled_cli.rs` が成功する
6. `docs/architecture.md` に basic プロファイル注記がある
7. `./scripts/verify.sh` と `./scripts/smoke-mock.sh` が成功する

## 実装サマリ

### aibe

| ファイル | 内容 |
|----------|------|
| `ports/outbound/config.rs` | `MemoryConfig { enabled: bool }` 追加（既定 `true`） |
| `adapters/outbound/toml_config.rs` | `[memory] enabled` パース、`AIBE_MEMORY_ENABLED` env |
| `adapters/outbound/env_config.rs` | env 経路でも `MemoryConfig` を読む |
| `adapters/outbound/contextual_memory_store.rs` | `EmptyContextualMemoryStore`（常に空） |
| `adapters/outbound/filesystem_memory_kind_registry.rs` | `shared_builtin_loader()` → `BuiltinMemoryKindRegistryLoader` |
| `application/memory_runtime.rs` | `MEMORY_DISABLED_MESSAGE` / `memory_disabled_response` |
| `application/server.rs` | composition root: disabled 時 `EmptyContextualMemoryStore` + builtin loader |
| `application/request_service.rs` | `memory_enabled` フラグ、memory RPC / subscribe ガード |
| `application/agent_turn.rs` | `prepare_turn_messages` で `memory_enabled == false` なら注入スキップ |

### ai

| ファイル | 内容 |
|----------|------|
| `adapters/outbound/toml_config.rs` | `[memory] enabled`、`AI_MEMORY_ENABLED` env、`ensure_memory_enabled()` |
| `main.rs` | `goal`/`now`/`idea`/`mem`/`context` で `ensure_memory_enabled()`、`ask` で `memory_space_id` 省略 |

### テスト

| ファイル | 内容 |
|----------|------|
| `aibe/tests/memory_disabled.rs` | memory_apply/query/kind_list/recipe_run 拒否、agent_turn 注入スキップ、破損 kinds.toml でも disabled 起動 |
| `ai/tests/memory_disabled_cli.rs` | `goal set` / `context current` 拒否、`ask` が `memory_space_id` を送らない、`AI_MEMORY_ENABLED` env 上書き |

### docs

| ファイル | 内容 |
|----------|------|
| `docs/architecture.md` | basic プロファイル（0038 Phase A）注記 |
| `docs/spec/0038_contextual-memory-pack-phase-a-spec.md` | 設計書 |

## 非対象

- memory クレート分離・動的プラグインロード（Phase B 以降）
- built-in kind の TOML 完全移行
- `ai goal` 等 CLI のビルド時除外

## 正常系コマンド

```bash
./scripts/verify.sh
./scripts/smoke-mock.sh
cargo test -p aibe memory_disabled
cargo test -p ai memory_disabled_cli
```
