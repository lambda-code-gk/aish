# 0044 — AISH Smart Preprocessor / Local Intent Router Phase 2.9 実装指示書

> **種別**: 実装指示書（`docs/tasks/`）  
> **設計正本**: [0044_smart-preprocessor-spec.md](../spec/0044_smart-preprocessor-spec.md)  
> **状態**: 進行中（Phase 2.9）  
> **起票**: 2026-06-20  
> **対象**: Phase 2.9

## 0. 目的

設計書 [0044](../spec/0044_smart-preprocessor-spec.md) の Phase 2.9 を、`ai` ローカルの `LocalRouteDecision` 導出と tool enablement fast path として実装する。

この Phase では、`SmartPreprocessDecision` をそのまま使うのではなく、local fast path 用に narrowing された `LocalRouteDecision` を作り、high confidence かつ safe な場合だけ `route_turn` を飛ばす。`route_turn` は引き続き fallback / backstop であり、`aibe` 側の wire 契約は変えない。

このタスクでは次を守る。

1. `LocalRouteDecision` は `ai` ローカルに閉じ込める
2. `route_turn` の fallback / backstop は残す
3. `tool_hints` は known safe tools の deterministic projection にのみ使う
4. `enabled_tools` は CLI 明示値の上限を超えない
5. `memory` 系は local route の対象にしない
6. `aibe-protocol` / `aibe` の wire 契約は変更しない

## 1. 変更ファイル一覧

### 1.1 本体

| 区分 | パス | 変更内容 |
|------|------|----------|
| domain | `ai/src/domain/smart_preprocessor.rs` | `LocalRouteKind` / `LocalOutputStyle` / `LocalRouteDecision` などの local-only DTO、safe intent の deterministic projection、tool enablement 用の純関数、unit test 追加 |
| application | `ai/src/application/smart_preprocessor.rs` | `SmartPreprocessDecision` から `LocalRouteDecision` を集約し、CLI 明示値と safe projection の交差を計算して main に渡す |
| main | `ai/src/main.rs` | `run_smart_route_with_preprocessor` / `run_smart_route` の分岐を更新し、high confidence safe input では local route を使い、fallback 時のみ従来の `route_turn` を呼ぶ |
| observation | `ai/src/adapters/outbound/smart_preprocessor_observation.rs` | `local_route_kind` / `local_route_used` / `route_turn_skipped_count` / `route_turn_fallback_count` / `local_route_latency_ms` / `route_turn_latency_ms` / `estimated_tokens_saved` を追加し、turn 単位で保存する |
| tests | `ai/src/domain/smart_preprocessor.rs` | `LocalRouteDecision` の deterministic 導出、safe tool projection、CLI 上限、unsafe / medium confidence の排除を unit で固定する |
| tests | `ai/src/main.rs` | local route が `run_smart_route` を飛ばす経路、tool enablement の交差、fallback の既存契約を unit で固定する |
| tests | `ai/src/adapters/outbound/smart_preprocessor_observation.rs` | local route metrics のシリアライズと redaction を unit で固定する |
| tests | `ai/tests/smart_preprocessor_ask_e2e.rs` | TTY + mock aibe で high confidence short-circuit、medium / unsafe fallback、tool enablement projection の E2E を追加する |
| docs | `docs/0000_spec-index.md` | `docs/tasks/` の 0044 Phase 2.9 を一覧に追加する |
| spec-acceptance | `scripts/spec-acceptance.toml` | Phase 2.9 の AC を `pending = true` で登録し、RED 固定の ignored テストを先に置く |

### 1.2 参照のみ

| 区分 | パス | 役割 |
|------|------|------|
| spec | `docs/spec/0044_smart-preprocessor-spec.md` | Phase 2.9 の設計正本 |

## 2. 実装手順

### 2.1 `LocalRouteDecision` 導出

1. `SmartPreprocessDecision` から local fast path 用の `LocalRouteDecision` を導出する。
2. `LocalRouteDecision` には少なくとも `local_route_kind`、`output_style`、`enabled_tools`、`fallback_required`、`estimated_tokens_saved` を持たせる。
3. local route の初期対象は設計書どおり `simple_chat`、`shell_help`、`git_inspect`、`output_style_request`、`code_review_context_selection` に限定する。
4. `shell_exec_candidate`、`command_suggest`、`fix_error`、`agent_candidate`、`file_write_candidate`、`retry`、`rerun`、`memory_query` は local route に入れず、従来の `route_turn` fallback に残す。
5. `tool_hints` は advisory ではなく deterministic projection の入力として扱うが、未知 / unsafe tool は local enable しない。
6. high confidence でも unsafe / ambiguous / conflicting / policy-touching のシグナルがあれば `fallback_required=true` とする。

### 2.2 `run_smart_route` 分岐

1. `run_smart_route_with_preprocessor` で `LocalRouteDecision` を先に作る。
2. `fallback_required=false` かつ high confidence safe intent の場合のみ local route を使う。
3. local route を使う場合は `route_turn` RPC を発行しない。
4. fallback が必要な場合だけ従来の `run_smart_route` を呼ぶ。
5. local route が使われたときは `route_turn_skipped_count=1`、fallback 時は `route_turn_fallback_count=1` とする。
6. `route_turn` fallback 時でも、既存の `route_turn` hint wire と `feature_executor` の正本経路は壊さない。

### 2.3 tool enablement projection

1. `tool_hints` を known safe tools だけに deterministic projection する。
2. `enabled_tools` は CLI で既に許可された tool 群との交差に限定する。
3. local route は tool を追加しない。CLI 明示値が常に上限である。
4. `memory.enabled=false` の境界は変更しない。memory 系は local route ではなく既存の `route_turn` + `feature_executor` 正本に残す。
5. `output_style` は bounded な local-only 列挙として扱い、`route_turn` wire には載せない。

### 2.4 observation metrics

1. observation に `local_route_kind`、`local_route_used`、`route_turn_skipped_count`、`route_turn_fallback_count`、`local_route_latency_ms`、`route_turn_latency_ms`、`estimated_tokens_saved` を追加する。
2. `route_turn_skipped_count` は local route で `route_turn` を呼ばなかった turn のみ `1` にする。
3. `route_turn_fallback_count` は local route 試行後に `route_turn` に落ちた turn のみ `1` にする。
4. `local_route_latency_ms` は preprocessor 受理から local decision 完了までを測る。
5. `route_turn_latency_ms` は `route_turn` RPC 実行時のみ測る。
6. `estimated_tokens_saved` は定数ベースの概算として扱い、判定条件には使わない。
7. raw shell log / raw LLM 出力 / raw tool output / secret / path は引き続き observation に残さない。

### 2.5 tests

1. unit では `LocalRouteDecision` の deterministic 性と safe tool projection を固定する。
2. integration / E2E では high confidence safe input の short-circuit と medium / unsafe fallback を固定する。
3. observation unit では新しい metrics が turn 単位で保存されることを固定する。
4. 追加するテストはすべて `#[ignore]` 付きで先に置き、`scripts/spec-acceptance.toml` を `pending = true` のまま RED 固定にする。

## 3. 受け入れ条件

| 条件 | 期待結果 |
|------|----------|
| `LocalRouteDecision` 導出 | `SmartPreprocessDecision` から deterministic に導出される |
| safe intent | high confidence safe input は local route に入り、`route_turn` を呼ばない |
| fallback | medium / low confidence、unsafe、曖昧な入力は `route_turn` に落ちる |
| tool enablement | `tool_hints` は known safe tools のみへ投影され、CLI 明示値を超えない |
| memory 境界 | memory 系は local route に入らず、既存の `route_turn` / `feature_executor` 正本経路に残る |
| observation metrics | `local_route_kind` / `local_route_used` / `route_turn_skipped_count` / `route_turn_fallback_count` / `local_route_latency_ms` / `route_turn_latency_ms` / `estimated_tokens_saved` が保存される |
| wire 境界 | `aibe-protocol` / `aibe` の wire 契約は変わらない |

## 4. `scripts/spec-acceptance.toml` 登録

Phase 2.9 の AC は、実装前の間はすべて `pending = true` とし、対応する Rust テストは `#[ignore]` 付きで先に追加する。

| phase | id | test 関数 | file_glob | pending |
|------|----|-----------|-----------|---------|
| 29 | `local_route_decision` | `local_route_decision_is_deterministic` | `ai/src/domain/smart_preprocessor.rs` | true |
| 29 | `tool_enablement_projection` | `local_route_enabled_tools_are_clamped_to_cli_allowlist` | `ai/src/main.rs` | true |
| 29 | `route_turn_short_circuit` | `local_route_skips_route_turn_for_high_confidence_safe_input` | `ai/tests/smart_preprocessor_ask_e2e.rs` | true |
| 29 | `route_turn_fallback` | `local_route_falls_back_to_route_turn_for_medium_or_unsafe_input` | `ai/tests/smart_preprocessor_ask_e2e.rs` | true |
| 29 | `observation_metrics` | `local_route_observation_records_metrics` | `ai/src/adapters/outbound/smart_preprocessor_observation.rs` | true |

登録後は `scripts/check-spec-acceptance.py` が Phase 2.9 の pending 状態を検出できることを前提にする。

## 5. Step 6 用 smoke / 追加コマンド

### 5.1 RED 固定の確認

```bash
cargo test -p ai local_route -j 1 -- --ignored
cargo test -p ai --test smart_preprocessor_ask_e2e -j 1 -- --ignored
```

### 5.2 実装後の smoke

```bash
cargo test -p ai local_route -j 1
cargo test -p ai --test smart_preprocessor_ask_e2e -j 1
cargo test -p ai --test smart_preprocessor_ask_e2e -j 1 -- --nocapture
```

### 5.3 最終確認

```bash
./scripts/verify.sh
```

## 6. 実装しないもの

1. `route_turn` の廃止
2. `aibe-protocol` / `aibe` の wire 契約変更
3. `memory_query` / `memory_recipe_hint` の local route 化
4. shell_exec、file write、memory write、approval、network の local enable
5. unknown / unsafe tool の automatic enable
6. learning / online fitting / self-improvement

## 7. 完了条件

1. Phase 2.9 の AC がすべて `pending = false` になる。
2. `LocalRouteDecision`、tool enablement projection、observation metrics がコードとテストに反映される。
3. `docs/0000_spec-index.md` と `scripts/spec-acceptance.toml` が同一変更で同期される。
4. `./scripts/verify.sh` が通る。
5. 本ファイルを `docs/done/` へ移動し、`docs/0000_spec-index.md` を更新する。

