# 0043 — Feature Pack Boundary Hardening 実装指示書

> **種別**: 実装指示書（`docs/tasks/`）  
> **設計正本**: [0043_feature-pack-boundary-hardening-spec.md](../spec/0043_feature-pack-boundary-hardening-spec.md)  
> **状態**: 実装済み（Phase 1）  
> **起票**: 2026-06-17  
> **対象**: Phase 1 のみ

## 0. 目的

0042 で `FeatureAction` 実行基盤、`features.toml`、`route_turn` の action schema 明示までは成立したが、`memory.enabled=false` でも baseline smart feature が残る経路があり、`RoutePlan.log_tail_bytes` の top-level clamp も未完成である。

このタスクの目的は、Phase 1 として次を固めることである。

1. `memory.enabled=false` のとき `aibe` の feature registry を **empty** にする
2. `route_turn` の top-level `log_tail_bytes` を protocol 上限で clamp する
3. 関連テストと docs を同一差分で同期する

## 1. 実装スコープ

### 1.1 Phase 1（本タスクで実装）

- `memory.enabled=false` では `FilesystemFeatureRegistryLoader::load()` を通さず、`FeatureRegistry::empty()` を composition root から渡す
- `route_turn` の finalization で `log_tail_bytes` を `SHELL_LOG_TAIL_MAX_BYTES` に clamp する
- `effective registry` が empty の場合、`route_turn` の feature catalog / feature_actions が残らないことを固定する
- loader / route_turn / disabled-memory の回帰テストを追加する
- `docs/architecture.md`、`docs/testing.md`、`docs/manual/ai-smart-entry.md`、必要なら `docs/aibe.config.example.toml` を更新する

### 1.2 Phase 2（今回は実装対象外、記載のみ）

- `kind_files=[]` / `recipe_files=[]` と `feature_files=None` の組み合わせを、AISH feature が残る設定罠にしない
- `RoutePlan.recommended_tools` と `FeatureAction::SetRecommendedTools` を read-only に統一する
- `priority`、`requires_memory`、`requires_recipe` を導入して trigger の eligibility を分離する

### 1.3 Phase 3（今回は実装対象外、記載のみ）

- `[features]` セクションへ memory / feature 設定を分離する

## 2. 受け入れ条件

| 条件 | 期待結果 |
|------|----------|
| `memory.enabled=false` | `aibe` は `FeatureRegistry::empty()` を使い、baseline smart feature を読まない |
| `route_turn.log_tail_bytes` | `SHELL_LOG_TAIL_MAX_BYTES` を超えない |
| `memory.enabled=false` の `route_turn` | feature catalog / feature_actions を返さない |
| `feature_files=None` | 既存の baseline 互換挙動を維持する |
| `feature_files=[]` | 空 registry を返す |
| `./scripts/verify.sh` | 通過する |
| `./scripts/smoke-mock.sh` | 通過する |

## 3. 変更ファイル一覧

| 区分 | 具体的パス | 変更内容 |
|------|------------|----------|
| composition root | `aibe/src/application/server.rs` | `memory.enabled=false` 時は `FeatureRegistry::empty()` を組み立て、loader を呼ばない |
| route_turn | `aibe/src/application/route_turn.rs` | `log_tail_bytes` の clamp と、empty registry 時の feature_actions 無効化を実装する |
| loader unit | `aibe/src/adapters/outbound/filesystem_feature_registry.rs` | `feature_files=None` / `feature_files=[]` の境界を unit test で固定する |
| integration | `aibe/tests/memory_disabled.rs` | disabled memory で route_turn が feature_actions を残さない回帰を追加する |
| unit | `aibe/src/application/route_turn.rs` | top-level `log_tail_bytes` clamp の unit test を追加する |
| docs | `docs/architecture.md` | `memory.enabled=false` で feature registry が empty になることと top-level clamp を追記する |
| docs | `docs/testing.md` | 0043 Phase 1 の unit / integration / smoke の検証観点を追記する |
| docs | `docs/manual/ai-smart-entry.md` | disabled-memory 時の smart feature 挙動を現行仕様に合わせて更新する |
| docs | `docs/aibe.config.example.toml` | 必要なら `[memory] enabled = false` の注記を更新する |

## 4. 実装手順

### 4.1 `server.rs` で feature registry の正本を切り替える

1. `aibe/src/application/server.rs` で `memory_config.enabled` を見て、`false` のときは `FilesystemFeatureRegistryLoader::load()` を呼ばない。
2. `false` の場合は `FeatureRegistry::empty()` をそのまま `RequestService` へ渡す。
3. `true` の場合だけ loader を使って registry を構築する。
4. ここで `feature_files=None` の baseline 互換は維持し、Phase 1 の変更範囲を `memory.enabled=false` に限定する。

### 4.2 `route_turn` の finalization を harden する

1. `aibe/src/application/route_turn.rs` の finalization で `log_tail_bytes` を `SHELL_LOG_TAIL_MAX_BYTES` に clamp する。
2. effective registry が empty の場合は、LLM の生出力に feature_actions が含まれていても最終 `RoutePlan` には残さない。
3. registry が非空のときだけ、既存の merge ロジックで `feature_actions` を補完する。
4. 既存の redaction / route_kind 正規化 / recommended_tools の扱いは Phase 1 では変えない。

### 4.3 loader と disabled-memory の回帰を固定する

1. `aibe/src/adapters/outbound/filesystem_feature_registry.rs` に unit test を追加し、`feature_files=None` は baseline 互換、`feature_files=[]` は empty になることを固定する。
2. `aibe/tests/memory_disabled.rs` に integration test を追加し、disabled memory で route_turn を実行したとき feature_actions が残らないことを固定する。
3. 可能なら `ScriptedMockLlm` を使い、LLM が feature_actions を返しても disabled では最終 `RoutePlan` に出ないことを確認する。
4. 既存の memory RPC 拒否・server 起動回帰は壊さない。

### 4.4 docs を同期する

1. `docs/architecture.md` に、`memory.enabled=false` の場合は feature registry が empty になることを追記する。
2. `docs/architecture.md` に、`route_turn.log_tail_bytes` の top-level clamp を追記する。
3. `docs/testing.md` の smart feature 章に、Phase 1 の unit / integration / smoke の追加観点を追記する。
4. `docs/manual/ai-smart-entry.md` の smart feature 章を、disabled-memory の挙動と現在の境界に合わせる。
5. `docs/aibe.config.example.toml` は、注記が実装後の意味とずれるなら最小差分で補正する。

### 4.5 Step 6 の検証を通す

1. `./scripts/verify.sh` を通す。
2. `./scripts/smoke-mock.sh` を通す。
3. 失敗時は `server.rs` → `route_turn.rs` → test → docs の順で差分を確認する。

## 5. テスト追加箇所

| 種別 | ファイル | 観点 |
|------|----------|------|
| unit | `aibe/src/adapters/outbound/filesystem_feature_registry.rs` | `feature_files=None` baseline 互換、`feature_files=[]` empty |
| unit | `aibe/src/application/route_turn.rs` | `log_tail_bytes` が `SHELL_LOG_TAIL_MAX_BYTES` で clamp される |
| integration | `aibe/tests/memory_disabled.rs` | `memory.enabled=false` で route_turn の feature_actions が消える |
| integration | `aibe/tests/memory_disabled.rs` | broken `memory/*.toml` があっても disabled 起動が壊れない |

## 6. docs 同期対象

| ファイル | 更新内容 |
|----------|----------|
| `docs/architecture.md` | `memory.enabled=false` のとき `FeatureRegistry::empty()` を使うこと、`route_turn.log_tail_bytes` の clamp を明記する |
| `docs/testing.md` | 0043 Phase 1 の unit / integration / smoke の検証表を追加する |
| `docs/manual/ai-smart-entry.md` | disabled-memory 時の smart feature 挙動を現行実装に合わせて更新する |
| `docs/aibe.config.example.toml` | 必要なら `[memory] enabled = false` 注記を更新する |

`docs/0000_spec-index.md` は今回触らない。

## 7. Step 6 コマンド

```bash
./scripts/verify.sh
./scripts/smoke-mock.sh
```

## 8. 完了条件

1. Phase 1 の変更が実装される。
2. Phase 1 の unit / integration が追加される。
3. `./scripts/verify.sh` が通る。
4. `./scripts/smoke-mock.sh` が通る。
5. docs 同期が実装差分に含まれる。

