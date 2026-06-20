# 0044 — AISH Smart Preprocessor / Local Intent Router Phase 2.7 実装指示書

> **種別**: 実装指示書（`docs/tasks/`）  
> **設計正本**: [0044_smart-preprocessor-spec.md](../spec/0044_smart-preprocessor-spec.md)  
> **状態**: 実装済み（Phase 2.7）  
> **起票**: 2026-06-20  
> **対象**: Phase 2.7

## 0. 目的

設計書 [0044](../spec/0044_smart-preprocessor-spec.md) の Phase 2.7 を実装する。Phase 2.6 で確立した `SmartPreprocessDecision` の観測正本を維持したまま、`route_turn` への hint wire を additive に追加し、`route_turn_required` / `short_circuit_allowed` / `inject_hints` を独立した判定軸として扱えるようにする。

このタスクでは次を守る。

1. Phase 2.6 の成果を前提にし、`route_turn` の本体契約は壊さない
2. `route_turn_required` と `inject_hints` を混線させない
3. `MemoryLookup` / `MemoryRecipeHint` は短絡せず、必要な hint だけを渡せるようにする
4. `aibe` 側の `route_turn` は hint を advisory としてのみ扱う
5. raw user text / raw tool output / raw error text を wire や observation に載せない
6. short-circuit の対象クラスを Phase 2.7 で広げない

## 1. Phase 2.7 スコープと前提

### 1.1 前提

- Phase 2.6 は完了済みである
- `SmartPreprocessDecision` には `reason_codes` / `failure_kind` / `context_needs` / `tool_hints` が存在する
- observation ログは Phase 2.6 の schema を持っている
- ただし `RouteTurnConversation` には preprocessor 由来の additive hints がまだ wire されていない

### 1.2 Phase 2.7 のスコープ

- 3 軸 gate を分離する
- `SmartRouteTurnHints` を拡張する
- `RouteTurnConversation` に additive wire を追加する
- `aibe` の `route_turn` で hint を advisory として参照できるようにする
- observation に `route_turn_hints_present` / `route_turn_hints_injected` を残す

### 1.3 非目標

- Phase 2.7 で short-circuit の対象 intent を増やさない
- preprocessor hint を `route_turn` の最終判定にしない
- `aibe` 側で hint を必須化しない
- raw text を保存しない方針を緩めない

## 2. 変更ファイル一覧

| 区分 | パス | 変更内容 |
|------|------|----------|
| domain | `ai/src/domain/smart_preprocessor.rs` | `SmartRouteTurnHints` を拡張し、wire 用 subset を切り出せるようにする。`route_turn_required` / `short_circuit_allowed` / `inject_hints` の 3 軸判定に必要な DTO とテストを追加する |
| application | `ai/src/application/smart_preprocessor.rs` | 3 軸 gate の分離結果を保持し、`build_route_turn_request()` に渡せる形へ集約する |
| main | `ai/src/main.rs` | `RouteTurnConversation` へ additive hint を載せる。short-circuit 時は request を作らず、hint 注入の有無だけを観測に残す |
| protocol | `aibe-protocol/src/request.rs` | `RouteTurnConversation` に `preprocessor_hints: Option<RouteTurnHints>` を追加し、`RouteTurnHints` DTO を新設する |
| aibe | `aibe/src/application/route_turn.rs` | `preprocessor_hints` を advisory として読み、prompt / context / tool recommendation の補助に使う。最終判定ロジックは変えない |
| observation | `ai/src/adapters/outbound/smart_preprocessor_observation.rs` | `route_turn_hints_present` / `route_turn_hints_injected` を追加し、hint wire の成否を区別して保存する |
| tests | `ai/src/domain/smart_preprocessor.rs` | 3 軸 gate 分離、`SmartRouteTurnHints` 拡張、hint subset の serde を固定する unit test を追加する |
| tests | `aibe-protocol/src/request.rs` | `RouteTurnConversation` / `RouteTurnHints` の additive roundtrip を固定する |
| tests | `aibe/src/application/route_turn.rs` | hint があっても `route_turn` の最終判定が変わらないことを固定する |
| tests | `ai/src/adapters/outbound/smart_preprocessor_observation.rs` | hint present / injected の区別と raw text 非保持を固定する |
| tests | `ai/tests/smart_preprocessor_ask_e2e.rs` | `MemoryLookup` / `MemoryRecipeHint` / git 差分相談 / debug failure の E2E を追加する |
| tests | `ai/tests/phase_a_cli.rs` | CLI 明示値優先と short-circuit 非拡張を回帰で守る |
| spec-acceptance | `scripts/spec-acceptance.toml` | Phase 2.7 の AC を `pending = false` 前提で登録する |
| docs | `docs/0000_spec-index.md` | `docs/tasks/` の 0044 Phase 2.7 を一覧に追加する |

## 3. 実装手順

### 3.1 3 軸 gate 分離

1. `route_turn_required` は policy / safety / memory 要件で決める。
2. `short_circuit_allowed` は confidence と safety の条件で決める。
3. `inject_hints` は `route_turn_required` とは独立に決める。
4. `route_turn_required=true` でも `inject_hints=true` なら request を作る。
5. `short_circuit_allowed=true` でも `inject_hints=true` なら request を作って hint を渡す。
6. `short_circuit_allowed=true` かつ `route_turn_required=false` かつ `inject_hints=false` のときだけ、`route_turn` 省略候補になる。
7. `MemoryLookup` / `MemoryRecipeHint` は常に `route_turn_required=true` とするが、hint 注入は許可する。

### 3.2 `SmartRouteTurnHints` 拡張

1. `SmartRouteTurnHints` に wire 用 subset を追加する。
2. 追加する項目は `context_needs`、`tool_hints`、`failure_kind`、`preprocessor_intent`、`preprocessor_reason_codes` とする。
3. `preprocessor_reason_codes` は `reason_codes` のうち wire に載せてよい列挙値だけを保持する。
4. raw text に逆引きできる情報は持たない。
5. `recent_summary` は既存フィールドとして維持する。

### 3.3 `RouteTurnConversation` wire

1. `aibe-protocol` に `RouteTurnHints` DTO を新設する。
2. `RouteTurnConversation` に `preprocessor_hints: Option<RouteTurnHints>` を additive に追加する。
3. `serde(default)` と `skip_serializing_if = "Option::is_none"` を併用する。
4. `deny_unknown_fields` は付けない。
5. 既存サーバは hints を無視して従来どおり動く前提を壊さない。

### 3.4 `aibe` の `route_turn` 利用

1. `aibe/src/application/route_turn.rs` で `preprocessor_hints` を読めるようにする。
2. hint は prompt / context / tool recommendation の補助に限定する。
3. hint を最終 route kind の決定に使わない。
4. unknown な hint 値は無視する。
5. `route_turn` の fallback と conversation store の正本は維持する。

### 3.5 observation 拡張

1. observation に `route_turn_hints_present` を追加する。
2. observation に `route_turn_hints_injected` を追加する。
3. `route_turn` request を作らなかった場合は `route_turn_hints_injected=false` とする。
4. raw text / secret / path を observation に残さない。
5. `route_turn_hints_present` と `route_turn_hints_injected` は別物として記録する。

## 4. 受け入れ条件

### 4.1 設計書 §16.3 の重点検証

| 条件 | 期待結果 |
|------|----------|
| `MemoryLookup` | `route_turn` は必須だが、hints が載る |
| `MemoryRecipeHint` | `route_turn` は必須だが、hints が載る |
| git 差分相談 | `context_needs = git_status / git_diff` が `route_turn` request に入る |
| debug failure | `session_error_summary` がある入力で `failure_kind` が `route_turn` request に入る |
| gate short-circuit | `route_turn` request が作られない |
| unsafe input | short-circuit しないが、必要なら hints は載る |
| observation | `route_turn_hints_present` と `route_turn_hints_injected` が区別される |

### 4.2 ユーザー指示 6 項目

| 条件 | 期待結果 |
|------|----------|
| Phase 2.6 完了前提 | Phase 2.7 は 2.6 の wire 以外の正本を壊さない |
| 3軸 gate 分離 | `route_turn_required` / `short_circuit_allowed` / `inject_hints` が独立して判定される |
| `SmartRouteTurnHints` 拡張 | wire 用 subset が追加される |
| `RouteTurnConversation` wire | additive / optional / `serde(default)` で追加される |
| `aibe route_turn` 利用 | hints を advisory として使える |
| observation 拡張 | hint の有無と注入有無が観測できる |

## 5. `scripts/spec-acceptance.toml` 登録案

Phase 2.7 の登録は `pending = false` を前提に、以下の AC を追加する。

| phase | id | test 関数 | file_glob | 意図 |
|------|----|-----------|-----------|------|
| 27 | `memory_lookup_hints` | `memory_lookup_keeps_route_turn_and_injects_hints` | `ai/tests/smart_preprocessor_ask_e2e.rs` | `MemoryLookup` でも `route_turn` 必須かつ hint 注入あり |
| 27 | `memory_recipe_hint_hints` | `memory_recipe_hint_keeps_route_turn_and_injects_hints` | `ai/tests/smart_preprocessor_ask_e2e.rs` | `MemoryRecipeHint` でも `route_turn` 必須かつ hint 注入あり |
| 27 | `git_context_needs` | `git_diff_consultation_injects_context_needs` | `ai/tests/smart_preprocessor_ask_e2e.rs` | `git_status` / `git_diff` が wire に載る |
| 27 | `debug_failure_kind` | `session_error_summary_injects_failure_kind_into_route_turn` | `ai/tests/smart_preprocessor_ask_e2e.rs` | `session_error_summary` 由来の `failure_kind` を wire する |
| 27 | `gate_short_circuit_request` | `gate_short_circuit_skips_route_turn_request` | `ai/tests/smart_preprocessor_ask_e2e.rs` | short-circuit 時に request を作らない |
| 27 | `observation_hint_flags` | `observation_distinguishes_hint_present_and_injected` | `ai/src/adapters/outbound/smart_preprocessor_observation.rs` | observation の 2 フラグを固定する |

登録時は `scripts/spec-acceptance.toml` に `[[cases]]` を追加し、各ケースを `pending = false` で置く。

## 6. テスト関数案

### 6.1 `ai/tests/smart_preprocessor_ask_e2e.rs`

- `memory_lookup_keeps_route_turn_and_injects_hints`
- `memory_recipe_hint_keeps_route_turn_and_injects_hints`
- `git_diff_consultation_injects_context_needs`
- `session_error_summary_injects_failure_kind_into_route_turn`
- `gate_short_circuit_skips_route_turn_request`

### 6.2 `ai/src/domain/smart_preprocessor.rs`

- `smart_route_turn_hints_extend_with_wire_subset`
- `three_axis_gate_decision_is_independent`
- `memory_lookup_stays_route_turn_required_but_allows_hint_injection`

### 6.3 `aibe-protocol/src/request.rs`

- `route_turn_conversation_roundtrip_with_preprocessor_hints`
- `route_turn_hints_serialize_as_additive_optional_field`

### 6.4 `aibe/src/application/route_turn.rs`

- `preprocessor_hints_do_not_change_final_route_kind`
- `unknown_preprocessor_hints_are_ignored`

### 6.5 `ai/src/adapters/outbound/smart_preprocessor_observation.rs`

- `observation_distinguishes_hint_present_and_injected`
- `observation_does_not_store_raw_text_for_preprocessor_hints`

## 7. 完了条件

1. Phase 2.7 の AC がすべて `pending = false` になる
2. `RouteTurnConversation` の additive wire が既存クライアント / サーバを壊さない
3. `route_turn_required` と `inject_hints` の意味が分離される
4. `./scripts/verify.sh` が通る
5. 本ファイルを `docs/done/` へ移動し、`docs/0000_spec-index.md` を更新する

## 8. 仕様との差分

- なし
