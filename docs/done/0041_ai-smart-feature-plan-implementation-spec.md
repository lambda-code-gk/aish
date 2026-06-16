# 0041 — `ai` Smart Feature Plan 実装指示書

> **種別**: 実装指示書（`docs/done/`）  
> **設計正本**: [0041_ai-smart-feature-plan-spec.md](../spec/0041_ai-smart-feature-plan-spec.md)  
> **状態**: 実装済み  
> **起票**: 2026-06-16

## 0. 目的

`ai "..."` の smart entry に、`route_turn` が返す `feature_actions` を構造化して実行する経路を追加する。MVP では **再帰的な `ai` 呼び出しを禁止** し、`MemoryQuery`、`MemoryRecipeRun(apply=false)`、`SetLogTailBytes`、`SetRecommendedTools` だけを扱う。危険な操作は **承認ゲート経由のみ** とし、文字列ベースの汎用コマンド生成はしない。

## 1. 実装方針

1. `route_turn` の結果は `FeatureAction` の配列として受け取り、LLM に次の `ai` コマンド文字列を作らせない。
2. 実行層は `FeatureAction` を `match` で分岐し、文字列解釈や汎用スクリプト実行に落とさない。
3. 読み取り系は自動適用、書き込み系や `shell_exec` 系は承認必須に分ける。
4. `MemoryRecipeRun` は MVP では `apply=false` のみを通し、`apply=true` は実装しないか、必ず承認経路に落とす。
5. CLI 明示値 (`--preset`, `--tools`, `--log-tail`) は advisory を上書きするが、承認拒否ポリシーは越えない。

## 2. 受け入れ条件

| 条件 | 期待結果 |
|------|----------|
| `RoutePlan` | `feature_actions` が追加され、空配列デフォルトで旧挙動を壊さない |
| `ai` | `route_turn` の結果を `feature_executor` に渡し、safe action を自動適用できる |
| `承認境界` | 危険な action は承認なしに実行されない |
| `再帰禁止` | `ai` バイナリを LLM から再帰呼び出しする経路がない |
| `MVP` | `MemoryQuery` / `MemoryRecipeRun(apply=false)` / `SetLogTailBytes` / `SetRecommendedTools` だけが実行対象 |
| `互換性` | `feature_actions` の wire/serde は optional/default で後方互換を保つ |
| `verify` | `./scripts/verify.sh` が通る |
| `smoke` | `./scripts/smoke-mock.sh` が通る |

## 3. 変更ファイル一覧

| 区分 | 具体的パス | 変更内容 |
|------|------------|----------|
| protocol | `aibe-protocol/src/response.rs` | `RoutePlan.feature_actions` を追加し、serde roundtrip を固定する |
| protocol | `aibe-protocol/src/memory.rs` | `MemoryRecipeRunRequestBody.user_instruction` と `apply` のデフォルト互換を維持する |
| protocol tests | `aibe-protocol/src/response.rs` / `aibe-protocol/src/memory.rs` | serde の unit test を追加する |
| server | `aibe/src/application/route_turn.rs` | `feature_actions` を組み立てるロジックを追加する |
| server | `aibe/src/application/protocol_convert.rs` | 必要なら `RoutePlan` / DTO 変換を更新する |
| server tests | `aibe/tests/route_turn.rs` | `feature_actions` を返す回帰を固定する |
| client | `ai/src/main.rs` | route plan の解釈と feature executor 呼び出しをつなぐ |
| client | `ai/src/application/feature_executor.rs` | **新規**。safe / approval 分岐を実装する |
| client | `ai/src/adapters/outbound/memory_recipe_approval_ui.rs` | `MemoryRecipeRun(apply=true)` を将来扱う場合の承認経路を明確化する |
| client tests | `ai/src/application/feature_executor.rs` | unit test を置く |
| integration | `ai/tests/smart_feature_plan.rs` | **新規**。smart feature plan の end-to-end を固定する |
| integration | `ai/tests/phase_a_cli.rs` | 既存の smart entry / non-TTY 回帰に必要なら追加する |
| docs | `docs/architecture.md` | feature executor / approval boundary を追記する |
| docs | `docs/testing.md` | unit / integration / smoke の追加観点を追記する |
| manual docs | `docs/manual/ai-smart-entry.md` | smart feature plan の手動確認を追記する |

## 4. 実装手順

### 4.1 Protocol の土台を広げる

1. `aibe-protocol/src/response.rs` に `FeatureAction` 型を追加する。`#[serde(tag = "type", rename_all = "snake_case")]` を使い、将来の追加を additive にする。
2. `RoutePlan` に `feature_actions: Vec<FeatureAction>` を追加し、`#[serde(default, skip_serializing_if = "Vec::is_empty")]` で空配列互換を保つ。
3. `aibe-protocol/src/lib.rs` の re-export を更新し、`ai` と `aibe` から参照できるようにする。
4. `aibe-protocol/src/memory.rs` の `MemoryQueryDto` / `MemoryRecipeRunRequestBody` の既存 default を崩さないことを確認する。必要なら `serde(default)` を明示する。
5. unit test を追加し、旧 JSON に `feature_actions` がなくても deserialize できること、新 JSON が roundtrip することを固定する。

### 4.2 AIBE 側で feature actions を生成する

1. `aibe/src/application/route_turn.rs` の route draft 生成に、`feature_actions` 生成ロジックを追加する。
2. MVP の生成対象は次に限定する。
   - `MemoryQuery`
   - `MemoryRecipeRun { apply: false }`
   - `SetLogTailBytes`
   - `SetRecommendedTools`
3. `route_turn` が `memory_write`、`shell_exec`、`MemoryRecipeRun { apply: true }`、再帰 `ai` を示唆する action を返さないことを保証する。
4. `feature_actions` の順序は意味を持たせる。読み取り系を先に置き、CLI 明示値で上書きされるものは後段で評価する。
5. `aibe/tests/route_turn.rs` に、`feature_actions` を含む JSON の roundtrip と、MVP 外 action が生成されない回帰を追加する。

### 4.3 `ai` に feature executor を組み込む

1. `ai/src/application/feature_executor.rs` を新設し、`RoutePlan` を受け取って safe action と approval required action を分離する。
2. executor は `FeatureAction` の `match` だけで分岐し、文字列コマンド生成や汎用スクリプト実行をしない。
3. `SetLogTailBytes` と `SetRecommendedTools` は CLI 明示値がある場合に上書きしない。`SetRecommendedTools` は `shell_exec` を safe 扱いにしない。
4. `MemoryQuery` と `MemoryRecipeRun(apply=false)` は read-only として扱い、失敗しても turn 全体を止めず、action 単位で落として続行できるようにする。
5. `ai/src/main.rs` から route plan 受け取り後の処理を `feature_executor` に委譲し、`agent_turn` へ渡す最終 request をそこで組み立てる。
6. `ai` の非 TTY fallback では `feature_executor` を呼ばない。smart feature plan は TTY 経路に閉じる。
7. `feature_executor` は aibe を自動起動しない。memory 系の action は既存接続の best-effort にとどめる。

### 4.4 承認境界を固定する

1. `shell_exec`、`memory apply`、`recipe apply` などの副作用は承認経路に落とすか、MVP では完全に実行対象外にする。
2. 承認が必要な action を UI に出す場合は、`action` 名が見える形で表示する。
3. `never` 相当の拒否ポリシーは最上位で維持し、CLI 明示値でも越えさせない。
4. `MemoryRecipeRun { apply: true }` を将来追加する場合のために、承認 UI と protocol boundary は先に整理するが、MVP では実行しない。

### 4.5 テストと docs を同期する

1. protocol unit を追加する。
2. server integration で `feature_actions` が返ることを固定する。
3. client unit で safe / approval 分岐と CLI override を固定する。
4. client integration で `route_turn` → `feature_executor` → `agent_turn` の end-to-end を固定する。
5. `docs/architecture.md`、`docs/testing.md`、`docs/manual/ai-smart-entry.md` を実装と同時に更新する。

## 5. wire / serde 互換

1. `feature_actions` は optional 扱いで、deserialize 時は `Vec::default()` に落とす。
2. serialize 時は空配列を省略できるようにする。
3. `RoutePlan` の既存フィールドは消さない。
4. `FeatureAction` は additive に拡張し、既存 variant の tag 名を変更しない。
5. `MemoryQuery` / `MemoryRecipeRun` の request body は、既存の `serde(default)` と `skip_serializing_if` を維持する。
6. 既存クライアントが `feature_actions` を無視しても、`recommended_*` / `log_tail_bytes` の advisory は従来どおり機能する。

## 6. MVP の実装範囲

MVP で実装するのは次だけ。

- `MemoryQuery`
- `MemoryRecipeRun(apply=false)`
- `SetLogTailBytes`
- `SetRecommendedTools`

MVP ではやらないこと。

- `ai` の再帰呼び出し
- `shell_exec`
- `memory apply`
- `MemoryRecipeRun(apply=true)` の本番実行
- file write / delete
- 汎用スクリプト実行

`SetRecommendedTools` は read-only の tool だけを自動採用し、`shell_exec` や未知 tool は safe にしない。

## 7. 承認ゲートの扱い

1. `shell_exec`、`memory apply`、`recipe apply`、外部副作用 tool は承認が必要になる前提で扱う。
2. MVP では、これらは **実装しない** か、実装しても **必ず承認経路に落とす**。
3. approval gate は action 単位で機能させる。read-only action の失敗で危険側に倒さない。
4. `MemoryRecipeRun(apply=false)` は承認不要の read-only 扱いに固定する。
5. 将来 `MemoryRecipeRun(apply=true)` を追加する場合も、`apply=false` と同じコードパスに混ぜず、明示的な分岐にする。

## 8. テスト追加方針

### 8.1 unit

期待ファイル場所:

- `aibe-protocol/src/response.rs`
- `aibe-protocol/src/memory.rs`
- `ai/src/application/feature_executor.rs`

観点:

- `RoutePlan.feature_actions` が空配列デフォルトで roundtrip する
- `FeatureAction` の serde が additive に壊れない
- `MemoryQuery` と `MemoryRecipeRun(apply=false)` が safe 分岐に入る
- `SetRecommendedTools` が `shell_exec` を safe にしない
- CLI 明示値が feature action より優先される

### 8.2 integration

期待ファイル場所:

- `aibe/tests/route_turn.rs`
- `ai/tests/smart_feature_plan.rs`
- 必要なら `ai/tests/phase_a_cli.rs`

観点:

- `route_turn` が `feature_actions` を返す
- `ai "..."` が mock route_turn から safe action を適用して `agent_turn` に進む
- approval required action は UI に止められる
- non-TTY では smart feature plan を経由しない

### 8.3 mock route_turn

1. `ai/tests/smart_feature_plan.rs` では mock aibe / mock route_turn を使う。
2. 可能なら既存の mock route_turn の fixture を再利用し、`feature_actions` だけ差し替える。
3. `shell_exec` や実ネットワークには依存しない。

## 9. `./scripts/verify.sh` と `./scripts/smoke-mock.sh` の重点確認ポイント

### 9.1 `./scripts/verify.sh`

1. protocol 追加後に `serde` roundtrip が落ちないこと。
2. `aibe` と `ai` の両方で `feature_actions` を読むコードがコンパイルできること。
3. approval gate を追加しても clippy で dead code / exhaustive match が崩れないこと。
4. `docs/testing.md` のテスト表が実ファイルと一致すること。

### 9.2 `./scripts/smoke-mock.sh`

1. `ai` の mock 起動で `route_turn` → feature executor → 最終表示まで通ること。
2. 実 API キー不要で完走すること。
3. stderr の smart plan 表示が壊れないこと。
4. safe action と approval action の分岐が smoke でも見えること。

## 10. docs の更新

このタスクでは `docs/spec/0041...` と `docs/tasks/0041...` 以外にも、挙動とプロトコルに触れる docs を更新する。

| ファイル | 更新内容 |
|----------|----------|
| `docs/architecture.md` | `RoutePlan.feature_actions`、feature executor、承認境界、MVP の safe / approval 分離を追記する |
| `docs/testing.md` | protocol unit、client integration、mock route_turn、smoke-mock の観点を追記する |
| `docs/manual/ai-smart-entry.md` | smart feature plan の表示、承認表示、CLI 明示値との優先関係を追記する |

`docs/0000_spec-index.md` はこの段階では更新しない。Step 8 で更新する前提を維持する。

## 11. 完了条件

1. 上記の unit / integration が追加される。
2. `./scripts/verify.sh` が通る。
3. `./scripts/smoke-mock.sh` が通る。
4. docs 更新が実装と同じ差分に含まれる。
5. `feature_actions` が空なら旧 smart entry と同等の挙動に落ちる。
