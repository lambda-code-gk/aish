# 0039 — AISH Memory Pack Externalization 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **起票**: 2026-06-16  
> **関連**: [0037_aibe-contextual-memory-runtime-v1-spec.md](0037_aibe-contextual-memory-runtime-v1-spec.md)、[0038_contextual-memory-pack-phase-a-spec.md](0038_contextual-memory-pack-phase-a-spec.md)、[0038_contextual-memory-pack-phase-b-spec.md](0038_contextual-memory-pack-phase-b-spec.md)、[0038_contextual-memory-pack-phase-c-spec.md](0038_contextual-memory-pack-phase-c-spec.md)、[0038_contextual-memory-pack-phase-d-spec.md](0038_contextual-memory-pack-phase-d-spec.md)、[architecture.md](../architecture.md)、[testing.md](../testing.md)

## 0. 目的

0038 で `aibe` の basic runtime / pack boundary は成立したが、`aibe` core にはまだ contextual memory の **AISH 固有知識** が残っている。

本仕様の目的は、`aibe` core を **generic memory primitive のみ** に縮退させ、AISH contextual memory を pack/config から読み込む構造に移すことである。具体的には、以下を実現する。

- built-in memory kind を Rust ハードコードから TOML 外部定義へ移す
- `FilesystemMemoryKindRegistryLoader` を `kind_files` 起点にする
- `clarify-goal` recipe を TOML + markdown に外出しする
- `ai` の `resolve_turn_memory_space_id` を `#[cfg(feature = "memory")]` で分岐する

この仕様で扱うのは **外部化の正本化** であり、wire protocol の再設計や dynamic plugin loader は対象外である。

## 1. 非目標

- 動的プラグインロード
- ワイヤー/DTO の変更
- `aish` に memory 依存を追加すること
- `MemoryContext` の意味変更
- `memory_subscribe` の transport 契約変更
- `aibe-protocol` への AISH pack 固有フィールド追加
- `docs/0000_spec-index.md` の更新

## 2. 現状ギャップ

### 2.1 `aibe` core に残っているハードコード

以下の実装は、AISH 固有の contextual memory 知識を Rust コードに保持している。

- [aibe/src/domain/memory_kind_registry.rs](../../aibe/src/domain/memory_kind_registry.rs)
  - `MemoryKindRegistry::builtin()`
  - `goal` / `now` / `rule` / `decision` / `idea` / `note` の built-in 定義
  - built-in を起点にした registry merge
- [aibe/src/adapters/outbound/filesystem_memory_kind_registry.rs](../../aibe/src/adapters/outbound/filesystem_memory_kind_registry.rs)
  - `<AIBE_ROOT>/memory/kinds.toml`
  - `<AIBE_ROOT>/memory/spaces/<space>/kinds.toml`
  - fixed path ベースの merge
- [aibe/src/domain/memory_recipe.rs](../../aibe/src/domain/memory_recipe.rs)
  - `RECIPE_CLARIFY_GOAL`
  - clarify-goal 用 material collection
  - recipe prompt の system/user 文面
  - JSON 出力 schema の固定
- [aibe/src/plugin_memory/memory_recipe_service.rs](../../aibe/src/plugin_memory/memory_recipe_service.rs)
  - `clarify-goal` だけを受ける分岐
  - recipe 実行の LLM プロンプトがコード固定

### 2.2 `ai` 側の残存結合

[ai/src/main.rs](../../ai/src/main.rs) は turn 用 `memory_space_id` を解決するが、feature 条件が緩く、memory 非有効 build でも同じ経路が見えている。

本仕様では、`resolve_turn_memory_space_id` を `#[cfg(feature = "memory")]` で分岐し、feature off 時は memory 空間解決を **コンパイル時に消す**。

### 2.3 0038 との関係

0038 は runtime toggle と pack boundary を確立した。

0039 はその上で、pack の **内容そのもの** を外部化する。したがって、0038 の `BasicPack` / `ContextualMemoryPack` という概念は維持しつつ、中身の kind/recipe 定義を TOML と markdown に移す。

## 3. 設計概要

### 3.1 方針

`aibe` core は、kind や recipe の具体値を知らない。知るのは次の 3 つだけである。

- registry を読み込むための primitive
- recipe を読み込むための primitive
- memory runtime の有効/無効と pack 組み立ての境界

具体の AISH contextual memory は、pack ディレクトリまたは config から与えられる。

### 3.2 pack の考え方

本仕様での pack は「AISH contextual memory を構成する一式」である。最低限、以下を含む。

- kind 定義ファイル群
- recipe 定義ファイル群
- recipe 本文 markdown

`enabled=true` でも `kind_files=[]` かつ `recipe_files=[]` なら、`aibe` は generic memory primitive だけを提供し、AISH 固有 kind / recipe は有効化しない。

## 4. 設定スキーマ

### 4.1 `aibe` 設定

`~/.config/aibe/config.toml` の `[memory]` に、既存の `enabled` に加えて外部ファイル一覧を追加する。

```toml
[memory]
enabled = true
kind_files = [
  "memory/packs/aish-memory/kinds.toml",
]
recipe_files = [
  "memory/packs/aish-memory/recipes/clarify-goal.toml",
]
```

#### 解釈

- `enabled = false` は 0038 どおり basic runtime
- `kind_files` は registry へ merge する TOML ファイルの順序付きリスト
- `recipe_files` は recipe registry へ merge する TOML ファイルの順序付きリスト
- `kind_files = []` は kind pack を無効化する
- `recipe_files = []` は recipe pack を無効化する
- `kind_files = []` かつ `recipe_files = []` のとき、AISH pack は完全に無効化される
- `kind_files` と `recipe_files` は独立に解釈し、未指定は互換モード、空配列は無効化、非空配列は明示 pack とする

#### 互換性

- `kind_files` / `recipe_files` を **明示しない** 場合は、移行期間の互換モードとして従来の AISH memory 配置を読む
- この互換モードでは、配布済みの baseline pack を先に読み、その後で既存の `memory/kinds.toml` と `memory/spaces/<space>/kinds.toml` の override を重ねる
- `kind_files = []` / `recipe_files = []` を **明示** した場合だけ、互換モードも含めて AISH pack を無効化する
- 明示された相対パスは `config.toml` の所在ディレクトリ基準で解決し、正規化後の絶対パスを使う

#### パス解決

`推測`: 最も安全なのは、相対パスを **config ファイルの所在ディレクトリ** 基準で解決し、server 起動後に絶対パスへ正規化する方式である。`cwd` 依存にすると、`aibe` 起動ディレクトリによって pack 解決が揺れるためである。

### 4.2 `ai` 設定

`ai` 側の `[memory] enabled` は 0038 どおり runtime toggle として残す。

本仕様の変更点は、`ai` が turn 用 `memory_space_id` を feature off build で解決しないことだけである。

## 5. TOML スキーマ

### 5.1 kind TOML

kind 定義は `kinds.toml` に外出しする。1 ファイルに複数 kind を置けるようにする。

```toml
[kinds.goal]
description = "User goal"
default_scope = "project"
default_inject = "pinned"
default_status = "active"
lifecycle = "active_inactive"
cardinality = "single_effective"
clear_from = "active"
clear_to = "inactive"
stale = "session_changed"
dedicated_cli = "goal"
aliases = ["goals"]

[kinds.goal.prompt]
auto_inject = true
on_demand = true
priority = 10
keywords = ["goal", "goals", "ゴール"]
max_entries = 1
```

#### 必須/任意

- `description` は必須
- `default_scope` / `default_inject` / `default_status` / `lifecycle` / `cardinality` / `clear_from` / `clear_to` / `stale` は kind の契約として解釈する
- `prompt.auto_inject` / `prompt.on_demand` / `prompt.priority` / `prompt.keywords` / `prompt.max_entries` は on-demand / auto-inject の判定情報
- `dedicated_cli` は AISH CLI の専用入口を表す
- `aliases` は kind 名の別名
- `builtin` は TOML では表現せず、ロード元が built-in pack かどうかで導出する

#### 完全スキーマ

`kinds.toml` は full definition と overlay fragment の両方を表現できるよう、以下のフィールドを持つ。

- `description`
- `default_scope`
- `default_inject`
- `default_status`
- `lifecycle`
- `cardinality`
- `clear_from`
- `clear_to`
- `stale`
- `dedicated_cli`
- `aliases`
- `prompt.auto_inject`
- `prompt.on_demand`
- `prompt.priority`
- `prompt.keywords`
- `prompt.max_entries`

### 5.2 recipe TOML

recipe 定義は TOML を正とし、markdown ファイルを本文として参照する。

```toml
id = "clarify-goal"
description = "Turn open ideas into a clarified goal set"
llm_profile = "default"
prompt_md = "clarify-goal.md"

[materials]
open_query = { kind = "idea", scope = "project", status = "open" }
goal_query = { kind = "goal", scope = "project", status = "active", limit = 1 }
now_query = { kind = "now", scope = "session", status = "active", limit = 1 }
rule_query = { kind = "rule", scope = "project", status = "active" }
decision_query = { kind = "decision", scope = "project", status = "active" }

[output]
format = "json"
summary_required = true
allow_operations = ["add"]
```

#### recipe md

`prompt_md` が指す markdown は、LLM に渡す実文面を保持する。

最低限、以下を含む。

- 役割説明
- 参照してよい memory の種類
- 禁止事項
- 出力形式
- `summary` と `proposals` の意味

`TOML` は machine-readable contract、`md` は human-readable prompt body とする。

#### 解釈

- `prompt_md` は recipe TOML ファイルの所在ディレクトリ基準で解決する
- `recipe_files` の読み込み順は merge 順であり、同一 `id` は後勝ちとする
- `materials` は既存 `MemoryQueryDto` と 1:1 に対応し、query contract として検証可能でなければならない
- `allow_operations` は LLM 出力の operation 制約であり、recipe ごとに明示する
- `llm_profile` は既存の profile registry を経由して解決する

## 6. ローダー変更

### 6.1 `MemoryKindRegistry`

`aibe/src/domain/memory_kind_registry.rs` の API は次の方向へ変える。

- `MemoryKindRegistry::builtin()` を廃止
- `MemoryKindRegistry::empty()` を追加
- `load_from_toml(...)` を追加
- `merge(...)` を追加

最終形では、registry の作成は次の流れになる。

1. `MemoryKindRegistry::empty()`
2. 1 つ以上の TOML ファイルを `load_from_toml`
3. `merge`

これにより、built-in を Rust 側に固定しない。

### 6.2 `FilesystemMemoryKindRegistryLoader`

[aibe/src/adapters/outbound/filesystem_memory_kind_registry.rs](../../aibe/src/adapters/outbound/filesystem_memory_kind_registry.rs) は、`aibe_root/memory/kinds.toml` 固定ではなく、`MemoryConfig.kind_files` を起点に読み込む。

挙動は以下とする。

- `kind_files` が **明示されている** 場合は、その順に読み込む
- `kind_files` が **明示されていない** 場合は、互換モードとして配布済みの baseline pack を先に読み、その後で従来の `memory/kinds.toml` と `memory/spaces/<space>/kinds.toml` を読み込む
- 各ファイルは `MemoryKindRegistry::load_from_toml` で解釈する
- 先頭の registry は `MemoryKindRegistry::empty()` とし、以降は `merge` する
- 破損 TOML は `load_strict` で error、`load_best_effort` では warn してそのファイルだけ skip する
- すべての kind ファイルが失敗した場合は empty registry に退避する
- `load_best_effort` の互換モードは、AISH 固有 kind が壊れても turn を止めないことを優先する

`推測`: `load_best_effort` は AISH kind の一部欠落を許すより、**ファイル単位での失敗を明確にする** ほうが運用上安全である。少なくとも strict path は失敗を隠さない方がよい。

### 6.3 pack root

pack の root は `kind_files` / `recipe_files` の相対パス解決に使うだけで、built-in 固定パスの代わりにはしない。

## 7. recipe registry

### 7.1 役割

clarify-goal を含む recipe は、`MemoryRecipeRegistry` のような registry で管理する。

registry は次の責務を持つ。

- recipe ID から TOML メタデータを引く
- markdown 本文を読む
- LLM profile を recipe ごとに解決する
- material query を recipe ごとに持つ
- duplicate recipe id を deterministic に解決する

`MemoryRecipeRegistry` は kind registry と同様に、次の API を持つ。

- `MemoryRecipeRegistry::empty()`
- `MemoryRecipeRegistry::load_from_toml(...)`
- `merge(...)`

ロード順は kind registry と同じく順序付きで、`recipe_files` の後勝ち merge を採る。`load_strict` は任意の recipe ファイル失敗を error にし、`load_best_effort` は失敗したファイルだけを skip する。`prompt_md` が解決できない recipe は strict では error、best-effort では skip とする。

### 7.2 clarify-goal の外部化

現状の `RECIPE_CLARIFY_GOAL` と `build_clarify_goal_messages` は、3 つに分解する。

- TOML の recipe metadata
- markdown の prompt body
- material collection helper

`MemoryRecipeService` は、recipe ID を見て registry から定義を引き、そこに書かれた query/contract を使う。

### 7.3 互換性の扱い

移行途中は、`clarify-goal` だけが最初の外部化対象である。

ただし最終形では、`aibe` core に recipe 名や `goal` / `now` / `idea` / `rule` / `decision` の固定文字列を残さない。

`recipe_files` を **明示しない** 場合は、既存の `clarify-goal` レシピ互換を維持する。`recipe_files = []` を明示したときだけ、recipe 外部化も含めて pack を無効化する。

## 8. ai の feature 分岐

### 8.1 `resolve_turn_memory_space_id`

[ai/src/main.rs](../../ai/src/main.rs) の turn 用 `memory_space_id` 解決は、feature gate を切る。

#### memory feature 有効

- 既存どおり `AIBE_CONTEXT_ID`
- `cfg` の current context
- project key
- legacy session
  の順で解決する

#### memory feature 無効

- `resolve_turn_memory_space_id` は `None` を返すか、そもそもコンパイル対象から外す
- `build_request_context` は `memory_space_id` を載せない
- `ai` は memory なし build で、context 解決を保持しない

### 8.2 目的

この分岐により、`cargo build -p ai --no-default-features` 相当の経路で、memory client side の依存が残らないようにする。

## 9. 移行パス

### 9.1 kind registry

1. `MemoryKindRegistry::empty()` と `load_from_toml()` を追加する
2. `FilesystemMemoryKindRegistryLoader` を `kind_files` ベースに変える
3. built-in registry 参照を削除する
4. `goal` / `now` / `rule` / `decision` / `idea` / `note` を pack TOML へ移す

### 9.2 recipe registry

1. `clarify-goal` の TOML metadata を追加する
2. markdown 本文を切り出す
3. `MemoryRecipeService` を registry lookup ベースに変える
4. `RECIPE_CLARIFY_GOAL` と固定 prompt 文面を削除する

### 9.3 ai feature gate

1. `resolve_turn_memory_space_id` を `#[cfg(feature = "memory")]` で囲む
2. memory feature off の stub を定義する
3. `build_request_context` の `memory_space_id` を feature dependent にする

## 10. テスト方針

### 10.1 unit

- `MemoryKindRegistry::empty/load_from_toml/merge`
- `kind_files` の読み込み順序
- duplicate kind id の扱い
- recipe TOML から markdown 参照が解決できること
- clarify-goal の metadata が registry 経由で取得できること
- `stale` / `prompt.max_entries` を含む kind schema の roundtrip
- `prompt_md` の相対パス解決
- `recipe_files` の duplicate id が deterministic になること
- `kind_files` / `recipe_files` 未指定時の互換モード

### 10.2 integration

- `AIBE_MEMORY_ENABLED=0` で basic runtime
- `cargo build -p aibe --no-default-features` で pack なし build
- `kind_files` / `recipe_files` 未指定で従来の kinds.toml override と clarify-goal 互換が維持されること
- `enabled=true, kind_files=[], recipe_files=[]` で RPC は生きるが AISH kind / recipe は出ない
- `enabled=true + aish-memory pack` で AISH contextual memory が有効
- `ai` の feature off build で `resolve_turn_memory_space_id` 経路が消えること

### 10.3 docs consistency

- [architecture.md](../architecture.md) の pack / registry / recipe 記述を更新する
- [testing.md](../testing.md) に pack 外部化のテスト位置を追記する

## 11. 受け入れ条件

| 条件 | 期待結果 |
|------|----------|
| `AIBE_MEMORY_ENABLED=0` | basic runtime として起動する |
| `cargo build -p aibe --no-default-features` | pack なしでビルドできる |
| `kind_files` / `recipe_files` 未指定 | 既存 kinds.toml override と clarify-goal 互換経路を維持する |
| `enabled=true, kind_files=[], recipe_files=[]` | RPC は可、AISH kind / recipe は有効化されない |
| `enabled=true + aish-memory pack` | AISH contextual memory が有効になる |
| `MemoryKindRegistry::builtin()` 廃止 | core に built-in 定義を残さない |
| `FilesystemMemoryKindRegistryLoader` | `kind_files` 起点で動く |
| `clarify-goal` 外部化 | TOML + md から読まれる |
| `ai` feature gate | `resolve_turn_memory_space_id` が feature 分岐される |

## 12. 未確定事項

- `kind_files` / `recipe_files` の相対パス基準を config ファイル基準にするか、aibe root 基準にするか
- recipe markdown のテンプレート変数をどこまで許すか
- `MemoryRecipeRegistry` の duplicate recipe id を hard error にするか last-wins にするか
- AISH pack のディレクトリ命名を `aish-memory` に固定するか、pack 名を config から参照するか

## 13. 補足

本仕様は 0038 の runtime pack boundary を前提にしているため、0038 の責務分離は維持する。そのうえで、AISH 固有の kind/recipe を Rust 固定値から外し、pack と config に閉じ込めるのがゴールである。
