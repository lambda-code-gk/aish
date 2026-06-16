# 0040 — Generic Recipe CLI / AISH Name Cleanup 実装指示書

> **種別**: 実装指示書（`docs/tasks/`）  
> **設計正本**: [0040_generic-recipe-cli-aish-name-cleanup-spec.md](../spec/0040_generic-recipe-cli-aish-name-cleanup-spec.md)  
> **状態**: 進行前  
> **起票**: 2026-06-16

## 目的

0040 の設計に従い、`ai mem run` を recipe registry に対する generic entry point に整理する。あわせて、recipe material の順序と表示名を TOML 正本へ移し、`STANDARD_KIND_GOAL/NOW/IDEA` の public export を production surface から外す。

このタスクは 0039 で残った AISH 固有の固定経路を除去し、CLI / domain / test-support / docs を同じ変更単位で揃える。

## 受け入れ条件

| 条件 | 期待結果 |
|------|----------|
| `ai mem run clarify-goal` | 従来どおり動作する |
| `ai mem run <other-recipe>` | CLI が recipe id を拒否せず、server-side registry に委譲する |
| `ai/src/main.rs` | `clarify-goal` 固定の guard が残らない |
| `ai/src/plugin_memory/memory_cli.rs` | `run_mem_recipe()` の generic 化後に `clarify-goal` 専用関数が production path に残らない |
| `aibe/src/domain/memory_recipe_registry.rs` | `RecipeTomlRoot.materials` が順序付きで読み込まれ、重複 name を error にする |
| `aibe/src/domain/memory_recipe.rs` | material title を TOML 定義から読み、コード固定の `material_title()` に依存しない |
| `aibe/src/domain/mod.rs` | `STANDARD_KIND_GOAL/NOW/IDEA` の public re-export が消える |
| `aibe/src/domain/test_support.rs` | standard kind を test-support module から参照できる |
| `ai/tests/phase_a_cli.rs` | recipe id の透過的 forwarding と既存 clarify-goal 回帰を固定できる |
| `./scripts/verify.sh` | fmt / clippy / test / architecture / docs が通る |

## 変更ファイル一覧

| 区分 | 具体的パス | 変更内容 |
|------|------------|----------|
| pack asset | `aibe/memory/packs/aish-memory/recipes/clarify-goal.toml` | `materials` を順序付き表現へ移し、各 material の `title` を追加する |
| domain | `aibe/src/domain/memory_recipe_registry.rs` | ordered materials の parse、duplicate name 拒否、title 読み込みを実装する |
| domain | `aibe/src/domain/memory_recipe.rs` | `build_recipe_messages()` を TOML 順序 + TOML title ベースへ変更する |
| domain | `aibe/src/domain/mod.rs` | `STANDARD_KIND_GOAL/NOW/IDEA` の public re-export を削除し、`test_support` module を公開する |
| domain | `aibe/src/domain/test_support.rs` | **新規**。標準 kind の test-only helper を置く |
| domain tests | `aibe/src/domain/memory_resolver_policy.rs` | unit test の import を test-support へ寄せる |
| adapter tests | `aibe/src/adapters/outbound/contextual_memory_store.rs` | unit test の import を test-support へ寄せる |
| application | `ai/src/main.rs` | `MemCommand::Run` の recipe 固定 guard を削除する |
| application | `ai/src/plugin_memory/memory_cli.rs` | `run_mem_recipe_clarify_goal()` を `run_mem_recipe()` に置き換える |
| application | `ai/src/application/memory_stub.rs` | fail-closed stub を generic シグネチャへ揃える |
| cli | `ai/src/clap_cli.rs` | `Run` サブコマンドの説明文から `clarify-goal` 固定表現を外す |
| tests | `ai/tests/phase_a_cli.rs` | arbitrary recipe id の forwarding 回帰を追加し、既存 clarify-goal 回帰を維持する |
| docs | `docs/architecture.md` | recipe entry point を generic 化した説明へ更新する |
| docs | `docs/testing.md` | 新しいテスト観点と検証コマンドを追記する |

## 実装手順

### 1. CLI の recipe 固定を外す

1. `ai/src/main.rs` の `MemCommand::Run` から `recipe != "clarify-goal"` の拒否分岐を削除する。
2. `ai/src/plugin_memory/memory_cli.rs` に `run_mem_recipe(recipe, ...)` を導入し、recipe id を引数のまま `memory_recipe_run` に渡す。
3. `ai/src/application/memory_stub.rs` も同じシグネチャへ更新し、feature off / stub 経路でも recipe 名の固定を残さない。
4. `ai/src/clap_cli.rs` の help 文言を generic 化する。
5. `ai/tests/phase_a_cli.rs` に、`clarify-goal` 以外の recipe id を `ClientRequest::MemoryRecipeRun` にそのまま渡す回帰を追加する。

### 2. recipe material を TOML 正本へ移す

1. `aibe/memory/packs/aish-memory/recipes/clarify-goal.toml` を `[[materials]]` 系の順序付き表現に更新し、各 material に `name` と `title` を明示する。
2. `aibe/src/domain/memory_recipe_registry.rs` の `RecipeTomlRoot` を ordered materials 前提に変更し、name 重複を parse error にする。
3. `aibe/src/domain/memory_recipe.rs` の `RecipeMaterials` を順序付き表現へ変更し、`build_recipe_messages()` は TOML の順序と title をそのまま使う。
4. 既存の `material_title()` のようなコード固定ロジックは削除する。
5. `aibe/src/domain/memory_recipe_registry.rs` と `aibe/src/domain/memory_recipe.rs` の unit tests で、順序・title・重複拒否を固定する。

### 3. standard kind の public export を整理する

1. `aibe/src/domain/mod.rs` から `STANDARD_KIND_GOAL/NOW/IDEA` の re-export を外す。
2. `aibe/src/domain/test_support.rs` を新設し、必要な standard kind 定数を test-only helper として置く。
3. `aibe/src/domain/memory_resolver_policy.rs` と `aibe/src/adapters/outbound/contextual_memory_store.rs` の unit test import を test-support に寄せる。
4. production code が public export 前提になっていないことを、既存テストと `cargo test` で確認する。

### 4. docs と検証を同期する

1. `docs/architecture.md` に、`ai mem run <recipe>` が registry 透過で動くことを反映する。
2. `docs/testing.md` に、generic recipe CLI の回帰観点と、ordered materials / title の unit test 観点を追記する。
3. 変更後は `./scripts/verify.sh` を通し、`cargo test -p aibe --test memory_recipe` と `cargo test -p ai --test phase_a_cli` を個別確認する。

## テスト追加・更新方針

### 単体テスト

- `aibe/src/domain/memory_recipe_registry.rs`
  - `materials` が TOML 記述順で保持されること
  - `title` が parser で読み込まれること
  - duplicate material name を拒否すること
- `aibe/src/domain/memory_recipe.rs`
  - `build_recipe_messages()` が TOML title を使うこと
  - prompt section の並びが recipe 定義順になること
- `aibe/src/domain/mod.rs`
  - public export から standard kind 定数が消えることを、import 依存が残らない形で固定する

### 統合テスト

- `ai/tests/phase_a_cli.rs`
  - `ai mem run clarify-goal` の既存回帰を維持する
  - `ai mem run <other-recipe>` の recipe id forwarding を固定する
  - `--apply` の non-interactive fail-closed を維持する

### 回帰確認コマンド

```bash
./scripts/verify.sh
cargo test -p aibe --test memory_recipe
cargo test -p ai --test phase_a_cli
```

## 実装メモ

- recipe の順序は LLM に渡す prompt の意味に直結するため、`HashMap` ベースの保持へ戻さない。
- material title は recipe TOML が正本であり、Rust 側の固定名へ戻さない。
- `clarify-goal` は特別扱いではなく、registry 上の 1 recipe として扱う。
- standard kind 定数は test-support に閉じ、production surface には出さない。
