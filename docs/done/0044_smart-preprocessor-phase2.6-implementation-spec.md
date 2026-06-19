# 0044 — AISH Smart Preprocessor / Local Intent Router Phase 2.6 実装指示書

> **種別**: 実装指示書（`docs/done/`）  
> **設計正本**: [0044_smart-preprocessor-spec.md](../spec/0044_smart-preprocessor-spec.md)  
> **状態**: 実装済み（Phase 2.6）  
> **起票**: 2026-06-20  
> **対象**: Phase 2.6

## 0. 目的

設計書 [0044](../spec/0044_smart-preprocessor-spec.md) の Phase 2.6 を、`ai` 側の production smart preprocessor として実装する。

Phase 2.6 では、Phase 2 の assist / hint までで止まっていた前段を仕上げる。`route_turn` の正本は維持したまま、閾値分離・観測スキーマ拡張・bundled model の標準化・failure 分類・context/tool hint の保持・session error の prefix 正規化を反映する。

このタスクでは次を守る。

1. `route_turn` / `feature_executor` の正本は変えない
2. `aish` に preprocessor を入れない
3. `Phase 4` や学習機構は実装しない
4. raw user text / secret / path を observation に残さない
5. `context_needs` / `tool_hints` はまず Decision と観測に載せ、`route_turn` への wire は後回しにする

## 1. 変更ファイル一覧

### 1.1 本体

| 区分 | パス | 変更内容 |
|------|------|----------|
| domain | `ai/src/domain/smart_preprocessor.rs` | `SmartFailureKind`、`SmartContextNeed`、`SmartToolHint`、`reason_codes` / `failure_kind` / `context_needs` / `tool_hints` の DTO 追加、閾値分離、判定ロジック更新、serde 互換調整 |
| application | `ai/src/application/smart_preprocessor.rs` | Decision の集約結果に `failure_kind` / `context_needs` / `tool_hints` を保持し、debug / observation 用に運ぶ |
| outbound | `ai/src/adapters/outbound/smart_preprocessor_observation.rs` | observation JSONL の schema 拡張、`reason_codes` / `failure_kind` / `context_needs` / `tool_hints` の永続化、redaction 強化 |
| outbound | `ai/src/adapters/outbound/smart_preprocessor_model.rs` | bundled model の load / parse fallback の扱いを明確化し、モデル読み込み失敗時の graceful fallback を維持 |
| config | `ai/src/adapters/outbound/toml_config.rs` | `assist_threshold` / `route_turn_threshold` の解釈整理、`smart_preprocessor` 設定の既定値・model_path fallback を明示化 |
| main | `ai/src/main.rs` | preprocessor decision を `route_turn` 前段へ渡す処理を更新し、observation へ新 schema を書き込む |
| tests | `ai/src/domain/smart_preprocessor.rs` | unit test を同居させる |
| tests | `ai/src/adapters/outbound/smart_preprocessor_observation.rs` | observation の redaction / schema / session error prefix を検証する |
| tests | `ai/tests/smart_preprocessor_ask_e2e.rs` | `reason_codes` / `failure_kind` / `context_needs` / `tool_hints` / bundled model / threshold の E2E を追加 |
| tests | `ai/tests/phase_a_cli.rs` | CLI 明示値優先と preprocessor 拡張の回帰を維持する |
| docs | `docs/0000_spec-index.md` | tasks セクションに Phase 2.6 を追加し、0044 の状態表記を実装中に寄せる |
| docs | `docs/testing.md` | 0044 Phase 2.6 の検証観点を追記 |
| docs | `docs/manual/ai-smart-entry.md` | Phase 2.6 の手動確認手順を追記 |
| config example | `docs/ai.config.example.toml` | `smart_preprocessor` の推奨設定例を Phase 2.6 に合わせる |

### 1.2 参照のみ

| 区分 | パス | 役割 |
|------|------|------|
| resource | `ai/resources/smart_preprocessor_model.json` | bundled model の正本。内容変更が必要な場合のみ更新する |

## 2. 実装手順

### 2.1 threshold 分離

1. `assist_threshold` は hint 供給用の閾値として扱い、`recent_summary` 注入と AssistRouteTurn の可否を同じ判定に揃える。
2. `route_turn_threshold` は gate short-circuit 用の閾値として扱い、assist と short-circuit を混線させない。
3. config の既定値は設計書の値に合わせるが、`assist_threshold` と `route_turn_threshold` の意味は固定する。

### 2.2 reason_codes

1. `SmartPreprocessDecision.reason_codes` を観測正本として扱う。
2. reason code は短い定数列に限定し、raw user text やエラー全文をコピーしない。
3. observation JSONL にも reason code 配列をそのまま残す。

### 2.3 bundled model

1. `model_path` 未指定時は `ai/resources/smart_preprocessor_model.json` を読む。
2. model load / parse 失敗時は preprocessor 全体を失敗させず、従来どおり `route_turn` へ graceful fallback する。
3. bundled model の version / feature_extractor_version の検証を維持する。

### 2.4 failure_kind

1. `SmartFailureKind` を追加し、固定 signal から `failure_kind` を選ぶ。
2. `permission denied` 系は `permission` に分類する。
3. raw error message は `failure_kind` に置き換え、保存しない。

### 2.5 context_needs + tool_hints

1. `context_needs` と `tool_hints` を Decision に追加する。
2. git 差分相談では `git_status` / `git_diff` を `context_needs` に入れる。
3. 「前に決めた方針」や memory 系の文脈では `memory_search` を `tool_hints` に入れる。
4. 初期実装ではこれらを `route_turn` へ wire しない。

### 2.6 session_error prefix

1. `session_error_summary` の source prefix を `session_error` に揃える。
2. 将来の `stderr_tail` / `stdout_tail` / `last_command` / `exit_code` 拡張に備えて、観測と debug の出力構造を prefix 単位で分ける。

### 2.7 tests

1. unit で DTO 直列化、redaction、failure 分類、threshold 判定を固定する。
2. observation で `reason_codes` と prefix 正規化を固定する。
3. integration / E2E で bundled model、context needs、tool hints、CLI 明示値優先を固定する。

## 3. 受け入れ条件

| 条件 | 期待結果 |
|------|----------|
| assist threshold | `assist_threshold` 到達時にのみ `recent_summary` と hint が供給される |
| route_turn threshold | `route_turn_threshold` 未満では gate short-circuit しない |
| reason_codes | observation JSONL に保存され、raw text を含まない |
| bundled model | `model_path` 未指定時に bundled model を読み、失敗時は graceful fallback する |
| failure_kind | `permission denied` などの固定 signal が `failure_kind` に分類される |
| context/tool hints | `context_needs` / `tool_hints` が Decision と観測に出る |
| session_error prefix | `session_error` prefix で扱われ、将来の tail 分割に備えられる |
| CLI 優先 | `--preset` / `--tools` / `--yes-exec` の明示値が preprocessor より優先される |

## 4. `spec-acceptance.toml` 登録

Phase 2.6 の受け入れ条件は `scripts/spec-acceptance.toml` の `phase = 26` エントリに登録済み（すべて `pending = false`）。

## 5. テストコマンド

```bash
cargo test -p ai smart_preprocessor -j 1
cargo test -p ai --test smart_preprocessor_ask_e2e -j 1
cargo test -p ai --test phase_a_cli -j 1
```

## 6. 実装しないもの

1. Phase 4
2. 学習済みモデルのオンライン更新
3. self-improvement / user-specific tuning
4. `context_needs` / `tool_hints` の `route_turn` wire 昇格
5. `aish` への preprocessor 追加
6. `route_turn` の廃止

## 7. 完了条件

1. Phase 2.6 の 7 要件がすべてコードとテストに反映される。
2. `docs/0000_spec-index.md` と `docs/testing.md` / `docs/manual/ai-smart-entry.md` が同一変更で同期される。
3. `./scripts/verify.sh` が通る。
4. `route_turn` / `feature_executor` / `aish` の境界が壊れていない。
