# 0043 — Feature Pack Boundary Hardening 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **起票**: 2026-06-17  
> **関連**: [0038_contextual-memory-pack-phase-a-spec.md](0038_contextual-memory-pack-phase-a-spec.md)、[0039_aish-memory-pack-externalization-spec.md](0039_aish-memory-pack-externalization-spec.md)、[0041_ai-smart-feature-plan-spec.md](0041_ai-smart-feature-plan-spec.md)、[0042_configurable-smart-features-spec.md](0042_configurable-smart-features-spec.md)、[architecture.md](../architecture.md)、[testing.md](../testing.md)

## 0. 目的

0042 で `FeatureAction` 実行基盤、`features.toml`、`route_turn` の action schema 明示、履歴 summary 分離までは成立した。しかし現状のままでは、pack を外したつもりでも baseline smart feature が残る経路と、`RoutePlan` の top-level advisory に安全境界が残る経路があり、設定だけで「basic aibe / generic memory / full AISH pack」の 3 状態を明確に分けられない。

本仕様の目的は次の 3 点である。

1. `memory.enabled=false` のときは **basic aibe** に確実に落とす
2. `memory.enabled=true` のときの memory pack と feature pack の有効・無効境界を明確化する
3. `route_turn` から出る不整合な feature / advisory を事前に抑制し、`ai` 側で危険な解釈をしない

0042 の §8.2「`memory.enabled=false` でも feature registry をロードする」は撤回し、`memory.enabled=false` では **empty registry** を返す。

## 1. 非目標

- `docs/0000_spec-index.md` の更新
- `aish` に memory / feature plan の責務を追加すること
- 動的プラグインロード
- `RoutePlan` wire の破壊的変更
- Windows 対応
- `[features]` セクション分離の実装

## 2. 現状ギャップ

### 2.1 `memory.enabled=false` でも baseline feature が残る

[aibe/src/application/server.rs](../../aibe/src/application/server.rs) は `memory_config.enabled` に関係なく `FilesystemFeatureRegistryLoader::load()` を呼んでいる。`feature_files` が `None` の場合、loader は baseline pack を返すため、basic aibe に戻したい設定でも smart feature が残る。

### 2.2 `kind_files=[]` / `recipe_files=[]` でも feature が残る設定罠

0039 は `kind_files=[]` / `recipe_files=[]` を generic memory への明示的な無効化として扱ったが、`feature_files` が未指定のままだと 0042 の baseline feature が残る。結果として、利用者は「memory pack を外したのに AISH feature が効く」ように見える。

### 2.3 top-level `RoutePlan.log_tail_bytes` が未 clamp

`FeatureAction::SetLogTailBytes` は `SHELL_LOG_TAIL_MAX_BYTES` で clamp されるが、`RoutePlan.log_tail_bytes` は `aibe` の finalize と `ai` の advisory 適用で上限が保証されていない。`route_turn` 経由の提案がそのまま大きな値を通す余地がある。

### 2.4 top-level `RoutePlan.recommended_tools` が shell_exec を通し得る

`RoutePlan.recommended_tools` は 0030 の advisory として残っているため、`ai/src/main.rs` の `apply_route_plan_advisory` は `shell_exec` に相当する tool 名を受け取り得る。FeatureAction 経路は read-only に寄せているのに、top-level advisory だけがより広い権限を持つのは境界不整合である。

### 2.5 trigger は部分一致のみ

現在の trigger 判定は部分一致のみであり、feature の適用優先度や、memory / recipe 依存の有無は表現していない。将来 `priority`、`requires_memory`、`requires_recipe` を加えない限り、不整合な feature を route_turn に出し続ける可能性がある。

## 3. 設計概要

### 3.1 状態モデル

本仕様は memory / feature の設定状態を 3 つに分けて扱う。

| 状態 | 設定 | 期待結果 |
|------|------|----------|
| basic aibe | `memory.enabled = false` | memory runtime は無効、feature registry は empty、route_turn の feature も出ない |
| generic memory | `memory.enabled = true` かつ pack を明示的に空にする | generic memory primitive は有効、AISH 固有 kind / recipe / feature は出ない |
| full AISH pack | `memory.enabled = true` かつ pack を有効にする | AISH kind / recipe / feature が有効になる |

generic memory の明示例は `kind_files=[]`、`recipe_files=[]`、`feature_files=[]` である。full AISH pack は互換モードの既定値または明示的な pack ファイル列挙で表現する。

### 3.2 effective registry の優先順位

feature registry の**最終的な有効/無効判定**は `memory.enabled` を第一ゲートとし、次に `feature_files` を解釈する。ただし `memory.enabled` の判定そのものは composition root の責務であり、`FilesystemFeatureRegistryLoader` は `feature_files` の解釈だけを担う。

1. `memory.enabled=false` なら registry は常に empty
2. `memory.enabled=true` で `feature_files=[]` なら registry は empty
3. `memory.enabled=true` で `feature_files` が非空なら列挙ファイルのみを読む
4. `memory.enabled=true` で `feature_files=None` は互換モードとして baseline pack を読む

この優先順位により、basic aibe と generic memory を設定だけで分離する。

### 3.3 route_turn の安全境界

`route_turn` が返す `RoutePlan` は、LLM の生出力をそのまま返さず、サーバ側で正規化した結果であるべきである。特に次の 2 つは top-level でも安全化する。

- `log_tail_bytes` は `SHELL_LOG_TAIL_MAX_BYTES` を上限に clamp する
- `recommended_tools` は Phase 2 で read-only tool のみを残し、`shell_exec` 系は route_turn の advisory として通さない

feature action 側と top-level advisory 側で安全規則が分かれるのではなく、`route_turn` の出力として最終的に同じ境界に収束させる。`aibe` は `RoutePlan` をこの境界に正規化する一次責務を持ち、`ai` 側の sanitization は defense-in-depth であり、境界を広げてはならない。

### 3.4 trigger から eligibility を分離する

trigger は現状どおり部分一致を基本とするが、将来は「trigger に当たった feature をそのまま返す」のではなく、「trigger で候補化し、eligibility で落とす」構造に分ける。

eligibility の判定には次を使う。

- `priority`
- `requires_memory`
- `requires_recipe`

この分離により、`memory.enabled=false` なのに memory 依存 feature が route_turn に出る、といった不整合を抑止する。

## 4. 設定

### 4.1 `memory.enabled`

`memory.enabled` は memory runtime の最上位ゲートであり、feature registry のロード条件でもある。

```toml
[memory]
enabled = false
```

解釈は次のとおりである。

- `false` の場合、`aibe` は basic runtime と empty feature registry を使う
- `true` の場合だけ pack の kind / recipe / feature を評価する
- `false` では `feature_files=None` でも baseline pack を読まない

### 4.2 pack の 3 状態

#### basic aibe

```toml
[memory]
enabled = false
```

この状態では memory / feature 由来の prompt 追加、registry merge、route_turn の feature_actions 補完は行わない。

#### generic memory

```toml
[memory]
enabled = true
kind_files = []
recipe_files = []
feature_files = []
```

この状態では generic memory primitive のみが有効であり、AISH 固有 kind / recipe / feature は有効化しない。

#### full AISH pack

```toml
[memory]
enabled = true
# 互換モードの既定値、または明示的な pack ファイル列挙
```

この状態では baseline pack もしくは明示 pack を使って AISH 固有 kind / recipe / feature を有効化する。

### 4.3 互換モードと明示空

`None` と `[]` は同じではない。

- `None` は互換モードを意味する
- `[]` は明示的な無効化を意味する

この差は kind / recipe / feature のいずれにも適用する。ただし 0043 では、kind / recipe の空指定が feature の baseline 読み込みを誘発しないように、pack 境界をそろえることを目標にする。

## 5. フェーズ

### 5.1 Phase 1

本 PR スコープは次である。

- `memory.enabled=false` のとき `aibe` の feature registry を empty にする
- `RoutePlan.log_tail_bytes` を top-level でも clamp する
- docs を同期する
- 追加テストを書く

Phase 1 では `kind_files` / `recipe_files` の整合や `recommended_tools` の read-only 統一は、設計上のフォローアップに留める。

### 5.2 Phase 2

フォローアップで次を実装する。

- `kind_files=[]` / `recipe_files=[]` と `feature_files=None` の組み合わせを、AISH feature が残る設定罠にしない
- `priority`、`requires_memory`、`requires_recipe` を導入して trigger の eligibility を分離する
- `RoutePlan.recommended_tools` と `FeatureAction::SetRecommendedTools` を read-only に統一する

### 5.3 Phase 3

将来、memory / feature の設定は `[features]` セクションへ分離する。

この段階では、feature pack を memory pack から独立した設定面として扱い、`kind_files` / `recipe_files` / `feature_files` の関係をより明示的にする。

## 6. `route_turn` と `ai` の契約

### 6.1 `aibe/src/application/server.rs`

composition root は `memory.enabled` を見て feature registry の初期値を決める。

- `memory.enabled=false` なら `FeatureRegistry::empty()` を渡す
- `memory.enabled=true` なら loader を使って registry を構築する

### 6.2 `aibe/src/application/route_turn.rs`

`route_turn` は prompt に feature schema を出す前に、effective registry を使って catalog を構成する。

- empty registry なら feature catalog は空
- 不整合 feature は prompt に含めない
- `log_tail_bytes` は finalization 時点で clamp する
- `recommended_tools` の read-only 化は Phase 2 の責務とし、Phase 1 では `log_tail_bytes` と empty registry に集中する

`recommended_tools` の最終的な安全化は Phase 2 で `aibe` が担い、`ai` の `sanitize_recommended_tools` は同じ境界を再確認するだけにとどめる。

### 6.3 `ai/src/main.rs`

`apply_route_plan_advisory` は `RoutePlan` をそのまま turn に反映しない。

- `log_tail_bytes` は protocol 上限を超えない値へ丸める
- `recommended_tools` は safe / read-only のみ採用する（Phase 2）
- `shell_exec` 相当の推奨は CLI 明示値か approval 経路のみで扱う

`ai` 側の sanitization は追加防御であり、`aibe` 側の正規化を前提とする。`ai` は `shell_exec` を許可する方向に拡張してはならない。

## 7. テスト

| 種別 | 内容 |
|------|------|
| Phase 1 unit | `FeatureRegistryLoader` が `feature_files=[]` で empty を返すこと、`feature_files=None` が baseline 互換であること |
| Phase 1 unit | `RoutePlan.log_tail_bytes` の clamp |
| Phase 1 integration | `memory.enabled=false` のとき route_turn が feature catalog / feature_actions を出さないこと |
| Phase 2 unit | trigger 部分一致で feature を出すが、eligibility で不整合 feature を落とすこと |
| Phase 2 unit | `RoutePlan.recommended_tools` と `FeatureAction::SetRecommendedTools` が read-only に統一されること |
| Phase 2 integration | generic memory で kind / recipe / feature が空になること |
| Phase 2 integration | full AISH pack で baseline feature が読み込まれること |

## 8. 受け入れ条件

### 8.1 Phase 1 受け入れ条件

- `memory.enabled=false` では baseline feature が読み込まれない
- `RoutePlan.log_tail_bytes` が top-level でも protocol 上限に収まる
- `./scripts/verify.sh` が通る

### 8.2 Phase 2 受け入れ条件

- `kind_files=[]` / `recipe_files=[]` / `feature_files=[]` で generic memory だけが有効になる
- `RoutePlan.recommended_tools` が `shell_exec` を通さない
- 不整合な feature は route_turn の結果に含まれない

## 9. 参照実装メモ

本仕様が直接の対象とする実装箇所は次である。

- [aibe/src/application/server.rs](../../aibe/src/application/server.rs)
- [aibe/src/adapters/outbound/filesystem_feature_registry.rs](../../aibe/src/adapters/outbound/filesystem_feature_registry.rs)
- [aibe/src/domain/feature_registry.rs](../../aibe/src/domain/feature_registry.rs)
- [ai/src/main.rs](../../ai/src/main.rs)
- [docs/spec/0042_configurable-smart-features-spec.md](0042_configurable-smart-features-spec.md)
