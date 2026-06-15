# 0038 — Contextual Memory Pack Phase D（optional crate 分離 / basic build）設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定（実装済み）  
> **起票**: 2026-06-15  
> **関連**: [0038_contextual-memory-pack-phase-a-spec.md](0038_contextual-memory-pack-phase-a-spec.md)、[0038_contextual-memory-pack-phase-b-spec.md](0038_contextual-memory-pack-phase-b-spec.md)、[0038_contextual-memory-pack-phase-c-spec.md](0038_contextual-memory-pack-phase-c-spec.md)、[0037_aibe-contextual-memory-runtime-v1-spec.md](0037_aibe-contextual-memory-runtime-v1-spec.md)、[architecture.md](../architecture.md)、[testing.md](../testing.md)

## 0. 目的

Phase D は、Phase A/B/C で整理した contextual memory の責務を **Cargo の optional crate** として分離し、`cargo build --no-default-features` で **basic build** を成立させることを目的とする。

Phase B で `aibe` の runtime boundary を pack 化し、Phase C で `ai` 側の CLI policy を pack 化した。Phase D ではその上で、memory 固有の実装群のうち **重い contextual / policy 部分** を repository-local の optional crate に逃がし、**依存の分離** と **バイナリサイズの抑制** を狙う。これらの crate は path dependency として参照し、default workspace member には含めない。

このフェーズで実現したい効果は次のとおり。

- memory 固有の実装を feature で切り離せる
- basic build では memory 固有の重い依存をリンクしない
- `aish` / `aibe` / `ai` の境界を崩さず、既存の runtime semantics を保つ

### 0.1 Phase A / Phase B / Phase C / Phase D の関係

- Phase A は `[memory] enabled` による **runtime toggle**
- Phase B は `aibe` の **server-side pack boundary**
- Phase C は `ai` の **client-side CLI policy boundary**
- Phase D はそれらを Cargo feature で包む **compile-time packaging boundary** である

Phase D は runtime の意味を変えない。`memory.enabled = false` の意味と、`--no-default-features` の意味を混同しない。

## 1. 非目標

- `dlopen` / `libloading` による動的プラグインロード
- `aibe-protocol` の wire 変更
- request / response DTO の再設計
- `aish` に memory 依存を追加すること
- Phase A の runtime toggle を廃止すること
- Phase B の pack boundary を捨てて service 直書きへ戻すこと
- Phase C の CLI policy 集約を再び `main.rs` に散らすこと

`推測`: このフェーズの本質は「機能追加」ではなく、既存の memory 実装を **feature-gated な配備単位** に再編することにある。したがって、既存の behavior を変える変更は原則避ける。

## 2. 現状と課題

### 2.1 現在の実装状況

確認できる現状は次のとおり。

- `aibe/src/application/server.rs`
  - `memory_config.enabled` に応じて `basic_pack_arc()` / `contextual_pack_arc()` を選ぶ
- `aibe/src/application/basic_memory_pack.rs`
  - `BasicPack` が `TurnHook` no-op + `RpcExtension` 拒否を実装する
- `aibe/src/application/contextual_memory_pack.rs`
  - `ContextualMemoryPack` が memory store / resolver / subscribe / recipe を束ねる
- `ai/src/application/memory_cli_pack.rs`
  - `memory_kind_list` の snapshot から `MemoryCommandPolicy` を組み立てる
- `ai/src/application/memory_cli.rs`
  - `goal` / `now` / `idea` / `mem` / `context` の memory CLI を実装する
- `ai/src/main.rs`
  - `require_memory_enabled()` を通じて memory CLI の起動前ガードを行う

### 2.2 課題

現状は runtime 境界としては正しいが、memory 固有のコードが `aibe` と `ai` の core crate に同居している。

その結果、次の問題が残る。

- memory 固有依存を compile-time で外しにくい
- basic build でも memory 由来のモジュールが core crate に残る
- `aibe` / `ai` の責務境界が runtime と compile-time で二重化している
- 将来別の optional pack を増やすときの雛形がない

## 3. 設計概要

### 3.1 optional crate による分離（当初設計）

Phase D では、memory 固有の実装を repository-local の optional crate に移すことを当初想定した。

最低限、次の 2 つを想定する。

- `aibe-plugin-memory`
  - `aibe` 側の contextual memory pack 実装を保持する
  - `ContextualMemoryPack` 相当の factory を提供する
  - memory service 群、store/resolver/broker の組み立てを内包する
- `ai-plugin-memory`
  - `ai` 側の memory CLI policy / handler を保持する
  - `MemoryCliPack` / `MemoryCommandPolicy` 相当の factory を提供する
  - `goal` / `now` / `idea` / `mem` / `context` の policy を内包する

`推測`: crate 名は最終的に多少調整されてもよいが、重要なのは「server-side memory pack」と「client-side memory CLI policy」を別 crate に出し、core crate から optional dependency として扱えるようにすることだ。

`BasicPack` は aibe core に残す。`ai` 側も feature off 時に使う fail-closed の最小 stub を core に残し、optional crate へ依存しない基本経路を維持する。

### 3.1.1 実装判断: in-crate `plugin_memory` モジュール

実装時、`aibe-plugin-memory` / `ai-plugin-memory` を path dependency として切り出すと **Cargo 循環依存**（plugin → core の公開 API 消費、core → plugin の factory 参照）が解消できなかった。

そのため Phase D の compile-time 分離は、**同一 crate 内の `plugin_memory/` モジュール** + **`memory` Cargo feature** で達成する。

- `aibe/src/plugin_memory/` — contextual pack と memory service 群（`#[cfg(feature = "memory")]`）
- `ai/src/plugin_memory/` — CLI policy / handler（同上）
- `application/` — facade と `BasicPack` / fail-closed stub（feature off でもビルド可能）

optional crate 分離は将来 `aibe-core` 等の leaf 化後に再検討可能。Phase D の受け入れ条件（basic build、`memory` feature 切替、runtime semantics 不変）は in-crate 方式でも満たす。

### 3.2 Cargo feature で有効化する

動的ロードは行わず、Cargo feature だけで切り替える。

- `aibe` は `memory` feature を持つ
- `ai` も `memory` feature を持つ
- default build では `memory` feature を有効にする
- `cargo build --no-default-features` では memory feature を外して basic build を作る

feature を外した build では、memory 固有 crate を dependency graph から外す。core 側は basic pack と fail-closed stub だけで成立させる。

### 3.3 依存方向

Phase D での依存方向は次のとおりに保つ。

- `aish` は memory crate を一切見ない
- `ai` は `aibe` を直接見ない
- `aibe` は `aish` を見ない
- `aibe-plugin-memory` と `ai-plugin-memory` は、core crate から optional dependency としてのみ参照する
- `aibe-protocol` は引き続き leaf crate として維持する

`aibe-plugin-memory` / `ai-plugin-memory` が必要とする共通型は、既存の `aibe-protocol` や小さな境界モジュールに寄せる。wire DTO を増やすのは最後の手段とし、最小限に抑える。

## 4. クレート境界

### 4.1 `aibe` と `aibe-plugin-memory`

`aibe` core は次を担当する。

- socket server の起動
- request dispatch
- route_turn / agent_turn の共通ロジック
- tool / provider / conversation store の core
- `BasicPack` 相当の no-op pack

`aibe-plugin-memory` は次を担当する。

- memory pack の具体実装
- memory service 群の束ね
- memory store / resolver / registry / recipe / subscribe の接続
- `ContextualMemoryPack` 相当の構成

`aibe` は feature 有効時のみ plugin crate を使う。無効時は core の `BasicPack` だけで起動できる。

### 4.2 `ai` と `ai-plugin-memory`

`ai` core は次を担当する。

- CLI parsing
- non-memory の turn 実行
- `aibe-client` を使った transport
- history / output / shell exec approval などの core UX
- memory feature off 時の fail-closed stub

`ai-plugin-memory` は次を担当する。

- memory CLI policy
- kind snapshot からの policy 解決
- `goal` / `now` / `idea` / `mem` / `context` の handler 実装
- `MemoryCliPack` 相当の command bundle

`ai` は feature 有効時のみ plugin crate を使う。無効時は core の fail-closed stub で明示的に拒否する。

### 4.3 `aibe-protocol` は基本的に固定

このフェーズでは wire / DTO の変更を避ける。

- `ClientRequest` / `ClientResponse`
- memory 関連 DTO
- `TurnHook` / `RpcExtension` の public 契約

は現状の意味を維持する。

## 5. feature 設計

### 5.1 default feature

default build では memory を有効にする。

理由:

- 既存の runtime behavior を壊さない
- 既存テストと手動運用を維持しやすい
- basic build は `--no-default-features` で明示できる

### 5.2 basic build

`cargo build --no-default-features` は次を満たす。

- `aibe` は memory plugin をリンクしない。`BasicPack` は core に残る
- `ai` は memory plugin をリンクしない。fail-closed stub は core に残る
- `aish` は従来どおり memory-free である
- basic build でも workspace 全体が compile できる

### 5.3 feature 名

feature 名は単純に `memory` を使うのが妥当である。

- `aibe` の `memory`
- `ai` の `memory`

`推測`: feature 名を増やしすぎると、Phase D の目的である「basic build の単純化」と逆方向に働く。少なくとも初回は `memory` 1 本に絞るのがよい。

## 6. composition root 変更

### 6.1 `aibe/src/application/server.rs`

`server.rs` は引き続き composition root だが、memory の具体実装は feature-gated にする。

変更方針:

- `memory.enabled` は runtime 設定として残す
- default build では contextual memory plugin を組み立てる
- `--no-default-features` では core の `BasicPack` だけを組み立てる
- `server.rs` 自体に memory 実装を戻さない

### 6.2 `ai/src/main.rs`

`main.rs` は CLI dispatch の薄い層に戻す。

変更方針:

- memory command の handler 実装は plugin crate に委譲する
- feature off では core の fail-closed stub を使う
- `require_memory_enabled()` のようなガードは runtime toggle 専用に整理し、build-time toggle とは分離する

### 6.3 `docs` と運用

このフェーズでは build 方式に影響するため、実装時には `docs/architecture.md` と必要な manual を同時に更新する前提とする。

## 7. Phase A / Phase B / Phase C との関係

### 7.1 Phase A

Phase A の `[memory] enabled` は **runtime の正規 toggle** として残す。

Phase D はこれを置き換えない。

### 7.2 Phase B

Phase B の `BasicPack` / `ContextualMemoryPack` は **server-side の runtime boundary** として残す。

Phase D はその pack 実装を optional crate に移すだけで、pack の意味自体は変えない。

### 7.3 Phase C

Phase C の `MemoryCliPack` / `MemoryCommandPolicy` は **client-side の policy boundary** として残す。

Phase D はその policy 実装を optional crate に移すだけで、CLI policy の source of truth を変えない。

## 8. 受け入れ条件

- `cargo build --no-default-features` が workspace で成功する
- default build では現行の memory 挙動が維持される
- `aibe` の contextual memory 実装が `plugin_memory` モジュール（feature-gated）に分離される
- `ai` の memory CLI policy が `plugin_memory` モジュール（feature-gated）に分離される
- `aish` は memory 依存を持たない
- `aibe-protocol` の wire / DTO 変更は最小限に抑えられる
- `dlopen` を使わず、Cargo feature だけで切り替えられる
- Phase A/B/C の既存テスト契約を壊さない
- 実装後に `./scripts/verify.sh` が通る

## 9. テスト方針

### 9.1 unit

- `aibe-plugin-memory` の pack factory が basic / contextual を正しく返すこと
- `ai-plugin-memory` の policy snapshot が kind metadata から正しく解決されること
- feature off 時に fallback が fail-closed になること

### 9.2 integration

- default build で既存の memory ルートが動くこと
- `cargo build --no-default-features` が通ること
- `ai goal` / `ai mem` / `ai context` の memory entry point が feature on/off で期待どおり分岐すること
- `aibe` の socket server が feature on/off で basic / contextual を切り替えられること

### 9.3 build matrix

- `cargo build`
- `cargo build --no-default-features`
- `cargo test`
- `cargo test --no-default-features`

を最低限の確認対象とする。

## 10. リスク

- optional crate 分離の途中で一時的なコード移動が増え、重複が発生しやすい
- feature matrix が増えると CI が複雑になる
- `ai` 側の memory CLI を feature off でどの程度残すかで UX がぶれる可能性がある
- shared 型を新たに切り出す際に、`aibe-protocol` を膨らませすぎる危険がある

## 11. 実装順の提案

1. `aibe-plugin-memory` を切り出し、`aibe` から memory pack 実装を外す
2. `ai-plugin-memory` を切り出し、`ai` から memory CLI policy を外す
3. `aibe` / `ai` に `memory` feature を導入する
4. `--no-default-features` の basic build を通す
5. 既存の Phase A/B/C テストを feature matrix に合わせて整理する
