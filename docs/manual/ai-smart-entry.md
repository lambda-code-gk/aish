# ai スマート入口 手動検証

`ai '...'` の smart entry（`route_turn` / `AI_SESSION_ID` / conversation 継続）の確認手順。

## 前提

```bash
cargo build -p aibe -p ai -p aish
export PATH="$PWD/target/debug:$PATH"
```

`aibe` 設定に `[router] profile = "fast"` があること（例: `docs/aibe.config.example.toml`）。

## 1. 標準フロー（`ai '...'`）

1. `aish shell` を起動し、`echo $AI_SESSION_ID` で session ID が export されていることを確認する。
2. 同一 shell 内で `ai 'hello'` を実行する。
3. 2 回目以降の `ai '...'` で会話が継続されること（stderr に継続通知が出る場合あり）。
4. `~/.local/share/aibe/conversations/<AI_SESSION_ID>/` に store が作られること。

## 2. 新規会話（`ai --new '...'`）

1. 上記 shell で `ai --new 'fresh start'` を実行する。
2. stderr に新規 conversation への切替が出ること。
3. 以降の `ai '...'`（`--new` なし）は新 conversation 内で継続すること。

## 3. 複数 tab での session 共有

1. 同一 `aish shell` セッションから 2 つの tab を開く（`AI_SESSION_ID` が同じ）。
2. tab A で `ai 'first tab'`、tab B で `ai 'second tab'` を実行する。
3. 両方が同一 conversation store を参照し、文脈が共有されること。

## 4. non-TTY fallback

```bash
echo hello | ai
ai 'hello' </dev/null
```

- `route_turn` を呼ばず従来の 1 shot ask になること。
- `AI_SESSION_ID` は request context に載ること（integration: `ai/tests/phase_a_cli.rs`）。

## 5. CLI 明示値の優先

```bash
ai --preset fast --tools read_file 'list files'
```

- `route_turn` は実行されるが、`--preset` / `--tools` が RoutePlan より優先されること。

## 6. route fallback

1. `aibe` を止めた状態、または router profile 未設定で `ai '...'` を実行する。
2. 1 回リトライ後、stderr に fallback 通知が出ること。
3. `tools=[]` の text-only 1 shot で応答が返ること。

## 7. shell 承認と `--yes-exec`

1. `shell_exec_approval=ask` で tool 付き ask を実行し、承認プロンプトが出ること。
2. 同一 session 内で `--yes-exec` を付けると承認が省略されること。
3. `shell_exec_approval=never` では `--yes-exec` でも実行されないこと。

## 8. Smart Feature Plan（0041 / 0042）

1. `ai '直近のエラーを調べて'` のようにエラー調査系の入力で、stderr に smart plan 関連の適用が出ること（mock / 実 LLM いずれも可）。
2. `ai '作業の目的を整理したい'` で memory recipe 提案が turn に載ること（memory 有効時）。
3. `ai --tools read_file '...'` では CLI 明示値が feature plan より優先されること。
4. `ai history` の replay payload に memory 全文が残らず、summary は `feature_summaries` のみであること（該当 turn 後）。
5. TTY で `ai history retry` / `rerun`（元が `ask`）を実行すると `route_turn` が再実行され、現行 registry に基づく feature が再適用されること。
6. `memory.enabled=false` のとき `route_turn` は feature catalog / `feature_actions` を返さないこと（smart feature は無効）。
7. `route_turn` の `recommended_tools` に `shell_exec` が含まれても `ai` 側では read-only tool のみ採用されること（0043 Phase 2）。
8. generic memory（`kind_files=[]` + `recipe_files=[]`、feature 未指定）では AISH baseline feature が効かないこと（`FeaturePackConfig` が empty に解決される）。

## 9. Smart Preprocessor（0044）

mock 導通（実 API 不要）:

```bash
./scripts/smoke-mock.sh
cargo test -p ai smart_preprocessor -j 1
cargo test -p ai --test smart_preprocessor_ask_e2e -j 1
```

手動確認:

1. `~/.config/ai/config.toml` に `[smart_preprocessor] enabled = true` / `mode = "shadow"` を追加する。
2. TTY で `ai 'hello'` を実行し、従来どおり応答が返ること（`route_turn` は呼ばれる）。
3. `~/.local/share/ai/smart_preprocessor/observation.jsonl`（または `observation_path` 指定先）に 1 行追記されること。raw secret / 長文ログは含まれないこと。
4. `mode = "assist"` に切り替え、`AISH_SESSION_DIR` 配下に session log がある状態でエラー修正系の入力を送ると、`route_turn` の `recent_summary` が補強されること（mock / stderr 確認）。
5. `mode = "gate"` は高信頼かつ安全な入力で local route fast path を使える（`simple_chat` / `shell_help` / `vcs_inspect` 等。`memory_lookup` / `retry` / `rerun` は対象外）。危険入力（`sudo` 等）では必ず `route_turn` に落ちること。

### Phase 2.6（production 仕上げ）

1. `model_path` を省略した状態で `enabled = true` / `mode = "shadow"` とし、bundled model（`ai/resources/smart_preprocessor_model.json`）で動作すること。
2. observation JSONL に `reason_codes` / `failure_kind` / `context_needs` / `tool_hints` が出ること。raw user text / secret / path は含まれないこと。
3. `assist_threshold = 0.55`（既定）で、session error がある入力時に `recent_summary` に `session_error:` プレフィックス付き要約が入ること。
4. confidence が `route_turn_threshold`（0.85）未満の gate 入力では `route_turn` が省略されないこと。
5. session log に `permission denied` があると observation の `failure_kind` が `permission` になること。
6. git 差分相談で `context_needs` に `git_status` / `git_diff`、「前に決めた方針」で `tool_hints` に `memory_search` が出ること（debug ログまたは observation で確認）。

### Phase 2.7（`route_turn` hint wire）

1. `mode = "assist"` で git 差分相談を送ると、mock aibe / ログ上の `route_turn` request に `preprocessor_hints.context_needs` に `git_status` / `git_diff` が載ること。
2. `AISH_SESSION_DIR` に session error がある debug 入力で `preprocessor_hints.failure_kind` が載ること（例: `permission`）。
3. `MemoryLookup` / memory 系入力でも `route_turn` は呼ばれ、`preprocessor_hints` が載ること。
4. `mode = "gate"` の `hello` 短絡時は `route_turn` request が作られず、observation に `route_turn_hints_injected: false` になること。
5. observation に `route_turn_hints_present` / `route_turn_hints_injected` が区別して記録され、raw text は含まれないこと。

### Phase 2.9（local route fast path）

1. `mode = "gate"` で `git diff を見て` のような安全な git inspect 入力を送ると、`route_turn` が呼ばれず、stderr に `tools enabled: git_diff,git_status` 相当が出ること。
2. unsafe 入力（`sudo rm` 等を含む）は従来どおり `route_turn` に落ちること。
3. observation に `local_route_kind` / `local_route_used` / `route_turn_skipped_count` / `route_turn_fallback_count` / `local_route_latency_ms` / `route_turn_latency_ms` / `estimated_tokens_saved` / `route_turn_required` / `short_circuit_allowed` / `inject_hints` が記録されること（`decision_path: local_route`）。
4. `--tools` 明示値があるとき、local route がその上限を超えて tool を追加しないこと。
5. local route 成功時に `context_needs` と `output_style` が bounded な system message として agent_turn に渡ること。

## 期待結果

- TTY の `ai '...'` は常に smart entry（v1 opt-out なし）。
- 会話の正本は aibe の conversation store。`ai` local history は索引のみ。
- `route_reason` は path mask 済みで stderr / history に残る。
