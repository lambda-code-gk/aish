# 0038 — Contextual Memory Pack Phase C 実装指示書

> **種別**: 実装指示書（`docs/done/`）  
> **設計正本**: [0038_contextual-memory-pack-phase-c-spec.md](../spec/0038_contextual-memory-pack-phase-c-spec.md)  
> **状態**: 実装済み（Phase C）  
> **起票**: 2026-06-15

## 目的

`ai` 側に残っている contextual memory の **CLI policy / built-in kind 知識** を pack 境界へ寄せる。

Phase C では `goal` / `now` / `idea` / `mem` / `context` の判定を 1 か所に集約し、`ai/src/application/memory_cli.rs` と `ai/src/main.rs` に散った kind ハードコードと disabled 判定を除去する。

## 受け入れ条件

1. `ai` 側の CLI policy が `goal` / `now` / `idea` の kind 名ハードコードを保持しない
2. `ai mem add goal` は専用 CLI への誘導を返すが、その判定は `memory_kind_list` の metadata 由来である
3. `ai mem add rule` / `decision` / `note` は generic `mem add` のまま使える
4. `ai mem kinds` は `MemoryKindDefinitionDto` の snapshot を入力にした表示であり、`Json` は DTO をそのまま、`Tsv` / `Env` は表示用 projection のみを返す
5. `memory_enabled` の gate は 1 か所に集約され、`main.rs` / `memory_cli.rs` に散らばらない
6. `Phase B` の `aibe` 側 pack 境界を壊さない
7. wire / DTO 変更を行わない
8. `docs/architecture.md` と `docs/testing.md` を同じ変更で更新する
9. `./scripts/verify.sh` と `./scripts/smoke-mock.sh` が成功する

## 非目標

- `aibe` 側の pack 境界の再設計
- wire / DTO / protocol の変更
- `aibe` の built-in kind baseline を TOML のみに置き換えること
- client が `kinds.toml` を直接読むこと
- dynamic plugin load
- `memory` クレート分離

`推測`: Phase C の安全な実装は、client が「kind 名の固定列挙」を持たず、`memory_kind_list` の metadata を command policy に変換して使う構成である。  
この前提に反する追加の client-side kind 定義は入れない。

## 実装方針

### `MemoryCliPack`

`ai` 側に、memory CLI の入口をまとめる薄い pack / facade を導入する。

- 役割: `MemoryClient` + `MemoryCliContext` + `MemoryCommandPolicy` を束ねる
- 責務: 各 CLI サブコマンドへのディスパッチ
- 禁止: kind 名のハードコード、`memory_enabled` の散在チェック、`aibe` wire 変更

### `MemoryCommandPolicy`

`memory_kind_list` の結果を 1 回だけ取得して、command policy に変換する。

- 取得タイミング: `run_memory_command` の入口で 1 回だけ
- 利用範囲: その command invocation のみ
- キャッシュ範囲: プロセス共有はしない
- 再取得: 同一 command 内では再取得しない

`推測`: policy の snapshot は command 単位の short-lived オブジェクトに留めるのが最も安全で、kind 更新の即時反映と実装簡潔性の両立ができる。

### metadata 由来の判定

`MemoryCommandPolicy` は `MemoryKindDefinitionDto` から次を導く。

- 専用 CLI を持つ kind の判定
- `mem add <kind>` の専用 CLI 誘導
- `mem clear <kind>` の scope 決定
- `query_active` の status 決定
- `mem kinds` の表示順と projection

判定の正本は server が返す metadata であり、`goal` / `now` / `idea` の固定文字列は policy の入力値としてしか扱わない。

## ファイル単位の実装手順

### 1. `ai/src/application/memory_command_policy.rs` を新規作成

- `MemoryCommandPolicy` と snapshot 用の補助型を定義する
- `MemoryKindDefinitionDto` の配列を受け取り、専用 CLI 有無・既定 scope/status・表示順を解釈する
- `dedicated_cli` を source of truth にして、固定 kind 名の分岐を持たない
- `mem add` 用の誘導文、`clear` の scope、`query_active` の status 決定をここに寄せる
- `memory_kind_list` の応答を 1 回だけ解釈する前提で実装する

### 2. `ai/src/application/memory_cli.rs` を refactor する

- `goal` / `now` / `idea` の個別処理を、`MemoryCommandPolicy` を受け取る薄い wrapper に置き換える
- `standard_kind_mem_add_hint`、`clear_scope_for_kind`、`query_active` の kind 固有分岐を削除する
- `mem add goal` の拒否文言は、固定文字列ではなく metadata 由来の dedicated CLI hint を使う
- `mem kinds` の `Json` は DTO をそのまま返し、`Tsv` / `Env` のみ projection にする
- `memory_kind_list` の取得はこのファイル内で再実行しない
- unit test は `MemoryCommandPolicy` / formatter / wrapper の境界ごとに分ける

### 3. `ai/src/main.rs` を refactor する

- `goal` / `now` / `idea` / `mem` / `context` の entry から、`MemoryCliPack` か同等の facade を呼ぶ形へ整理する
- `prepare_memory_context` と `run_context` に散った `cfg.ensure_memory_enabled()` を、共通 helper に寄せて 1 か所化する
- `memory_enabled` の判定は command entry の共通 helper だけに残し、サブコマンドごとの重複判定を削除する
- `context current/use/new` の disabled 判定も共通 helper 経由に統一する
- `run_memory_command` は、context 構築後に policy snapshot を受け取って dispatch するだけにする
- `memory_kind_list` snapshot は command 単位で 1 回だけ取り、その snapshot を `goal` / `now` / `idea` / `mem add` / `mem clear` / `mem kinds` に再利用する

### 4. `ai/tests/phase_a_cli.rs` を更新する

- `mem add goal` のテストは、固定 kind 名のハードコードではなく `dedicated_cli` あり snapshot を使う形に変更する
- `mem kinds` のテストは、server が返す `MemoryKindDefinitionDto` の snapshot そのものを検証する
- `Json` 出力は DTO roundtrip を壊さないことを確認する
- `Tsv` / `Env` は表示用 projection に限定されることを確認する
- `mem add custom` / `mem run clarify-goal` / `--apply` 非対話 fail-closed の既存回帰は維持する

### 5. `ai/tests/memory_disabled_cli.rs` を更新する

- disabled gate が `main.rs` の共通 helper に集約されても、`goal set` / `context current` / `ask` の既存回帰が維持されるようにする
- `AI_MEMORY_ENABLED` の env override が従来どおり効くことを維持する
- `memory_space_id` を送らない経路は既存のまま固定する

### 6. `docs/architecture.md` と `docs/testing.md` を更新する

- `docs/architecture.md`
  - contextual memory の Phase C を client-side pack boundary として追記する
  - `MemoryCliPack` / `MemoryCommandPolicy` の位置づけを追記する
  - `memory_kind_list` snapshot を command 単位で取得する方針を追記する
  - `aibe` 側 pack 境界はそのまま、wire 変更なしであることを明記する
- `docs/testing.md`
  - `ai` の memory policy / pack boundary を固定するテスト位置を追記する
  - 追加する個別 `cargo test` コマンドを追記する
  - `verify.sh` / `smoke-mock.sh` の役割分担を維持する

### 7. `aibe` 側は最小変更に留める

- `aibe` の runtime / pack / wire / DTO は原則変更しない
- Phase C で必要なのは `ai` 側の policy 変更であり、server-side の bundle を増やさない
- もしコンパイル都合で最小限の修正が必要になっても、wire 契約と pack の責務は変えない

## Step 6: 検証コマンド

以下を順に実行する。

```bash
./scripts/verify.sh
./scripts/smoke-mock.sh
```

追加の `cargo test` は以下を実行する。

```bash
cargo test -p ai --test phase_a_cli
cargo test -p ai --test memory_disabled_cli
cargo test -p ai memory_cli
```

## 実装後の確認ポイント

- `ai mem add goal` が `MemoryCommandPolicy` 経由で専用 CLI に誘導される
- `ai mem add rule` / `decision` / `note` は generic 経路のまま動く
- `ai mem kinds` の JSON が DTO そのものを壊さない
- `goal` / `now` / `idea` / `context` の disabled gate が 1 箇所にまとまる
- `aibe` の pack 境界や wire 契約に変更がない

## 受け入れ条件チェックリスト

- [ ] `MemoryCliPack` を導入し、CLI entry の責務を薄くした
- [ ] `MemoryCommandPolicy` が `memory_kind_list` snapshot を 1 回だけ解釈する
- [ ] `goal` / `now` / `idea` の kind 名ハードコードを削除した
- [ ] `mem add goal` が metadata 由来の専用 CLI hint を返す
- [ ] `mem add rule` / `decision` / `note` は generic 経路のまま
- [ ] `mem kinds` の `Json` は DTO をそのまま返す
- [ ] `memory_enabled` の gate を 1 か所へ集約した
- [ ] `ai/tests/phase_a_cli.rs` を更新した
- [ ] `ai/tests/memory_disabled_cli.rs` を更新した
- [ ] `docs/architecture.md` を更新した
- [ ] `docs/testing.md` を更新した
- [ ] `./scripts/verify.sh` が通る
- [ ] `./scripts/smoke-mock.sh` が通る
