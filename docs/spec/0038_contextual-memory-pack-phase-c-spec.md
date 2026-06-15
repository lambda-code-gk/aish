# 0038 — Contextual Memory Pack Phase C（CLI / built-in kind pack 移行）設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定（Phase C 実装済み）  
> **起票**: 2026-06-15  
> **関連**: [0038_contextual-memory-pack-phase-a-spec.md](0038_contextual-memory-pack-phase-a-spec.md)、[0038_contextual-memory-pack-phase-b-spec.md](0038_contextual-memory-pack-phase-b-spec.md)、[0037_aibe-contextual-memory-runtime-v1-spec.md](0037_aibe-contextual-memory-runtime-v1-spec.md)、[architecture.md](../architecture.md)、[testing.md](../testing.md)

## 0. 目的

Phase C は、`ai` 側に残っている contextual memory の **CLI policy / built-in kind knowledge** を pack 境界に寄せ、`goal` / `now` / `idea` / `mem` / `context` の判定を 1 か所に集約する。

Phase A で `memory.enabled` による basic 切替を導入し、Phase B で `aibe` 側の `TurnHook` / `RpcExtension` を pack 化した。Phase C ではその上にある **client 側の memory command policy** を整理し、`ai/src/application/memory_cli.rs` と `ai/src/main.rs` に散った kind ハードコードを減らす。

ここで言う pack は、Phase B の server-side pack を置き換えるものではない。Phase C は **`ai` クライアントの command policy pack** を導入し、server が返す kind metadata を source of truth として使う。

### 0.1 Phase A / Phase B / Phase C の関係

- Phase A は `[memory] enabled` の runtime toggle である
- Phase B は `aibe` 内の pack 境界である
- Phase C は `ai` 側の CLI policy と built-in kind affordance の集約である

### 0.2 この設計で解く問題

- `ai` が `goal` / `now` / `idea` を個別関数と文字列分岐で扱っている
- `ai mem add goal` の拒否理由がコードに直書きされている
- `main.rs` が memory command ごとに `memory_enabled` を再確認している
- `kinds.toml` の server-side 正本と、client 側の固定知識が二重管理になりやすい

## 1. 非目標

- 動的プラグインロード
- `memory` クレート分離
- wire / DTO 変更
- `aibe` 側 pack 境界の再設計
- `aibe/src/domain/memory_kind_registry.rs` の built-in baseline をこのフェーズで TOML のみに置き換えること
- client が `kinds.toml` を直接読むこと

`推測`: built-in kind 定義の実体は当面 `aibe` 側に残し、`ai` は `memory_kind_list` の metadata を参照して UX policy を組み立てるのが最も安全である。

## 2. 現状と課題

### 2.1 server 側の built-in kind は正しいが、client に重複がある

`aibe/src/domain/memory_kind_registry.rs` には built-in 6 kind がある。

- `goal`
- `now`
- `rule`
- `decision`
- `idea`
- `note`

ここには `default_scope` / `default_inject` / `default_status` / `dedicated_cli` / `priority` / `prompt` がまとまっている。

一方で `ai` 側は次を重複保持している。

- `ai/src/application/memory_cli.rs`
  - `run_goal_set` / `run_goal_show` / `run_goal_clear`
  - `run_now_set` / `run_now_show` / `run_now_clear`
  - `run_idea_add` / `run_idea_list` / `run_idea_clear`
  - `clear_scope_for_kind`
  - `query_active` の `idea` 特例
  - `standard_kind_mem_add_hint`
- `ai/src/main.rs`
  - `run_goal` / `run_now` / `run_idea` / `run_mem` / `run_context`
  - `prepare_memory_context` と `run_context` の `memory_enabled` ガード
- `ai/src/adapters/outbound/toml_config.rs`
  - `memory_enabled` 読み込みと env override

### 2.2 `kinds.toml` 方針との整合

`docs/spec/0037_aibe-contextual-memory-runtime-v1-spec.md` では、registry の正本を次の順で merge すると定義している。

1. built-in definitions
2. server config definitions
3. memory-space-local definitions

この方針に照らすと、client は built-in kind の挙動を自前で再実装せず、server が返す `MemoryKindDefinitionDto` を UX policy の材料として扱うべきである。

`推測`: Phase C で client に必要なのは「kind 名の知識」ではなく「kind metadata の読み方」であり、`dedicated_cli` / `default_scope` / `default_inject` / `default_status` を使って CLI の分岐を作ることで十分である。

### 2.3 Phase B の pack 境界はそのまま使う

Phase B で `aibe` 側はすでに `BasicPack` / `ContextualMemoryPack` を持っている。

- `BasicPack`: no-op 注入 + memory RPC 拒否
- `ContextualMemoryPack`: 注入 + memory RPC 実装

Phase C はこれを壊さない。むしろ `ai` 側の CLI policy を pack 化して、server pack への依存を明確化する。

## 3. 設計概要

### 3.1 client 側に memory command policy pack を置く

`ai` 側には、memory コマンドの振る舞いを集約する薄い pack / policy 層を導入する。

`推測`: 実装名は `MemoryCliPack` か `MemoryCommandPack` が自然である。重要なのは名前ではなく、`main.rs` と `memory_cli.rs` が kind 名・専用 CLI・disabled 判定を直接持たないことである。

この policy 層が担当するのは次の通り。

- `goal` / `now` / `idea` の専用 CLI の振る舞い
- `mem add <kind>` の redirect / hint
- `mem kinds` の表示整形
- `context current` / `context use` の disabled 判定
- `memory_enabled` が false のときの fail-closed

### 3.2 kind affordance の source of truth を server metadata に寄せる

`ai` は built-in kind を「固定の 3 個」として扱わない。

代わりに、server が返す kind metadata を使って次を決める。

- `dedicated_cli` がある kind は専用 CLI を持つ
- `default_scope` / `default_inject` / `default_status` は dedicated CLI の既定値
- `builtin` は標準 kind として表示するかの判断材料になる
- `priority` は `mem kinds` 表示や hint 順序に使える

これにより、`goal` / `now` / `idea` が専用 CLI であることは維持しつつ、client 側の分岐は kind 名のハードコードではなく metadata 駆動になる。

この policy 層は、`memory_kind_list` の応答から得た `MemoryKindDefinitionDto` 群を 1 回だけ解釈し、その snapshot を `goal` / `now` / `idea` / `mem add <kind>` / `mem clear <kind>` / `mem kinds` の判断に使う。
`now` の session scope や `idea` の open status のような既存の特殊分岐は、kind 名の文字列分岐ではなくこの snapshot の `default_scope` / `default_status` から導く。

### 3.3 `memory_enabled` は設定入力のまま、判定は 1 箇所へ寄せる

`ai/src/adapters/outbound/toml_config.rs` の `memory_enabled` は残す。

ただし Phase C では次を目指す。

- `memory_enabled` の読み込みは config/env の責務として残す
- `main.rs` での per-command 判定はやめる
- memory command 入口で 1 回だけ enabled/basic を選ぶ

`推測`: 現実的には、`prepare_memory_context` と `run_context` を共通 helper に寄せ、その helper が memory policy pack を構築する形が最小の変更である。

### 3.4 `ai` 側の pack は server pack の相棒である

Phase B の `aibe` pack は runtime そのものを切り替える。

Phase C の `ai` pack は、runtime のメタデータを使って CLI の UX を切り替える。

つまり:

- server pack は execution boundary
- client pack は command-policy boundary

であり、責務は重ならない。

## 4. 具体的な移行対象

### 4.1 `ai/src/application/memory_cli.rs`

このファイルからは、次のような kind 固有知識を減らす。

- `goal` / `now` / `idea` の分岐
- `idea` だけ `Open` を見る特例
- `now` だけ session scope を使う特例
- `goal` / `now` / `idea` を文字列で列挙する hint

残してよいものは、kind metadata の表示・整形・汎用 RPC 呼び出しである。
`mem clear <kind>` の scope 決定と `query_active` の status 決定も、ここではなく 3.2 の snapshot から得る。

### 4.2 `ai/src/main.rs`

`main.rs` は command dispatch の薄い層にする。

今ある次の責務を、pack / policy helper に寄せる。

- `run_goal` / `run_now` / `run_idea` の分岐
- `run_mem` の専用 kind hint
- `prepare_memory_context` と `run_context` の enabled 判定の重複

CLI の表面は維持する。`ai goal` / `ai now` / `ai idea` / `ai mem` / `ai context` を消す必要はない。

### 4.3 `ai/src/adapters/outbound/toml_config.rs`

ここは config parse のまま維持する。

ただし Phase C では、ここに新しい kind policy を足さない。`memory_enabled` は runtime switch であって、kind affordance の source of truth ではない。

### 4.4 `aibe/src/domain/memory_kind_registry.rs`

このファイルの built-in baseline は server の正本として残す。

Phase C では、`ai` に重複している知識を減らすだけであり、server 側の baseline を移設しない。

`推測`: 将来 built-in baseline の完全データ駆動化をする場合でも、このフェーズではやらない方がよい。client/server の両面を同時に動かすと、`kinds.toml` の互換性検証が難しくなる。

## 5. 受け入れ条件

- `ai` 側の CLI policy が `goal` / `now` / `idea` の kind 名ハードコードを保持しない
- `ai mem add goal` は専用 CLI への誘導を返すが、その判定は registry metadata 由来である
- `ai mem add rule` / `decision` / `note` は generic `mem add` のまま使える
- `ai mem kinds` は server から返った `MemoryKindDefinitionDto` の配列を入力にした表示であり、`Json` では DTO をそのまま出力し、`Tsv` / `Env` は表示用 projection に限る
- `memory_enabled` の gate は 1 か所に集約され、`main.rs` / `memory_cli.rs` に散らばらない
- Phase B の `aibe` pack 行為を壊さない
- `docs/spec/0037_aibe-contextual-memory-runtime-v1-spec.md` の kinds.toml 仕様と矛盾しない
- 追加実装後に `./scripts/verify.sh` が通る

## 6. テスト方針

### 6.1 unit

- registry metadata から dedicated CLI hint を引くロジック
- standard kind 判定が kind 名の固定列挙ではなく metadata 由来であること
- `now` の session scope / `idea` の open status など、kind metadata から導いた分岐
- `mem clear <kind>` が registry snapshot の `default_scope` を使うこと
- `mem kinds` の `Json` 出力が `MemoryKindDefinitionDto` を損なわないこと

### 6.2 integration

- `ai goal set` が従来どおり `goal` の専用経路を使うこと
- `ai mem add goal` が専用 CLI への誘導を返すこと
- `ai mem add rule` / `decision` / `note` が generic 経路で動くこと
- `ai mem kinds` が server registry を表示すること
- memory disabled 時に `goal` / `context` / `ask` の挙動が fail-closed であること

### 6.3 docs consistency

- `docs/0000_spec-index.md` に Phase C を追加する
- Phase C 設計が Phase A/B の関係を誤らせない

## 7. リスク

- client が registry metadata をキャッシュしすぎると、`kinds.toml` 更新を即時反映できない
- client が server の registry ルールを local で再実装すると、`0037` の merge 仕様とズレる
- `memory_enabled` の入口が複数残ると、Phase A の重複問題が復活する

`推測`: metadata のキャッシュは process lifetime のみ、または command 1 回ごとに再取得する方が安全である。性能上のボトルネックにはなりにくい。
