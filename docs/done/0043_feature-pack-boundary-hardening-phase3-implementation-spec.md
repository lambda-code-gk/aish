# 0043 — Feature Pack Boundary Hardening Phase 3 実装指示書

> **種別**: 実装指示書（`docs/done/`）  
> **設計正本**: [0043_feature-pack-boundary-hardening-spec.md](../spec/0043_feature-pack-boundary-hardening-spec.md)  
> **状態**: 実装済み（Phase 3）  
> **起票**: 2026-06-17  
> **前提**: Phase 1 / Phase 2 完了（`docs/done/0043_feature-pack-boundary-hardening-*.md`）

## 0. 目的

Phase 3 では、feature pack を memory pack から明示的に切り離し、将来の `[features]` 分離に備える。ただし本 Phase では TOML `[features]` の parse / 読み込みは行わない。

実装の狙いは次の 3 点である。

1. `FeaturePackConfig` を明示的な設定モデルとして導入し、feature registry の入力を 1 か所へ集約する
2. composition root が `MemoryConfig` から `FeaturePackConfig` を解決し、generic memory 時の `feature_files=None` を空 registry として扱う
3. `FilesystemFeatureRegistryLoader` を `FeaturePackConfig` のみ参照する形へ寄せ、`kind_files` / `recipe_files` / `memory.enabled` への暗黙依存をなくす

## 1. 受け入れ条件

以下は実装後に検証可能な条件として固定する。

| 条件 | 期待結果 |
|------|----------|
| feature pack 分離 | `FeaturePackConfig` が `MemoryConfig` から独立した型として存在し、`feature_files` だけを保持する |
| composition root 解決 | `aibe/src/application/server.rs` が `MemoryConfig` から `FeaturePackConfig` を解決し、feature registry の初期状態を 1 回だけ決める |
| generic memory | `kind_files=[]` + `recipe_files=[]` + `feature_files=None` で feature registry は empty になる |
| full AISH pack 互換 | `memory.enabled=true` かつ generic memory ではない場合、`feature_files=None` は baseline pack 互換を維持する |
| explicit empty | `feature_files=[]` は常に empty registry であり、baseline pack に落ちない |
| loader 境界 | `FilesystemFeatureRegistryLoader` は `FeaturePackConfig` のみを参照し、`kind_files` / `recipe_files` / `memory.enabled` を直接見ない |
| Phase 2 保持 | `memory.enabled=false` では引き続き `FeatureRegistry::empty()` が使われる |
| Phase 2 保持 | `priority` / `requires_memory` / `requires_recipe` による eligibility は変化しない |
| Phase 2 保持 | `RoutePlan.recommended_tools` と `FeatureAction::SetRecommendedTools` の read-only 境界は変化しない |
| 非目標維持 | `[features]` TOML 節の parse / 読み込みは追加しない |
| Step 6 | `./scripts/verify.sh` が通る |
| Step 6 | `./scripts/smoke-mock.sh` が通る |

## 2. レイヤー別 実装タスク

### 2.1 domain

1. `FeaturePackConfig` を domain 側に導入し、`feature_files` のみを保持する。
2. `effective_feature_mode` を domain の責務として表現し、少なくとも次の分岐を型または enum で明示する。
   - `Empty`
   - `BaselineCompat`
   - `ExplicitFiles`
3. generic memory 判定は composition root の入力として受けるが、domain 側の `FeaturePackConfig` 自体には `memory.enabled` を持たせない。
4. `FeatureRegistryLoader` や `FilesystemFeatureRegistryLoader` が参照する判定ロジックを domain の明示モデルに寄せる。

対象の中心は `aibe/src/domain/feature_registry.rs` である。必要なら新規モジュールを切り、`aibe/src/domain/mod.rs` で再 export する。

### 2.2 ports

1. `MemoryConfig` から `FeaturePackConfig` を構成できるよう、ports 側に解決用 API を追加する。
2. 既存の `MemoryConfig::memory_kinds_enabled()` / `recipes_enabled()` / `is_explicit_generic_memory_pack()` は Phase 3 でも保持し、generic memory の解釈に使う。
3. `FeaturePackConfig` を loader 入力として扱うため、`aibe/src/ports/outbound/config.rs` と `aibe/src/ports/outbound/mod.rs` を更新する。
4. loader の trait 契約は維持しつつ、実装側が `FeaturePackConfig` を受け取る構造に切り替える。

### 2.3 adapters

1. `FilesystemFeatureRegistryLoader` を `FeaturePackConfig` のみ参照する実装へ変更する。
2. `feature_files=None` の互換モードは loader 単体ではなく、composition root から渡された effective mode に従って解釈する。
3. `feature_files=[]` は loader 単体でも empty registry になることを unit test で固定する。
4. `feature_files` が複数指定された場合は、現行どおり順に merge する。
5. loader の unit test は Phase 3 の責務分離を反映するように置き換える。

対象の中心は `aibe/src/adapters/outbound/filesystem_feature_registry.rs` である。

### 2.4 composition root

1. `aibe/src/application/server.rs` で `MemoryConfig` から `FeaturePackConfig` を組み立てる。
2. `memory.enabled=false` の場合は、これまでどおり `FeatureRegistry::empty()` を渡す。
3. `memory.enabled=true` かつ generic memory の場合は、`feature_files=None` を empty registry として解決する。
4. `memory.enabled=true` かつ generic memory でない場合は、`feature_files=None` を baseline pack 互換として扱う。
5. `RequestService::new_with_turns_and_packs` へ渡す feature registry の生成責務を server.rs に集約し、loader が composition root を再現しないようにする。

## 3. 変更ファイル一覧

以下はこの Phase で変更対象とする具体的パスである。

| 区分 | パス | 内容 |
|------|------|------|
| domain | `aibe/src/domain/feature_registry.rs` | `FeaturePackConfig` / `effective_feature_mode` / 既存 registry API の調整 |
| domain | `aibe/src/domain/mod.rs` | 新しい domain 型の re-export |
| ports | `aibe/src/ports/outbound/config.rs` | `FeaturePackConfig` を解決するための設定型追加 |
| ports | `aibe/src/ports/outbound/mod.rs` | 新しい ports 型の re-export |
| adapter | `aibe/src/adapters/outbound/filesystem_feature_registry.rs` | `FeaturePackConfig` ベースの loader へ変更 |
| composition root | `aibe/src/application/server.rs` | `MemoryConfig` から `FeaturePackConfig` を解決し registry を組み立てる |
| integration test | `aibe/tests/feature_pack_boundary.rs` | generic memory / full pack / explicit empty の回帰固定 |
| integration test | `aibe/tests/memory_disabled.rs` | `memory.enabled=false` の Phase 2 挙動維持を確認 |
| unit test | `aibe/src/adapters/outbound/filesystem_feature_registry.rs` | loader 境界の unit 追加 / 更新 |
| unit test | `aibe/src/domain/feature_registry.rs` | `FeaturePackConfig` / effective mode の unit 追加 |
| docs | `docs/architecture.md` | feature pack 分離後の責務分界を更新 |
| docs | `docs/testing.md` | Phase 3 の unit / integration / regression 観点を更新 |
| docs | `docs/aibe.config.example.toml` | 既存 `[memory]` 設定の注記を Phase 3 に合わせて補正 |
| docs | `docs/manual/ai-smart-entry.md` | `memory.enabled=false` / smart feature 無効条件の説明を更新 |

## 4. テスト計画

### 4.1 unit

1. `FeaturePackConfig` が `feature_files=None` / `[]` / 非空で正しく分岐することを固定する。
2. `effective_feature_mode` が `memory.enabled` と pack 状態から一意に決まることを固定する。
3. `FilesystemFeatureRegistryLoader` が `FeaturePackConfig` のみを使い、explicit empty と baseline compat を分けて扱うことを固定する。
4. 既存の Phase 2 unit は、eligibility / read-only tools の振る舞いを変えていないことを確認する目的で再実行する。

### 4.2 integration

1. `aibe/tests/feature_pack_boundary.rs` で、generic memory 時に `feature_files=None` でも baseline feature が復活しないことを固定する。
2. 同テストで、full AISH pack 相当の条件では `feature_files=None` が baseline pack 互換であることを固定する。
3. `feature_files=[]` が常に empty registry であることを固定する。
4. `aibe/tests/memory_disabled.rs` で、`memory.enabled=false` が Phase 2 と同じく feature registry を empty に保つことを確認する。
5. 既存 integration への回帰として、`priority` / `requires_memory` / `requires_recipe` の判定結果が変わらないことを確認する。

### 4.3 既存テストの更新方針

1. Phase 2 で `MemoryConfig` にぶら下がっていた feature loader unit は、Phase 3 の責務分離に合わせて期待値を更新する。
2. `feature_files=None` の意味は loader 単体ではなく composition root の解決結果で説明する。
3. 既存の `docs/testing.md` の 0043 項目は、Phase 3 unit / integration / regression を追記して更新する。
4. `aibe/tests/route_turn.rs` や `ai/tests/smart_feature_plan.rs` に Phase 3 のための挙動変更が不要なら、期待値は触らず再利用する。

## 5. 回帰条件

Phase 3 で壊してはいけない条件を明示する。

1. `memory.enabled=false` は引き続き `FeatureRegistry::empty()` になる。
2. `kind_files=[]` / `recipe_files=[]` / `feature_files=[]` は generic memory の明示形として維持される。
3. `feature_files=None` は、generic memory でない場合に限って baseline pack 互換を維持する。
4. `feature_files=None` でも generic memory を選んだ場合は baseline feature を復活させない。
5. Phase 2 の eligibility 判定はそのまま維持する。
6. Phase 2 の read-only tools 境界はそのまま維持する。
7. `route_turn` の `feature_actions` 取り扱いは、`memory.enabled=false` で引き続き strip される。

## 6. docs 更新対象

次の docs を実装差分と同じ PR / 同じ変更で更新する。

1. `docs/architecture.md`
2. `docs/testing.md`
3. `docs/aibe.config.example.toml`
4. `docs/manual/ai-smart-entry.md`

更新内容は次を含む。

1. feature pack が memory pack から独立した設定面であること
2. `FeaturePackConfig` と `MemoryConfig` の責務分割
3. `feature_files=None` / `[]` の意味の違い
4. `[features]` TOML 節は未導入であること
5. Phase 2 の挙動維持条件

## 7. Step 6

```bash
./scripts/verify.sh
./scripts/smoke-mock.sh
```

## 8. 完了条件

1. Phase 3 の本番経路が実装されている。
2. 追加した unit / integration が通る。
3. `./scripts/verify.sh` が通る。
4. `./scripts/smoke-mock.sh` が通る。
5. docs 同期が実装差分に含まれる。

## 9. 未確定事項 / 残リスク

### 未確定・推測・指示外

- `FeaturePackConfig` の最終的なモジュール配置を新規ファイルに分けるか、既存 `feature_registry.rs` / `config.rs` に寄せるかは実装時に既存命名規則へ合わせて決める
- `[features]` TOML 節の将来設計は本 Phase の範囲外

### 残リスク

- `feature_files=None` の互換モードと generic memory の判定を composition root に寄せるため、既存テストの期待値更新漏れが起きる可能性がある
- `docs/aibe.config.example.toml` と `docs/manual/ai-smart-entry.md` は実装の最終型に合わせて微修正が必要になる可能性がある
- `./scripts/smoke-mock.sh` は feature pack の分離自体ではなく、結果としての route_turn 挙動を確認するため、loader 単体の保証は unit test 側で補う必要がある
