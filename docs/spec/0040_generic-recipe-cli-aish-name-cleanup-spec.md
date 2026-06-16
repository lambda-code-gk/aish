# 0040 — Generic Recipe CLI / AISH Name Cleanup 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **起票**: 2026-06-16  
> **関連**: [0039_aish-memory-pack-externalization-spec.md](0039_aish-memory-pack-externalization-spec.md)、[0039_aish-memory-pack-externalization-implementation-spec.md](../done/0039_aish-memory-pack-externalization-implementation-spec.md)、[0037_aibe-contextual-memory-runtime-v1-spec.md](0037_aibe-contextual-memory-runtime-v1-spec.md)、[architecture.md](../architecture.md)、[testing.md](../testing.md)

## 0. 目的

0039 の実装レビューで残った AISH 固有の固定経路を取り除き、recipe 実行を一般化する。

本仕様の目的は次の 5 点である。

- `ai mem run <recipe>` から `clarify-goal` 固定の分岐をなくす
- `run_mem_recipe_clarify_goal()` を一般化して `run_mem_recipe()` にする
- `RecipeTomlRoot.materials` を順序付きにし、TOML 上の宣言順を保持する
- material の表示名をコード固定から recipe TOML に移す
- `STANDARD_KIND_GOAL/NOW/IDEA` の production export をやめ、テスト用に隔離する

0039 で recipe registry と pack 外部化の骨格は成立したが、CLI 層・material 表示・テスト用定数の公開面に、まだ AISH 固有の名前と固定スイッチが残っている。ここを整理して、recipe 実行系を generic に保つ。

## 1. 非目標

- 新しい recipe schema の追加
- `MemoryRecipeRun` wire protocol の変更
- `clarify-goal` の output schema 変更
- recipe の apply セマンティクス変更
- memory kind の意味変更
- 0039 で定めた pack / config の解決ルール変更
- `goal` / `now` / `idea` の kind 挙動変更
- `docs/0000_spec-index.md` の更新

## 2. 現状ギャップ

### 2.1 CLI 層に残る `clarify-goal` 固定

現行の `ai/src/main.rs` は `MemCommand::Run` で recipe 名を受け取っていても、`clarify-goal` 以外を即座に拒否している。さらに `ai/src/plugin_memory/memory_cli.rs` は `run_mem_recipe_clarify_goal()` を専用関数として持ち、内部で recipe id を固定している。

この形は 0039 の「recipe registry 化」に対して、CLI 層だけが古い特殊扱いを保持している状態である。

### 2.2 `RecipeTomlRoot.materials` が順序を失う

`aibe/src/domain/memory_recipe_registry.rs` の `RecipeTomlRoot.materials` は `HashMap<String, MaterialTomlEntry>` であり、TOML の記述順を保持できない。`build_recipe_messages()` は `recipe.materials` の順に section を組み立てるため、材料定義の順序が実質的な prompt 仕様になるが、その順序が parser で壊れている。

### 2.3 material title がコード固定

現在の `material_title()` は `open_query` / `goal_query` / `now_query` / `rule_query` / `decision_query` を Rust 側で `Open ideas` などに変換している。

このため、recipe の見た目と順序が TOML で完結せず、レイアウトの正本がコード側に残っている。

### 2.4 standard kind 定数の公開面が広い

`STANDARD_KIND_GOAL/NOW/IDEA` は `aibe::domain` から再 export されており、production surface に AISH 固有の語彙が漏れている。

推測だが、レビューの意図は「kind の意味を消す」ことではなく、「テストや内部実装で便利だからといって public export に残さない」ことにある。したがって、定数は内部実装か test-support に閉じる。

## 3. 設計概要

### 3.1 recipe の正本は recipe TOML

recipe の正本は TOML であり、TOML が以下を一貫して持つ。

- recipe id
- description
- llm profile
- prompt 本文への参照
- material の順序
- material の表示 title
- query contract
- output contract

Rust 側は TOML を読み、順序付きの material 定義として保持するだけにする。

### 3.2 CLI は recipe id を透過的に扱う

`ai mem run <recipe>` は recipe 名を検査せず、入力された recipe id をそのまま `memory_recipe_run` に渡す。

recipe の存在確認は server-side registry に委ねる。CLI が `clarify-goal` だけを知る必要はない。

### 3.3 standard kind 定数は内部化する

`goal` / `now` / `idea` の文字列は contextual memory の実装詳細としては残してよいが、`aibe::domain` の public export には出さない。

必要なら内部モジュールか test-support module から参照し、production コードは registry / helper 経由で判定する。

## 4. 変更方針

### 4.1 `ai mem run <recipe>` の固定チェックを削除する

`ai/src/main.rs` の `MemCommand::Run` 分岐から、`recipe != "clarify-goal"` のガードを削除する。

その代わりに、CLI は以下だけを責務とする。

- recipe 名を引数から受け取る
- `run_mem_recipe()` に recipe 名を渡す
- apply 時の confirmation だけを維持する

`clarify-goal` は単なる registry entry の 1 つとして扱う。

### 4.2 `run_mem_recipe_clarify_goal()` を `run_mem_recipe()` に置き換える

`ai/src/plugin_memory/memory_cli.rs` に一般化された `run_mem_recipe()` を導入し、recipe id を引数で受け取る。

この関数は、現行の `clarify-goal` 専用処理と同じく次を行う。

- `memory_recipe_run` を呼ぶ
- `summary` と `proposals` を表示する
- `--apply` の場合は確認後に proposal を順次適用する

違いは recipe 名が固定でないことだけである。`memory_stub.rs` 側の fail-closed stub も同じシグネチャに揃える。

### 4.3 `RecipeTomlRoot.materials` を順序付き配列にする

`aibe/src/domain/memory_recipe_registry.rs` の TOML root は、`materials: HashMap<...>` ではなく、順序付き配列として解釈する。

設計上の要件は次のとおり。

- TOML の記述順をそのまま recipe の prompt 順にする
- material 名は明示フィールドとして保持する
- name 重複は parse error にする
- 順序の意味は prompt 生成と material 収集の両方で一致させる

`RecipeMaterials` も順序付き表現を持つ。lookup だけのために map を本体にしない。

### 4.4 material title を TOML に移す

material ごとの title は TOML に `title` として書く。

`build_recipe_messages()` は Rust 側の `material_title()` を参照せず、recipe 定義に入っている title をそのまま使う。title は prompt の読みやすさを決める metadata であり、コード固定にする必要がない。

これにより、prompt の見た目や section 名の変更がコード修正なしで行える。

### 4.5 `STANDARD_KIND_*` を production export から外す

`aibe/src/domain/mod.rs` の public re-export から `STANDARD_KIND_GOAL/NOW/IDEA` を外す。

隔離先は `domain::test_support` に統一する。ここに standard kind の test helper を置き、定数は test からのみ参照する。

`aibe::domain` の public API から AISH 固有の定数名が消えることが必須であり、production code は raw constant を前提にせず、registry / helper function を使う。
既存の kind 判定処理は内部化された定数か既存 registry API に置き換える。

## 5. 受け入れ条件

- `ai mem run clarify-goal` 以外の recipe id でも CLI が起動する
- `ai/src/main.rs` に `clarify-goal` 固定の recipe guard が残らない
- `run_mem_recipe_clarify_goal()` が production path からなくなり、`run_mem_recipe()` に集約される
- `RecipeTomlRoot.materials` が順序付きで読み込まれる
- material の title が TOML で指定され、Rust 側の `material_title()` に依存しない
- TOML の材料順が、収集順と prompt 生成順の両方に反映される
- `STANDARD_KIND_GOAL/NOW/IDEA` が `aibe::domain` の public export に残らない
- テストは新しい helper から必要な定数を参照できる
- `domain::test_support` から standard kind 定数を参照できる
- `./scripts/verify.sh` が通る

## 6. テスト方針

### 6.1 単体テスト

- `aibe/src/domain/memory_recipe_registry.rs`
  - materials の順序が TOML 記述順で保持されること
  - material title が parser で読み込まれること
  - duplicate material name を拒否すること
- `aibe/src/domain/memory_recipe.rs`
  - `build_recipe_messages()` が TOML 上の title と順序を使うこと
  - `RecipeMaterials` の並びが prompt に反映されること
- `aibe/src/domain/mod.rs` 周辺
  - production export から standard kind 定数が消えていることを compile で固定する

### 6.2 統合テスト

- `ai/tests/phase_a_cli.rs`
  - `ai mem run clarify-goal` の既存回帰を維持する
  - CLI が recipe 名を透過的に渡すことを固定する
- `aibe/tests/memory_recipe.rs`
  - recipe 実行が `clarify-goal` 固定に依存しないことを固定する

### 6.3 回帰確認

- `cargo test --workspace`
- `./scripts/verify.sh`
- `docs/architecture.md` と `docs/testing.md` の該当節が実装に同期される

## 7. 実装上の注意

- recipe の順序は見た目だけでなく、LLM に渡すコンテキストの意味を持つため、`HashMap` ベースの保持に戻さない
- title をコードに戻すと 0040 の目的を失うため、title は TOML 正本に固定する
- standard kind 定数の export を残すと、AISH 固有 API が再び production surface に出る
- `clarify-goal` の特例を CLI に残すと、将来 recipe が増えたときに再び分岐が増殖する

## 8. 期待される完了状態

この仕様が実装されると、`ai mem run` は recipe registry に対する generic entry point になり、`clarify-goal` は AISH pack に入った 1 recipe として扱われる。

また、material の順序と title は TOML の責務になり、`goal` / `now` / `idea` の定数は production export から消える。これで 0039 の外部化は「機能的に動く」だけでなく、「API 面でも AISH 固有名に引っ張られない」状態になる。
