# 0038 — Contextual Memory Pack Phase A（basic プロファイル切替）設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定（Phase A 実装済み）  
> **起票**: 2026-06-14  
> **関連**: [0037_aibe-contextual-memory-runtime-v1-spec.md](0037_aibe-contextual-memory-runtime-v1-spec.md)、[architecture.md](../architecture.md)

## 目的

contextual memory を **同一バイナリのまま設定で無効化** し、basic ランタイム（agent loop / route_turn / tools / conversation のみ）として動かせるようにする。将来の Pack 化（TurnHook / RpcExtension）の第一歩。

## 非目標（Phase A）

- memory クレート分離・動的プラグインロード
- built-in kind の TOML 完全移行
- `ai goal` 等 CLI のビルド時除外

## 設定

### aibe（`~/.config/aibe/config.toml`）

```toml
[memory]
enabled = false   # 省略時 true（従来互換）
```

環境変数オーバーライド: `AIBE_MEMORY_ENABLED=0` / `false` / `no` / `off`

### ai（`~/.config/ai/config.toml`）

```toml
[memory]
enabled = false   # 省略時 true
```

環境変数オーバーライド: `AI_MEMORY_ENABLED=0` / `false` / `no` / `off`

**運用**: basic 利用時は **aibe と ai の両方** で `enabled = false` にすること（片方のみだと CLI とサーバの挙動がずれる）。

## 無効時の挙動

| 経路 | 挙動 |
|------|------|
| `agent_turn` | memory block を注入しない |
| `memory_apply` / `memory_query` / `memory_kind_list` / `memory_recipe_run` | `InvalidRequest` + 固定メッセージ |
| `memory_subscribe` | 同上（専用接続の初回応答） |
| `route_turn` | 変更なし（もともと memory を見ない） |
| `ai goal` / `now` / `idea` / `mem` / `context` | 起動前にエラー終了 |
| `ai ask` 等 | `memory_space_id` を送らない |

## 実装要点

- composition root: `memory.enabled == false` なら `EmptyContextualMemoryStore` + builtin registry loader
- `RequestService.memory_enabled` で RPC ガード
- `AgentTurnService.memory_enabled` で `prepare_turn_messages` の注入スキップ

## 受け入れ条件

- [x] `[memory] enabled = false` で上記挙動
- [x] 省略時は従来どおり memory 有効
- [x] `aibe/tests/memory_disabled.rs` / `ai/tests/memory_disabled_cli.rs`
- [x] `./scripts/verify.sh` 成功

## 次フェーズ（参考）

- Phase B: TurnHook / RpcExtension trait 化
- Phase C: CLI / built-in kind の pack 移行
