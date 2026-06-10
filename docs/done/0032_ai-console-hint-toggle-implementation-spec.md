# 0032 — `ai` コンソールヒント切り替え 実装指示書

> **種別**: 実装指示書（`docs/done/`）  
> **設計の正本**: [0032_ai-console-hint-toggle-spec.md](../spec/0032_ai-console-hint-toggle-spec.md)  
> **状態**: 実装済み

## 変更ファイル一覧

- `ai/src/clap_cli.rs` - `TurnOptions` に `--console-hint` / `--no-console-hint` / `-H` を追加する
- `ai/src/adapters/outbound/toml_config.rs` - `[ask].console_hints` と `[presets.*].console_hints` を読む
- `ai/src/domain/console_hint.rs` - 新規。`resolve_console_hints` と結果型を置く
- `ai/src/domain/mod.rs` - 新しい console hint ドメイン型を re-export する
- `ai/src/domain/ask.rs` - `AskRequest` / `AskInput` が pre-resolved の context を運べるように調整する
- `ai/src/application/ask.rs` - application 経路でも同じ pre-resolved context を受け渡す
- `ai/src/main.rs` - `ResolvedTurnSettings` への組み込み、request 組み立て、dry-run 反映
- `ai/src/domain/reports.rs` - dry-run 出力に console hint の解決結果を追加する
- `ai/src/adapters/outbound/aibe_client.rs` - terminal size の direct detect を除去し、受け取った context をそのまま serialise する
- `ai/tests/phase_a_cli.rs` - CLI / dry-run の非 TTY 系検証を追加する
- `ai/tests/ux_gap_closure.rs` - chat / retry / rerun の dry-run 互換性を追加する
- `ai/tests/ask_integration.rs` - request context の受け渡し確認を追加する
- `ai/tests/console_hint_tty.rs` - 新規。TTY 正常系の統合テストを置く
- `docs/architecture.md` - `ai` の責務と console hint の解決位置を更新する
- `docs/ai.config.example.toml` - `[ask]` / `[presets.*]` の `console_hints` 例を追加する
- `docs/0000_spec-index.md` - 進行中タスクとして 0032 を載せるなら同時更新する

## 実装順

1. `clap` のフラグ定義を固定する。`TurnOptions` に `--console-hint` / `-H` を enable、`--no-console-hint` を disable として追加し、同一 turn 内で排他的に扱う。
2. config / preset を拡張する。`AiConfig` と `AiPresetConfig` に `console_hints: Option<bool>` を追加し、`docs/ai.config.example.toml` にも例を入れる。
3. `resolve_console_hints` を新設する。CLI > preset > config > hardcoded default `true` の順で `requested` を決め、TTY と `output_format` を加味して `effective` と `suppressed_by` を決める。
4. `ResolvedTurnSettings` に console hint の解決結果を組み込む。`resolve_turn_settings` で 1 回だけ計算し、`ask` / `chat` / `retry` / `rerun` の request 組み立てと dry-run の両方に渡す。
5. request 生成を一本化する。`request_from_messages` と `AibeUnixClient::to_client_request` が同じ resolved policy を使うようにし、`aibe_client.rs` 自身は terminal state を見ない。
6. `aibe_client.rs` から direct detect を除去する。`detect_terminal_size()` をここで呼ばない。必要なら `AskRequest` 側に pre-resolved の `RequestContextInput` か同等の値を持たせ、transport は serialize のみ担当させる。
7. dry-run 出力を更新する。`console_hint` の結果を表示し、実際の system instruction 本文は出さない。
8. unit / integration テストを追加し、`./scripts/verify.sh` まで通す。

## `clap` の定義方法

`TurnOptions` には次の 3 つを追加する。

- `--console-hint`
- `--no-console-hint`
- `-H`

実装方針は次の通り。

- `-H` は `--console-hint` の short form にする
- `--no-console-hint` は long only にする
- `--console-hint` と `--no-console-hint` は同時指定を拒否する
- `quiet` など既存フラグと同じく turn 単位のオプションとして扱う

`clap` の field は `TurnOptions` に持たせ、resolve 時に `Option<bool>` 相当へ正規化する。

## `resolve_console_hints` ドメイン関数

新規ドメイン関数は、CLI / preset / config / default の優先順位と、TTY / format による抑止を 1 箇所に閉じ込める。

期待する入力は少なくとも次の 5 点。

- CLI 明示値
- preset 値
- config 値
- TTY かどうか
- output format

期待する出力は次の 6 点。

- `requested`
- `source`
- `tty`
- `output_format`
- `effective`
- `suppressed_by`

解決ルールは次の通り。

- `requested` は CLI > preset > config > default `true`
- `source` は `cli` / `preset` / `config` / `default`
- `effective` は `requested && tty && output_format.is_none()`
- `suppressed_by` は 1 つだけ返す。優先順位は `tty` > `format` > `none`
- `output_format` は `json` / `tsv` / `env` / `none` を使う

この関数は純粋関数として書く。terminal size の取得や `std::env` 参照はここへ入れない。

## `ResolvedTurnSettings` への組み込み

`resolve_turn_settings` の中で console hint を 1 回だけ解決し、`ResolvedTurnSettings` に保持する。

保持の目的は 3 つ。

- request 組み立てで同じ policy を再利用する
- dry-run で同じ結果を表示する
- `aibe_client` 側で再判定しない

`request_from_messages` は `ResolvedTurnSettings` から pre-resolved の値を受け取り、`RequestContextInput.system_instruction` を組み立てる。`aibe_client.rs` は同じ値をそのまま wire 化するだけにする。

## `aibe_client.rs` の direct detect 除去方針

`aibe_client.rs` からは次を削る。

- `detect_terminal_size()` の直接呼び出し
- `--format` や TTY の判定ロジック
- console hint を有効化するかどうかの policy 判断

残す責務は次だけにする。

- `AskRequest` あるいは同等の入力に入っている `RequestContextInput` を `ClientRequest::AgentTurn` に写像する
- transport 送受信を行う

もし `AskRequest` に pre-resolved context を持たせる形にするなら、その context は `main.rs` 側で組み立ててから渡す。`aibe_client.rs` が新しい policy を持たないことを最優先にする。

## dry-run 出力フィールド名

`DryRunReport` には `console_hint` のネストを追加し、少なくとも次のフィールドを出す。

- `console_hint.requested`
- `console_hint.source`
- `console_hint.tty`
- `console_hint.output_format`
- `console_hint.effective`
- `console_hint.suppressed_by`

値の意味は次の通り。

- `console_hint.requested` は policy 解決後の on/off
- `console_hint.source` は `cli` / `preset` / `config` / `default`
- `console_hint.tty` は `true` / `false`
- `console_hint.output_format` は `json` / `tsv` / `env` / `none`
- `console_hint.effective` は最終的に system instruction を付けたかどうか
- `console_hint.suppressed_by` は `tty` / `format` / `none`

`effective=false` のときは、理由を 1 つだけ出す。本文の system instruction は dry-run に含めない。

## 単体・統合テスト一覧

### 単体

- `ai/src/domain/console_hint.rs`
  - CLI > preset > config > default の優先順位
  - `tty=false` で `effective=false`
  - `output_format=Some(_)` で `effective=false`
  - `suppressed_by` が 1 つだけ返ること
- `ai/src/clap_cli.rs`
  - `-H` が `--console-hint` として解析されること
  - `--no-console-hint` が long only で解析されること
  - 両者の排他が効くこと
- `ai/src/domain/reports.rs`
  - dry-run の JSON / TSV / ENV に `console_hint.*` が出ること
  - `effective=false` でも system instruction 本文を出さないこと
- `ai/src/main.rs`
  - `ResolvedTurnSettings` に console hint の resolved 値が入ること
  - `request_from_messages` が resolved 値を使うこと
- `ai/src/adapters/outbound/aibe_client.rs`
  - 受け取った context をそのまま wire 化すること
  - terminal size を見ないこと

### 統合

- `ai/tests/phase_a_cli.rs`
  - `ai ask --console-hint --dry-run --format json` で `console_hint.*` が表示されること
  - `ai ask --no-console-hint --dry-run` で `requested=false` になること
  - `--format` 指定時は `effective=false` になること
- `ai/tests/ux_gap_closure.rs`
  - `chat` / `retry` / `rerun` でも同じ toggle が効くこと
  - dry-run に `console_hint.*` が出ること
- `ai/tests/ask_integration.rs`
  - `Ask` 経路でも pre-resolved context が transport まで落ちること
  - `aibe_client` 側が console hint policy を再判定しないこと
- `ai/tests/console_hint_tty.rs`
  - TTY では `--console-hint` が system instruction を送ること
  - TTY でも `--no-console-hint` が送らないこと

## 受け入れ条件（DoD）

- `--console-hint` / `--no-console-hint` / `-H` が `ask` / `chat` / `retry` / `rerun` で使える
- `[ask].console_hints` と `[presets.*].console_hints` が読める
- `resolve_console_hints` が CLI > preset > config > default の順で決まる
- `TTY` と `--format` による抑止が `effective` と `suppressed_by` に反映される
- `ResolvedTurnSettings` に console hint の resolved 値が入る
- `request_from_messages` と `AibeUnixClient` が同じ resolved policy を使う
- `aibe_client.rs` に terminal size の direct detect が残らない
- dry-run に `console_hint.*` が出る
- 既存の非対象 subcommand `status` / `doctor` / `ping` / `history` / `complete` には影響しない
- 単体・統合テストが追加され、`./scripts/verify.sh` が成功する

## docs 更新

- `docs/architecture.md` - `ai` の責務、console hint の解決位置、`aibe_client` が policy を持たないことを明記する
- `docs/ai.config.example.toml` - `[ask].console_hints` と `[presets.*].console_hints` の例を追加する
- `docs/0000_spec-index.md` - 進行中タスク一覧に 0032 を載せるなら同時に更新する

## 補足

この機能は `console hint` の on/off を追加するだけで、文言そのものは再設計しない。`aibe` の wire schema は増やさず、`ai` の turn 解決だけで完結させる。
