# 0038 — Contextual Memory Pack Phase D 実装指示書

> **種別**: 実装指示書（`docs/tasks/`）  
> **設計正本**: [0038_contextual-memory-pack-phase-d-spec.md](../spec/0038_contextual-memory-pack-phase-d-spec.md)  
> **状態**: 実装済み  
> **起票**: 2026-06-15

## 目的

Phase D は、Phase A/B/C で整理した contextual memory の責務を **Cargo の optional crate** として分離し、`cargo build --workspace --no-default-features` で basic build を成立させるための実装指示書である。

このフェーズでやることは機能追加ではない。`aibe` と `ai` の core crate に残っている memory 固有実装を、feature-gated な配備単位へ移し替え、default build と basic build を明確に分ける。

## 受け入れ条件

1. `aibe` と `ai` は `memory` feature を持ち、default build では有効、`--no-default-features` では無効になる
2. `cargo build --workspace --no-default-features` が通る
3. `cargo test --workspace --no-default-features` が通る
4. optional crate は repository-local の path dependency として導入されるが、default workspace member には含めない
5. `aibe` の contextual memory 実装は optional crate 側へ移る
6. `ai` の memory CLI policy / handler 実装は optional crate 側へ移る
7. default build では現行の memory 挙動が維持される
8. basic build では memory 固有依存をリンクせず、fail-closed stub だけで起動できる
9. `aish` に memory 依存を追加しない
10. `docs/architecture.md` と `docs/testing.md` を同じ変更で更新する
11. `./scripts/verify.sh` と `./scripts/smoke-mock.sh` が成功する

## 非目標

- `dlopen` / `libloading` による動的ロード
- `aibe-protocol` の wire / DTO 再設計
- `aish` への memory 依存追加
- Phase A の runtime toggle を廃止すること
- Phase B の pack boundary を戻すこと
- Phase C の CLI policy 集約を `main.rs` に散らすこと

`推測`: このフェーズで守るべき最重要点は、runtime semantics を変えずに compile-time packaging だけを分離することにある。したがって、既存の挙動を壊す変更は避け、依存方向が詰まる場合は先に最小の共有 API を切り出す。

## 実装方針

### 1. feature と workspace の境界を先に作る

`aibe` と `ai` の `memory` feature を先に定義し、default で有効・basic build で無効になるようにする。

optional crate は repository-local の path dependency とするが、workspace の default member には入れない。`Cargo.toml` の workspace 定義では、必要なら `exclude` を使って明示的に外す。

### 2. optional crate は core crate を逆参照しない

`aibe-plugin-memory` と `ai-plugin-memory` は、core crate に依存するのではなく、必要な公開 API だけを消費する。もし循環参照が出るなら、先に pack-facing の最小共有面を leaf crate に切り出してから実装する。

### 3. core crate は fail-closed stub を保持する

feature off でも `aibe` と `ai` はビルドできる必要がある。

`aibe` は `BasicPack` を core に残し、`ai` は memory CLI の最小 stub か、明示的な拒否経路を core に残す。

### 4. 既存の Phase B / Phase C テスト契約を壊さない

Phase D は packaging boundary なので、`aibe` 側の `BasicPack` / `ContextualMemoryPack` の意味と、`ai` 側の `MemoryCliPack` / `MemoryCommandPolicy` の意味は保つ。変えるのは置き場所と依存の向きだけ。

## ファイル単位の実装手順

### 1. `Cargo.toml` / 各 crate の feature wiring を先に整理する

- ルート `Cargo.toml` に optional crate 用の workspace 境界を追加する
- 必要なら `exclude = ["aibe-plugin-memory", "ai-plugin-memory"]` を入れて default workspace member から外す
- `aibe/Cargo.toml` に `features.memory` を追加し、default で有効にする
- `ai/Cargo.toml` にも同じく `features.memory` を追加し、default で有効にする
- `aibe` と `ai` はそれぞれ `optional = true` の path dependency を持つ
- feature の切替は `dep:` ベースにして、`--no-default-features` で optional crate が dependency graph に入らないことを確認する

### 2. `aibe-plugin-memory` crate を新規作成する

作成ファイルは最低限次を含める。

- `aibe-plugin-memory/Cargo.toml`
- `aibe-plugin-memory/src/lib.rs`
- `aibe-plugin-memory/src/contextual_memory_pack.rs`
- `aibe-plugin-memory/src/memory_service.rs`
- `aibe-plugin-memory/src/memory_recipe_service.rs`
- `aibe-plugin-memory/src/memory_subscribe_service.rs`

この crate の責務は次のとおり。

- contextual memory pack の組み立て
- memory store / resolver / broker / capability policy の配線
- `ContextualMemoryPack` 相当の factory の公開
- memory RPC と turn hook の bundle 生成

移行対象の実装は、現状 `aibe/src/application/contextual_memory_pack.rs` と `aibe/src/application/memory_service.rs`、`aibe/src/application/memory_recipe_service.rs`、`aibe/src/application/memory_subscribe_service.rs` にある memory 固有ロジックを基準にする。

この crate 側に unit test を置き、pack の組み立てと RPC 分岐が feature on のときだけ成立することを固定する。

### 3. `ai-plugin-memory` crate を新規作成する

作成ファイルは最低限次を含める。

- `ai-plugin-memory/Cargo.toml`
- `ai-plugin-memory/src/lib.rs`
- `ai-plugin-memory/src/memory_cli_pack.rs`
- `ai-plugin-memory/src/memory_command_policy.rs`
- `ai-plugin-memory/src/memory_cli.rs`

この crate の責務は次のとおり。

- `memory_kind_list` snapshot から command policy を組み立てる
- `goal` / `now` / `idea` / `mem` / `context` の CLI policy を束ねる
- `MemoryCliPack` 相当の facade を公開する
- `mem kinds` の表示形式と `mem add` の誘導を保持する

移行対象の実装は、現状 `ai/src/application/memory_cli_pack.rs`、`ai/src/application/memory_command_policy.rs`、`ai/src/application/memory_cli.rs` にある memory 固有ロジックを基準にする。

この crate 側で unit test を保持し、`memory_kind_list` を command 単位で 1 回だけ解釈することを固定する。

### 4. `aibe` core を feature-gated facade に整理する

更新対象は主に次のファイル。

- `aibe/src/application/mod.rs`
- `aibe/src/application/server.rs`
- `aibe/src/application/basic_memory_pack.rs`
- `aibe/src/application/contextual_memory_pack.rs`
- `aibe/src/application/request_service.rs`
- `aibe/src/application/agent_turn.rs`

手順は次の順で進める。

1. `basic_memory_pack.rs` は core の `BasicPack` と fail-closed な `RpcExtension` のまま残す
2. `contextual_memory_pack.rs` は feature on のときだけ optional crate を使う facade にする
3. `server.rs` は `memory_config.enabled` を見て `BasicPack` か contextual pack を選ぶが、feature off のときは core の basic path に倒す
4. `request_service.rs` と `agent_turn.rs` が memory 固有実装を直接持っているなら、pack / facade から受け取る形に整理する
5. `aibe` の core から optional crate を直接参照するのは `memory` feature のみとする

`server.rs` 以外に `memory.enabled` の判定を増やさない。runtime toggle の正本は従来どおり `server.rs` に置く。

### 5. `ai` core を feature-gated facade に整理する

更新対象は主に次のファイル。

- `ai/src/application/mod.rs`
- `ai/src/main.rs`
- `ai/src/application/memory_cli.rs`
- `ai/src/application/memory_cli_pack.rs`
- `ai/src/application/memory_command_policy.rs`

手順は次の順で進める。

1. `memory` feature on のときだけ optional crate を使う
2. feature off のときは fail-closed stub を core に残す
3. `main.rs` からは `goal` / `now` / `idea` / `mem` / `context` の entry だけを見せ、policy 本体は crate 外へ逃がす
4. `require_memory_enabled()` 相当の runtime gate は、build-time gate と混ぜずに 1 箇所へ集約する
5. `memory_kind_list` の snapshot は command 単位で 1 回だけ取り、その snapshot を `goal` / `now` / `idea` / `mem add` / `mem clear` / `mem kinds` に再利用する

### 6. テストを feature matrix に合わせて整理する

更新対象の既存テストは次を基準にする。

- `aibe/tests/memory_disabled.rs`
- `aibe/tests/contextual_memory.rs`
- `aibe/tests/memory_pack_turn_hook.rs`
- `aibe/tests/memory_subscribe.rs`
- `ai/tests/phase_a_cli.rs`
- `ai/tests/memory_disabled_cli.rs`

実施内容は次のとおり。

1. `aibe` の basic 回帰テストは `BasicPack` の fail-closed 挙動を固定する
2. `aibe` の contextual 回帰テストは optional crate 経由でも既存挙動が維持されることを固定する
3. `ai` の CLI テストは `MemoryCliPack` / `MemoryCommandPolicy` の移動後も `mem add`、`mem kinds`、disabled gate の契約を固定する
4. feature off のテストは、memory CLI が明示的に拒否されることと、`memory_space_id` が載らないことを確認する
5. feature on のテストは、既存の Phase A/B/C 契約が壊れていないことを確認する

必要なら optional crate 側に unit test を追加し、core 側の統合テストは薄い契約固定に寄せる。

### 7. `docs/architecture.md` と `docs/testing.md` を同時更新する

`docs/architecture.md` には次を追記する。

- Phase D が compile-time packaging boundary であること
- `memory` feature の default / no-default-features の意味
- optional crate の位置づけ
- `aibe` / `ai` の default build と basic build の違い

`docs/testing.md` には次を追記する。

- `cargo build --workspace --no-default-features`
- `cargo test --workspace --no-default-features`
- optional crate を含めた feature matrix の見方
- `verify.sh` と `smoke-mock.sh` の役割分担

## テスト方針

### 単体

- `aibe-plugin-memory` の pack factory が contextual pack を正しく組み立てること
- `ai-plugin-memory` の policy snapshot が `memory_kind_list` の metadata から正しく導かれること
- `memory_kind_list` の取得が command 単位で 1 回だけであること
- feature off 時に fail-closed stub が返ること

### 統合

- default build で既存の memory ルートが動くこと
- `cargo build --workspace --no-default-features` が通ること
- `ai goal` / `ai mem` / `ai context` が feature on/off で期待どおり分岐すること
- `aibe` の socket server が feature on/off で basic / contextual を切り替えられること

### ビルドマトリクス

最低限、次を通す。

```bash
cargo build
cargo build --workspace --no-default-features
cargo test
cargo test --workspace --no-default-features
```

### optional crate の個別テスト

workspace member から外した optional crate は、個別 manifest で直接回す。

```bash
cargo test --manifest-path aibe-plugin-memory/Cargo.toml
cargo test --manifest-path ai-plugin-memory/Cargo.toml
```

## verify / smoke 手順

### 標準検証

```bash
./scripts/verify.sh
```

### smoke

```bash
./scripts/smoke-mock.sh
```

### Phase D 追加確認

`verify.sh` は default build の品質ゲートなので、Phase D では追加で basic build の確認を必ず回す。

```bash
cargo build --workspace --no-default-features
cargo test --workspace --no-default-features
```

## 実装順の提案

1. ルート workspace と `aibe` / `ai` の `memory` feature を定義する
2. `aibe-plugin-memory` と `ai-plugin-memory` を新規作成する
3. `aibe` と `ai` の core から memory 固有実装を optional crate へ移す
4. `server.rs` と `main.rs` の feature-gated ルーティングを整理する
5. 既存テストを feature matrix に合わせて更新する
6. `docs/architecture.md` と `docs/testing.md` を更新する
7. `./scripts/verify.sh`、`./scripts/smoke-mock.sh`、`cargo build --workspace --no-default-features`、`cargo test --workspace --no-default-features` を順に通す
8. optional crate を workspace 外のまま個別 manifest でテストする

## 実装メモ（完了時）

当初設計どおり `aibe-plugin-memory` / `ai-plugin-memory` を path dependency 化したが、Cargo 循環依存のため **in-crate `plugin_memory/` + `memory` feature** に切り替えた。

- `aibe/src/plugin_memory/` — contextual pack / memory service 群
- `ai/src/plugin_memory/` — CLI policy / handler
- `application/` — facade + `BasicPack` / fail-closed stub
- memory 統合テストは `#![cfg(feature = "memory")]` で default build のみ実行

設計書 [0038 Phase D spec](../spec/0038_contextual-memory-pack-phase-d-spec.md) §3.1.1 に実装判断を追記済み。

## 実装後の確認ポイント

- default build では現行の memory 挙動が維持される
- basic build では memory 固有依存をリンクしない
- basic build では memory 固有実装（`plugin_memory`）を feature off で除外できる
- `aish` に memory 依存が入っていない
- `docs/` の仕様とテスト方針が実装に追随している
- `./scripts/verify.sh` と `./scripts/smoke-mock.sh` が成功する
