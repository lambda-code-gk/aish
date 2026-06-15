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

環境変数オーバーライド: `AIBE_MEMORY_ENABLED=0` / `false` / `no` / `off`（有効側: `1` / `true` / `yes` / `on`）

**優先順位**: 環境変数 > config ファイル > 既定値 `true`。未知の env 値は無視し、下位の設定を維持する。

### ai（`~/.config/ai/config.toml`）

```toml
[memory]
enabled = false   # 省略時 true
```

環境変数オーバーライド: `AI_MEMORY_ENABLED=0` / `false` / `no` / `off`（有効側: `1` / `true` / `yes` / `on`）

**優先順位**: 環境変数 > config ファイル > 既定値 `true`。未知の env 値は無視し、下位の設定を維持する。

**運用**: basic 利用時は **aibe と ai の両方** で `enabled = false` にすること（片方のみだと CLI とサーバの挙動がずれる）。

## 無効時の挙動

| 経路 | 挙動 |
|------|------|
| `agent_turn` | memory block を注入しない |
| `memory_apply` / `memory_query` / `memory_kind_list` / `memory_recipe_run` | `InvalidRequest` + 固定メッセージ |
| `memory_subscribe` | 同上（専用接続の初回応答） |
| `route_turn` | 変更なし（もともと memory を見ない） |
| `ai goal` / `now` / `idea` / `mem` / `context` | 起動前にエラー終了（`context` は selection UI も memory pack の一部のため無効化対象） |
| `ai ask` 等 | `memory_space_id` を送らない（`AIBE_CONTEXT_ID` / config `[context].current` も参照しない） |

## 実装要点

- composition root: `memory.enabled == false` なら `EmptyContextualMemoryStore` + `BuiltinMemoryKindRegistryLoader`（静的 built-in 定義のみ。`<AIBE_ROOT>/memory/*.toml` の parse/merge は完全にスキップし、破損 TOML で basic 起動不能にならない）
- `RequestService.memory_enabled` で RPC ガード
- `AgentTurnService.memory_enabled` で `prepare_turn_messages` の注入スキップ

## 受け入れ条件

- [x] `[memory] enabled = false` で上記挙動
- [x] 省略時は従来どおり memory 有効
- [x] 未知の env 値は無視し下位設定を維持
- [x] disabled mode では `<AIBE_ROOT>/memory/*.toml` を読まない
- [x] `aibe/tests/memory_disabled.rs` / `ai/tests/memory_disabled_cli.rs`
- [x] 環境変数 `AIBE_MEMORY_ENABLED` / `AI_MEMORY_ENABLED` で config を上書きできる
- [x] `docs/architecture.md` に basic プロファイル注記
- [x] `./scripts/verify.sh` 成功

## 次フェーズ（参考）

- Phase B: [0038_contextual-memory-pack-phase-b-spec.md](0038_contextual-memory-pack-phase-b-spec.md) — TurnHook / RpcExtension trait 化、Pack 合成
- Phase C: CLI / built-in kind の pack 移行
