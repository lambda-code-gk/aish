# 0048 — `ai` output filter と assistant streaming の整合化 実装指示書

> **種別**: 実装指示書（`docs/done/`）  
> **設計の正本**: [0048_ai-filter-streaming-fix-spec.md](../spec/0048_ai-filter-streaming-fix-spec.md)  
> **状態**: 実装済み  
> **起票**: 2026-06-22  
> **関連**: [architecture.md](../architecture.md)、[testing.md](../testing.md)、[ai-ask-tools.md](../manual/ai-ask-tools.md)、[`scripts/spec-acceptance.toml`](../../scripts/spec-acceptance.toml)、[`docs/0000_spec-index.md`](../0000_spec-index.md)

## 0. 目的

`AI_FILTER` / `[ask].filter` が assistant streaming 導入後に効かなくなっている問題を、`ai` クレート内の stdout 契約修正で解消する。

本指示書では、filter 有効時に assistant streaming chunk を stdout に出さず、turn 終了時の本文に 1 回だけ filter を適用する。あわせて `streamed` の意味を「実際に stdout へ chunk を出したか」に戻し、`progress` や `timeout` を stdout 契約に混ぜない。

## 1. パック構成の適用

**No**。今回の変更は optional 機能の脱着ではなく、`ai` の presenter と turn 終了処理にある stdout 契約の修正である。Active Pack / Basic Pack を導入する話ではないため、pack boundary は不要。

## 2. Phase 分割

| Phase | 内容 | 完了条件 |
|-------|------|----------|
| 1 | `StdoutPresenter` と `main.rs` の表示契約を修正し、filter 有効時の streaming 抑止と final filter 出力を一致させる。unit / integration / docs / spec-acceptance を同一変更で揃える。 | `scripts/spec-acceptance.toml` の 0048 ケースがすべて `pending = false` になり、関連テストが通る |

## 3. 変更ファイル一覧

| パス | 役割 |
|------|------|
| `ai/src/adapters/outbound/stdout_presenter.rs` | `assistant_stream_stdout_enabled()` に filter 条件を追加する。`show_response()` の `streamed` / `filter` 分岐を修正する。`ensure_stdout_newline` の呼び出し条件を見直す。unit テストを追加する。 |
| `ai/src/main.rs` | `let streamed = streamed || settings.progress || settings.timeout_secs.is_some();` を削除し、`streamed` を「chunk を stdout に出した事実」に限定する。filter 有効時の final 表示経路と整合させる。 |
| `ai/tests/phase_a_cli.rs` | streaming 付き `ai ask` の integration 回帰を追加する。mock socket server で `AssistantStreaming` → `AgentTurnResult` を返し、filter 有効時に chunk が stdout に漏れないことを確認する。 |
| `scripts/spec-acceptance.toml` | 0048 の AC を `pending = false` で登録する。unit / integration のテスト関数名と 1:1 で結ぶ。 |
| `docs/architecture.md` | `assistant_streaming` / filter / progress / `streamed` の責務分離を追記する。 |
| `docs/manual/ai-ask-tools.md` | output filter の手動手順に streaming 付きケースを追加し、filter 有効時に chunk が stdout に出ないことを明記する。 |
| `docs/testing.md` | 0048 の unit / integration 配置と検証観点を追記する。 |
| `docs/0000_spec-index.md` | `docs/tasks/` の 0048 行を追加する。 |

## 4. 実装手順

### 4.1 `StdoutPresenter` の契約修正

1. `assistant_stream_stdout_enabled()` を `!self.quiet && self.output_format.is_none() && self.output_filter.is_none()` に変更する。
2. `show_stream_chunk()` は、この判定が false のときに no-op のままにする。
3. `show_response()` では、`streamed=true` でも `output_filter` があるなら final assistant message を必ず出力する。
4. `show_response()` の `streamed` 分岐は「streaming chunk を stdout に実際に出したか」で判定し、filter 有効時に final 出力を飛ばさない。
5. `ensure_stdout_newline` は、stdout に streaming chunk を実際に出した場合にのみ必要とする。filter 有効時は追加改行を入れない。

### 4.2 `main.rs` の streamed 集約を整理

1. `run_agent_turn_*` の戻り値から得た `streamed` は、その turn で chunk を stdout に出したかの事実だけを保持する。
2. `let streamed = streamed || settings.progress || settings.timeout_secs.is_some();` は削除する。
3. `progress` は stderr spinner の制御に閉じ込める。
4. `timeout_secs` は cancel / timeout 通知の制御に閉じ込める。
5. `show_response()` に渡す `streamed` が、filter 有効時の final 出力抑止に使われないことを確認する。

### 4.3 unit テストを追加する

1. `ai/src/adapters/outbound/stdout_presenter.rs` の `#[cfg(test)]` に、filter あり / なしで `assistant_stream_stdout_enabled()` が期待どおりに切り替わるテストを追加する。
2. `show_response()` の `streamed=true` + filter 有効ケースを検証するテストを追加する。
3. `ensure_stdout_newline` の条件が、filter 有効時に余計な改行を入れないことを確認するテストを追加する。

### 4.4 integration テストを追加する

1. `ai/tests/phase_a_cli.rs` に streaming 付きの mock socket server ケースを追加する。
2. `AI_FILTER` もしくは `[ask].filter` を設定し、`AssistantStreaming` の chunk が stdout に混ざらないことを確認する。
3. `AgentTurnResult` の final 本文は filter 後の値だけが stdout に残ることを確認する。
4. `ensure_stdout_newline` の修正で、filter 有効時に余計な空行が増えないことも確認する。

## 5. テスト追加

### 5.1 unit

- `ai/src/adapters/outbound/stdout_presenter.rs`
  - `assistant_stream_stdout_enabled()` が `output_filter` 有効時に false になる
  - `show_response()` が `streamed=true` でも filter 有効時に final 本文を出す
  - `ensure_stdout_newline` が filter 有効時に余計な改行を入れない

### 5.2 integration

- `ai/tests/phase_a_cli.rs`
  - streaming 付き `ask` で filter 有効時に chunk が stdout に出ない
  - final assistant 本文だけが stdout に残る
  - streaming 抑止時に余計な空行が出ない

## 6. `scripts/spec-acceptance.toml` 登録案

`spec = "0048"` を 4 件追加し、すべて `pending = false` にする。

| Phase | id | description | test | file_glob | pending |
|------|----|-------------|------|-----------|---------|
| 1 | `stream_stdout_gate` | filter 有効時は assistant streaming を stdout に出さない | `assistant_stream_stdout_enabled_returns_false_when_filter_is_set` | `ai/src/adapters/outbound/stdout_presenter.rs` | false |
| 1 | `show_response_streamed_filter` | streamed turn でも filter 後の final 本文を出す | `show_response_emits_filtered_stdout_even_when_streamed` | `ai/src/adapters/outbound/stdout_presenter.rs` | false |
| 1 | `newline_gate` | filter 有効時は `ensure_stdout_newline` を追加しない | `streamed_filter_does_not_force_extra_newline` | `ai/src/adapters/outbound/stdout_presenter.rs` | false |
| 1 | `streaming_cli_filter` | CLI の streaming turn で chunk が stdout に漏れない | `ask_with_filter_hides_streaming_chunks` | `ai/tests/phase_a_cli.rs` | false |

## 7. `docs/` 更新方針

### 7.1 `docs/architecture.md`

- `assistant_streaming` の stdout 契約に filter 有効時の抑止を追記する。
- `streamed` を「chunk を stdout に出した事実」に限定する。
- `progress` と `timeout` は stdout ではなく stderr / cancel 側の責務であることを明記する。

### 7.2 `docs/manual/ai-ask-tools.md`

- output filter の節に、streaming 付きの確認手順を追加する。
- filter 有効時は chunk が stdout に出ないことを明記する。
- 手動確認時に、final stdout と stderr warning の見え方を分けて確認できるようにする。

### 7.3 `docs/testing.md`

- 0048 を `ai` の stdout 契約修正として追記する。
- unit は `stdout_presenter.rs`、integration は `phase_a_cli.rs` を正本にする。
- `streamed` / `progress` / `timeout` の責務分離を検証観点に追加する。

## 8. 受け入れ条件チェックリスト

- [x] `assistant_stream_stdout_enabled()` が filter 有効時に false になる
- [x] filter 有効時に assistant streaming chunk が stdout に出ない
- [x] `show_response()` は `streamed=true` でも final 本文へ filter を 1 回だけ適用する
- [x] `let streamed = streamed || settings.progress || settings.timeout_secs.is_some();` が削除される
- [x] `ensure_stdout_newline` は filter 有効時に余計な改行を入れない
- [x] `ai/tests/phase_a_cli.rs` で streaming 付き filter 回帰が追加される
- [x] `scripts/spec-acceptance.toml` の 0048 ケースがすべて `pending = false` になる
- [x] `docs/architecture.md`、`docs/manual/ai-ask-tools.md`、`docs/testing.md` が同一変更で更新される
- [x] `docs/0000_spec-index.md` の tasks に 0048 が追加される
- [x] `./scripts/verify.sh` が通る

