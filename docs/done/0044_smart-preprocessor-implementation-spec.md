# 0044 — AISH Smart Preprocessor / Local Intent Router 実装指示書

> **種別**: 実装指示書（`docs/tasks/`）  
> **設計正本**: [0044_smart-preprocessor-spec.md](../spec/0044_smart-preprocessor-spec.md)  
> **状態**: 進行中  
> **起票**: 2026-06-18  
> **対象**: Phase 1 - 3

## 0. 目的

設計書 [0044](../spec/0044_smart-preprocessor-spec.md) をそのまま実装に落とし込む。Phase 1 から Phase 3 までを順に実装し、`ai` の `route_turn` 前段に Smart Preprocessor を入れる。

このタスクでは次を守る。

1. `route_turn` と `feature_executor` の正本は変えない
2. `aish` に preprocessor を入れない
3. `Phase 4` と学習機構は実装しない
4. 失敗時は必ず `route_turn` に落とす
5. `route_turn` を省略できるのは Phase 3 の狭い短絡条件だけ

### 推測

設計書に物理ファイル名が未指定のため、以下は実装上の配置案として固定する。

- 純関数 / DTO / feature hashing は `ai/src/domain/smart_preprocessor.rs`
- オーケストレーションと observation log 書き込みは `ai/src/application/smart_preprocessor.rs`
- append-only log のファイル I/O は `ai/src/adapters/outbound/smart_preprocessor_observation.rs`
- observation log の保存先は `ai` の local state 配下、例: `~/.local/share/ai/smart_preprocessor/observation.jsonl`
- `model_path` は `AI_CONFIG` 所在ディレクトリ基準で解決する

## 1. 実装順序

### Phase 1

観測のみ。`route_turn` への入力と実行経路は変えない。

#### 変更ファイル

| 区分 | パス | 変更内容 |
|------|------|----------|
| domain | `ai/src/domain/smart_preprocessor.rs` | `SmartPreprocessDecision` / `SmartPreprocessMode` / `SmartConfidenceGate` / `SmartHeadScores` / `SmartRouteTurnHints` / `SmartSafetySummary` / `SmartEvidence` の定義、feature hashing、confidence gate、redaction、serde roundtrip |
| application | `ai/src/application/smart_preprocessor.rs` | local history / session tail / CLI / provenance を束ねる preprocessor 実行器、shadow 実行、fallback 判定 |
| outbound | `ai/src/adapters/outbound/smart_preprocessor_observation.rs` | append-only observation log writer、redaction 後の永続化、byte clamp |
| config | `ai/src/adapters/outbound/toml_config.rs` | `AiConfig` に `[smart_preprocessor]` を追加し、`FileConfig` / `SmartPreprocessorSection` をパース |
| module wiring | `ai/src/domain/mod.rs` | 新規 module の `mod` / `pub use` 追加 |
| module wiring | `ai/src/application/mod.rs` | 新規 module の `mod` / `pub use` 追加 |
| module wiring | `ai/src/adapters/outbound/mod.rs` | observation log writer の `mod` / `pub use` 追加 |
| CLI entry | `ai/src/main.rs` | `run_ask` / `run_retry` / `run_rerun` から preprocessor を呼ぶ。Phase 1 では `shadow` のみ有効 |
| tests | `ai/src/domain/smart_preprocessor.rs` | unit test を同居させる |
| tests | `ai/tests/smart_preprocessor_ask_e2e.rs` | TTY + mock aibe で `route_turn` 前段に入ることを固定 |
| tests | `ai/tests/phase_a_cli.rs` | non-TTY は従来どおり `route_turn` を飛ばす回帰を維持 |

#### 実装手順

1. `ai/src/domain/smart_preprocessor.rs` を作り、DTO と純関数だけを置く。
2. feature hashing は redacted / bounded な feature のみを受け、raw command / raw output を受け取らない。
3. classifier 失敗時は `route_turn_required = true` に倒す。
4. `SmartPreprocessDecision` を serde 可能にし、basis points は 0..=10000 で固定する。
5. `ai/src/application/smart_preprocessor.rs` で入力を集約し、`shadow` モードとして decision を生成する。
6. observation log を append-only で書く。失敗しても `route_turn` 全体は失敗させない。
7. `ai/src/main.rs` では Phase 1 で `run_smart_route` の前に decision を生成するが、`route_turn` リクエスト内容は変えない。

#### 受け入れ条件

| 条件 | 期待結果 |
|------|----------|
| `enabled = false` または `mode = "off"` | preprocessor は実質無効で、既存挙動を維持する |
| `shadow` | decision と observation log のみ出る。`route_turn` は従来どおり呼ぶ |
| classifier 失敗 | `route_turn` へフォールバックする |
| redaction | raw shell log / raw LLM output / secret / 長文は残さない |
| serde | Decision DTO の roundtrip が通る |

#### テストコマンド

```bash
cargo test -p ai smart_preprocessor
cargo test -p ai --test smart_preprocessor_ask_e2e
cargo test -p ai --test phase_a_cli non_tty_ask_skips_route_turn_and_injects_ai_session_id
```

---

### Phase 2

`assist` mode を入れ、bounded な hint を `route_turn` の前段に供給する。

#### 変更ファイル

| 区分 | パス | 変更内容 |
|------|------|----------|
| domain | `ai/src/domain/smart_preprocessor.rs` | `SmartRouteTurnHints` に bounded summary 用の補助を追加 |
| application | `ai/src/application/smart_preprocessor.rs` | `recent_summary` の生成、assist 判定、RouteTurnHints へのマッピング |
| main | `ai/src/main.rs` | `RouteTurnHints` を拡張し、`build_route_turn_request()` に `recent_summary` を渡す |
| tests | `ai/tests/smart_preprocessor_ask_e2e.rs` | assist mode で `conversation.recent_summary` が入ることを固定 |
| tests | `ai/tests/phase_a_cli.rs` | CLI 明示値優先と assist の両立を固定 |
| docs | `docs/manual/ai-smart-entry.md` | assist の手動確認手順を追加 |

#### 実装手順

1. `RouteTurnHints` を `conversation_id` だけでなく `recent_summary` を持てる形に拡張する。
2. `run_smart_route` の前に preprocessor を通し、assist なら bounded summary を作る。
3. `build_route_turn_request()` で `conversation.recent_summary` にその summary を載せる。
4. summary は local history と session tail の redacted 要約だけに限定し、全文は使わない。
5. 依然として `route_turn` は必ず呼ぶ。Phase 2 では short-circuit しない。

#### 受け入れ条件

| 条件 | 期待結果 |
|------|----------|
| `assist` | `route_turn` は呼ばれる。`recent_summary` だけが補強される |
| `--preset` / `--tools` / `--yes-exec` | CLI 明示値が常に優先される |
| bounded summary | max bytes を超えない |
| CLI / TTY | 既存 smart entry の振る舞いを壊さない |

#### テストコマンド

```bash
cargo test -p ai --test smart_preprocessor_ask_e2e
cargo test -p ai --test phase_a_cli
```

---

### Phase 3

`gate` mode で、ごく狭い安全な short-circuit を有効化する。

#### 変更ファイル

| 区分 | パス | 変更内容 |
|------|------|----------|
| application | `ai/src/application/smart_preprocessor.rs` | `allow_shortcuts` / confidence / safety を見て short-circuit 可否を決める |
| main | `ai/src/main.rs` | `run_smart_route` に short-circuit 分岐を追加し、条件成立時のみ `route_turn` を呼ばない |
| tests | `ai/tests/smart_preprocessor_ask_e2e.rs` | `route_turn` 省略ケースと fallback ケースを固定 |
| tests | `ai/tests/phase_a_cli.rs` | 既存 fallback と `--preset` / `--tools` 優先を維持 |
| docs | `docs/architecture.md` | `gate` の意味と short-circuit 条件を追記 |
| docs | `docs/testing.md` | Phase 3 の検証観点を追記 |
| docs | `docs/manual/ai-smart-entry.md` | gate の手動確認を追記 |

#### 実装手順

1. `gate` mode では `allow_shortcuts` に含まれる intent だけを候補にする。
2. short-circuit 条件は `route_turn_required = false`、`gate = ShortCircuitAllowed`、`confidence_bps >= route_turn_threshold * 10000`、`safety` が全て安全、かつ `model_version` / `feature_hash_version` が一致する場合に限定する。
3. `simple_chat` 以外は短絡しない（`retry` / `rerun` / `memory_lookup` は route_turn 必須）。
4. `shell` / `write` / `network` / `memory write` が絡む入力は必ず `route_turn` に落とす。
5. short-circuit が成立した場合だけ `run_smart_route` は `route_turn` を呼ばず、最小構成の `SmartRouteOutcome` を返す。
6. それ以外は既存の `route_turn` → `feature_executor` → `agent_turn` 経路に戻す。
7. `route_turn` を使う場合の retry / fallback は既存の 2 回試行 + text-only one-shot を崩さない。

#### 受け入れ条件

| 条件 | 期待結果 |
|------|----------|
| `gate` かつ安全条件成立 | `route_turn` を呼ばずに完走する |
| `gate` かつ不確実 / high-risk | 必ず `route_turn` に落ちる |
| `retry` / `rerun` | 既存の再実行 UX を壊さない |
| fallback | `route_turn` 失敗時の text-only one-shot が残る |

#### テストコマンド

```bash
cargo test -p ai --test smart_preprocessor_ask_e2e
cargo test -p ai --test phase_a_cli
```

## 2. `run_smart_route` への統合手順

`ai/src/main.rs` の `run_smart_route` は、Phase 1 から Phase 3 まで同じ入口を使う。差分は mode に応じた分岐だけに限定する。

1. `run_ask` / `run_retry` / `run_rerun` の直前で preprocessor を実行する。
2. preprocessor の入力は `query`、`ResolvedTurnSettings`、`TurnOptions`、`RouteTurnHints`、`AI_SESSION_ID`、`AISH_SESSION_DIR`、local history tail の bounded summary に限定する。
3. `shadow` では decision を記録するだけで `build_route_turn_request()` は今までどおり呼ぶ。
4. `assist` では `RouteTurnHints.recent_summary` を埋めたうえで `build_route_turn_request()` を呼ぶ。
5. `gate` では `route_turn` を呼ぶ前に `decision` を評価し、短絡条件成立時のみ `route_turn` をスキップする。
6. `route_turn` を呼ぶ場合の retry / fallback は既存の `try_route_turn()` をそのまま使う。
7. 既存の `apply_smart_route_and_features()` は Phase 1-3 で温存し、`feature_executor` への連携を変えない。

## 3. config TOML 追加箇所

### 3.1 追加先

- `docs/ai.config.example.toml`
- `ai/src/adapters/outbound/toml_config.rs`
- `AiConfig` / `FileConfig` / 新規 `SmartPreprocessorSection`

### 3.2 追加するセクション

```toml
[smart_preprocessor]
enabled = true
mode = "shadow"
model_path = "smart_preprocessor/model.json"
feature_hash_buckets = 262144
feature_hash_seed = 17
route_turn_threshold = 0.85
assist_threshold = 0.95
max_evidence_bytes = 4096
max_observation_bytes = 512
allow_shortcuts = ["simple_chat"]
```

### 3.3 実装指示

1. `AiConfig` に smart preprocessor 専用の設定構造体を追加する。
2. 既存の `[ask]` / `[presets.*]` と混ぜず、別セクションとして読む。
3. 既存設定を上書きしない。未指定時は `enabled = false` 相当の無効状態にする。
4. `model_path` は config ファイル基準で解決する。
5. `max_evidence_bytes` と `max_observation_bytes` は log / evidence の clamp に使う。

## 4. docs 同期対象

| ファイル | 更新内容 |
|----------|----------|
| `docs/architecture.md` | `smart_preprocessor` の位置づけ、`route_turn` 前段、`gate` 短絡条件、`ai` config に `[smart_preprocessor]` を追加 |
| `docs/testing.md` | 0044 の unit / integration / mock / manual の検証表を追加 |
| `docs/manual/ai-smart-entry.md` | Phase 1-3 の手動確認手順と mock 導通コマンドを追加 |
| `docs/ai.config.example.toml` | `[smart_preprocessor]` の設定例を追加 |
| `docs/0000_spec-index.md` | tasks セクションに 0044 を追加し、状態を `進行中` にする |

## 5. Step 6 用の mock 導通コマンド

Phase 1-3 の実装確認で、少なくとも次の正常系を通す。

```bash
./scripts/smoke-mock.sh
```

この Step 6 用コマンドは `shadow` の正常系を前提にする。`gate` の short-circuit は別途 unit / integration で固定する。

## 6. 実装しないもの

次はこのタスクの対象外にする。

1. Phase 4
2. 学習済みモデルのオンライン更新
3. self-improvement / user-specific tuning
4. offline fitting / calibration の実装
5. `aish` への preprocessor 追加
6. `aibe-protocol` への DTO wire 昇格
7. `route_turn` の廃止

## 7. 完了条件

1. Phase 1 - 3 が順に実装される。
2. 4. で列挙した docs が同一変更で同期される。
3. 5. の mock 導通コマンドが通る。
4. `./scripts/verify.sh` が通る。
5. `route_turn` / `feature_executor` / `aish` の境界が壊れていない。
