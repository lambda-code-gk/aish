# 0033 — `ai` structured output と streaming の衝突解消 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **起票**: 2026-06-11  
> **関連**: [architecture.md](../architecture.md)、[testing.md](../testing.md)、[0027_ai-ux-spec.md](0027_ai-ux-spec.md)、[0030_ai-smart-entry-spec.md](0030_ai-smart-entry-spec.md)

## 目的

`ai` の progress spinner 導入後に露出した 2 つの P0 リスクを、`ai` クレート内の最小差分で解消する。

1. `--format json|tsv|env` で assistant streaming chunk が stdout に混入し、structured output が壊れる問題を止める
2. turn 実行中のエラーや早期 return で spinner が残留する問題を止める

本書は、`ai` の stdout 契約と stderr の progress を責務分離し、structured output と TTY UX を両立させるための設計を確定する。

## 非目標

- `aibe` のプロトコル変更
- `aibe-client` の transport 仕様変更
- `aish` の挙動変更
- 0027 で導入した TTY progress の既定 ON を弱めること
- structured output の schema 変更
- assistant streaming そのものを無効化すること

## 現状と問題

### 1. stdout 契約と streaming の責務分離が崩れている

`ai/src/main.rs` では、turn 実行の本体である `run_agent_turn_core` が `AibeUnixClient::agent_turn_request_stream` を使い、受け取った `AssistantStreaming` を `StdoutPresenter::show_stream_chunk()` がそのまま stdout に書き出す。`settings.progress` は progress event の表示制御であって、assistant streaming を stdout に流すかどうかの判定ではない。

一方で `StdoutPresenter::show_response()` は、`output_format` がある場合に structured response を最後に 1 回だけ stdout へ出す。つまり、structured output の最終出力は `show_response()` にあるが、途中の streaming chunk は `show_stream_chunk()` が stdout に出してしまう。

このため `ai --format json "hello"` のような呼び出しで、assistant chunk が JSON の前後や途中に混入し、機械可読性が壊れる。

### 2. spinner の終了が RAII ではない

`StdoutPresenter::begin_turn_progress()` は turn 開始時に spinner を始めるが、`ai/src/main.rs` では `run_agent_turn_async(...) ?` / `run_agent_turn_sync(...) ?` の早期 return を経由すると `presenter.end_turn_progress()` に到達しない。

このため、通信エラー・decode エラー・中断系エラーで spinner が残りうる。現状の `show_error()` / `show_response()` は個別に `stop_spinner_for_output()` を呼ぶが、turn スコープ全体の終了保証にはなっていない。

### 3. `--format` と progress の関係が暗黙になっている

`resolve_turn_settings()` は `progress_spinner = progress && !quiet && stderr_tty && output_format.is_none()` としており、`--format` 指定時は spinner を止めている。

しかし、structured output と stderr progress は本来別責務であり、`--format` によって stderr の progress を抑止する必要はない。今回の修正では、この関係を明示的に整理する。

## 決定事項

### 1. structured output 時は assistant streaming を stdout に出さない

- `--format json|tsv|env` のとき、assistant streaming chunk は stdout へ出力しない
- structured output の stdout は `show_response()` が最後に 1 回だけ担当する
- streaming event は受信しても、stdout への反映は無効化する
- この抑止は `run_agent_turn_core` が使う sync / async のどちらの経路にも適用する

この決定により、`ai --format json "hello"` は、streaming を伴っても final structured output だけを stdout に出す。

### 2. stderr の progress は `--format` と独立させる

- progress spinner / progress line は stderr の責務とする
- `--format` は stdout の表現だけを決める
- `--format` 指定時でも、TTY・`progress` 有効・`quiet` 無効であれば progress を表示しうる

つまり、structured output の可読性と stderr progress は干渉しない。

### 3. `ProgressGuard` で spinner を必ず停止する

- `begin_turn_progress()` と `end_turn_progress()` の呼び分けに依存せず、turn スコープを RAII 化する
- `ProgressGuard` は生成時に spinner を開始し、`Drop` で必ず停止する
- `?` による早期 return、エラー分岐、`Ctrl+C` 由来の途中終了でも spinner が残らない

`ProgressGuard` は `ai` 内部の実装補助であり、`aibe` wire には載せない。

### 4. `ai` の責務境界は維持する

- structured output 判定、stream suppression、spinner guard はすべて `ai` 側で完結する
- `aibe-client` と `aibe` は変更しない
- `aish` のログ経路・PTY 経路に手を入れない

## 仕様

### 1. stdout / stderr の出力契約

`StdoutPresenter` の役割を次のように固定する。

- `show_stream_chunk()` は assistant streaming の stdout 反映点だが、`output_format.is_some()` のときは no-op とする
- `show_response()` は structured output の唯一の stdout 出力点とする
- progress 関連の表示は stderr に限定する

これにより、`--format` を指定した turn では stdout が機械可読な最終出力だけになる。

### 2. `ProgressGuard`

`ai/src/main.rs` に turn スコープの guard を導入する。

- turn 開始時に guard を生成する
- guard の寿命中だけ spinner が有効である
- guard が drop された時点で spinner を必ず停止する

実装上は、`begin_turn_progress()` の直後に guard を束縛し、`run_agent_turn_*` の結果に関係なく scope end で drop させる。

### 3. `resolve_turn_settings()` の扱い

`resolve_turn_settings()` は progress の最終有効値を解決する責務だけを持つ。

- `quiet` は progress を抑制できる
- `TTY` による既定 ON は維持する
- `--format` は stdout 契約には影響するが、progress の有効/無効の正本にはしない

つまり、`output_format` による `progress_spinner` の暗黙抑制はやめ、progress と structured output を独立に扱う。

### 4. `show_response()` の structured 出力

`show_response()` は以下を満たす。

- `output_format.is_some()` の場合、最後に 1 回だけ structured 出力を出す
- streamed turn でも final output の JSON / TSV / ENV は壊れない
- `quiet` は stderr の補助出力を抑制しても、structured stdout には影響しない

## 受け入れ条件

### unit

- `StdoutPresenter::show_stream_chunk()` は `output_format = Some(Json|Tsv|Env)` のとき stdout を汚さない
- `StdoutPresenter::show_response()` は structured output を 1 回だけ出す
- `ProgressGuard` の drop で spinner が停止する
- turn 実行のエラー経路でも spinner 停止が保証される
- `--format` 指定時でも structured output と progress が独立に扱われる

### integration

- `ai --format json "hello"` が、streaming を伴っても parse 可能な JSON を stdout に出す
- `ai --format tsv` / `ai --format env` でも、assistant chunk が stdout に混ざらない
- TTY かつ `progress` 有効時、structured output の有無にかかわらず stderr progress は維持される
- `run_agent_turn_*` が失敗しても spinner が残らない

### docs

- `docs/architecture.md` の turn 進行表示の記述を、stdout 契約と stderr progress の分離に合わせて更新する
- `docs/spec/0027_ai-ux-spec.md` の A-6 / C-2 / C-3 に、structured output と streaming の責務分離を追記する
- `docs/0000_spec-index.md` に 0033 を追記する

## 影響範囲

### 変更対象クレート

- `ai` のみ

### 具体的な主対象

- `ai/src/adapters/outbound/stdout_presenter.rs`
- `ai/src/main.rs`

### 非対象

- `aibe`
- `aibe-client`
- `aibe-protocol`
- `aish`

## `docs/0000_spec-index.md` への追記案

```md
| 0033 | [0033_ai-structured-output-stream-fix-spec.md](spec/0033_ai-structured-output-stream-fix-spec.md) | 設計確定 | `ai` structured output と streaming の衝突解消 |
```

## 補足

本件は「streaming をやめる」話ではなく、「stdout 契約を壊す streaming を止める」話である。  
progress は stderr、structured output は stdout という分離を明示し、`ai` の最終出力面を機械可読に保つ。
