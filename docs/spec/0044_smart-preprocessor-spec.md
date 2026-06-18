# 0044 — AISH Smart Preprocessor / Local Intent Router 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **起票**: 2026-06-18  
> **関連**: [0030_ai-smart-entry-spec.md](0030_ai-smart-entry-spec.md)、[0041_ai-smart-feature-plan-spec.md](0041_ai-smart-feature-plan-spec.md)、[0042_configurable-smart-features-spec.md](0042_configurable-smart-features-spec.md)、[0043_feature-pack-boundary-hardening-spec.md](0043_feature-pack-boundary-hardening-spec.md)、[architecture.md](../architecture.md)、[testing.md](../testing.md)、[security.md](../security.md)

## 0. 目的

本仕様は、`ai` クライアント内に置く **Smart Preprocessor / Local Intent Router** を定義する。

役割は 1 つである。

- ユーザー入力、TTY / non-TTY、会話状態、`aish` 由来の session provenance、bounded な session log tail、直近の local history を材料に、`route_turn` を呼ぶ前の **局所的な意図判定** を行う

この局所判定は `route_turn` の置き換えではない。`route_turn` は従来どおり aibe 側の軽量 LLM fallback であり、本仕様の preprocessor はその **前段の補助レイヤ** である。

本仕様の狙いは次の通りである。

1. `route_turn` に入る前に、軽量で安全な local intent 判定を行う
2. 低コストで確実なケースは、LLM に全部を委ねずに前処理で整理する
3. 不確実・危険・副作用ありのケースは、既存の `route_turn` と approval / policy に必ず落とす
4. 観測ログのみを残し、学習機構は導入しない

## 1. 非目標

- 学習済みモデルのオンライン更新
- classifier の重み学習、自己改善、ユーザー別最適化
- `aish` に LLM / HTTP クライアントを入れること
- `aibe` の approval gate や policy を bypass すること
- 長文の shell log、LLM 出力全文、tool output 全文を classifier に投入すること
- Windows 対応
- `route_turn` を廃止すること

## 2. 0030 / 0041 / 0042 / 0043 との関係

### 2.1 0030 との関係

0030 は `ai` の smart entry と `route_turn` の基盤を定義した。本仕様はその前段に、**LLM を使わない局所判定** を追加する。

0030 の `route_turn` は依然として aibe 側の正本である。本仕様は `route_turn` の代替ではなく、`route_turn` に渡す入力を整理し、確信度が十分高い場合だけ一部を短絡する補助層である。

### 2.2 0041 / 0042 との関係

0041 / 0042 は `route_turn` が返す `feature_actions` と feature registry の仕組みを定義した。本仕様の preprocessor は、feature action の実行主体ではない。

preprocessor ができるのは次の 3 点だけである。

- feature 由来になりうる入力シグナルを局所抽出する
- `route_turn` に入れる前に、bounded な hint を作る
- 高信頼な読取系の前処理だけを `ai` 内で短絡候補化する

feature action の実行、memory 書き込み、shell 実行昇格、approval gate は既存の 0041 / 0042 / 0036 / 0043 の責務である。

### 2.3 既存の smart entry / feature plan への挿し込み

preprocessor は `ai` の smart entry 前段に入り、`route_turn` を呼ぶかどうかと、呼ぶ場合の入力を整える。

既存フローとの対応は次のとおりである。

- `off` / `shadow` は現行の `run_smart_route` に観測を差し込むだけで、`route_turn` / `feature_executor` の意味論は変えない
- `assist` は `RouteTurnHints` と bounded summary を補強するが、`route_turn` を必ず通す
- `gate` は高信頼かつ安全な `simple_chat` のみ `route_turn` を省略し、それ以外は現行の `route_turn` → `feature_executor` → `agent_turn` に戻す
- `memory_lookup` / `retry` / `rerun` は transcript または memory 経路が必要なため短絡対象外
- `feature_executor` は 0041 / 0042 の正本のままで、preprocessor は feature action を直接実行しない
- `--preset` / `--tools` / `--log-tail` / `--yes-exec` の CLI 明示値は、preprocessor の出力よりも常に優先する
- 実装上の差し込み点は `ai/src/main.rs` の `run_smart_route` / `build_route_turn_request` の前後であり、`feature_executor` への連携は既存の `execute_feature_actions_mvp` を再利用する

### 2.4 0043 との関係

0043 で `memory.enabled=false` と pack 境界が整理された。本仕様では、その状態判定を壊さない。

preprocessor は `memory.enabled` の真偽を見て局所的に保守的になることはあっても、`memory.enabled=false` を自力で無効化したり、逆に有効化したりしない。

### 2.5 aish との関係

`aish` は shell 起動と JSONL session logging のみを担う。preprocessor は `aish` に入れない。

`aish` は provenance と log tail の供給源であり、classifier の実装主体ではない。

## 3. クレート境界

| クレート | 責務 |
|---------|------|
| **aish** | shell 実行、session provenance、session log の記録のみ |
| **ai** | Smart Preprocessor / Local Intent Router、本仕様の本体 |
| **aibe-protocol** | もし preprocessor の決定を wire に載せる必要が出た場合の DTO 正本。ただし MVP では最小限に留める |
| **aibe** | 既存の `route_turn`、approval gate、feature plan、conversation store の正本。preprocessor の判断主体ではない |

依存方向は変えない。

- `ai` は `aibe` 本体へ直接依存しない
- `ai` は `aibe-client` / `aibe-protocol` 経由でのみ通信する
- `aish` は LLM に接続しない

## 4. Hard Rules

1. classifier は policy / approval を bypass しない
2. classifier は実行器ではない。判定結果は advisory である
3. classifier は raw の長文 shell log や LLM 出力全文を見ない
4. classifier は redacted / bounded な signal のみを使う
5. `route_turn` は front stage の補助であり、fallback かつ policy backstop である
6. 失敗時は fail-open ではなく、既存の `route_turn` に落とす
7. `--preset` / `--tools` / `--log-tail` / `--yes-exec` といった CLI 明示値は preprocessor より優先する
8. 学習機構は実装しない。残すのは観測ログのみ
9. `aish` に LLM / HTTP 実装を追加しない

## 5. Signal / Feature Extraction

### 5.1 入力ソース

preprocessor が見る入力は次に限定する。

| ソース | 使い方 | 制約 |
|------|------|------|
| ユーザー query | 意図クラス判定の主入力 | 原文はそのまま保持せず、feature 化する |
| CLI 既定値 | `--new`、`--tools`、`--preset`、`--log-tail`、`--yes-exec` | 明示値は上書きしない |
| TTY / non-TTY | smart entry 継続可否 | non-TTY では保守的に動く |
| aish provenance | `AI_SESSION_ID`、`AISH_SESSION_DIR` | 参照だけ。保存しない |
| bounded session log tail | shell error、直近コマンド失敗、直近出力の型 | raw 全文ではなく capped summary |
| local history | 直近の command / status / summary | replay 用 transcript は見ない |
| existing route metadata | 前回の route kind、fallback 有無、approval 必要性 | 直近 1 turn 以内を優先 |

### 5.2 抽出ルール

抽出は pure / deterministic に行う。

- 文字列は ASCII / Unicode を問わず正規化する
- 絶対パス、秘密らしいトークン、長大な連続文字列は即 redaction する
- raw shell log は最大 byte 数を定めて切る
- LLM 出力全文は入力に入れない
- 失敗時は feature を落とすだけで、入力全体を失敗させない

### 5.3 Feature Set

preprocessor の feature は小さく、機械的に扱えるものに限定する。

| Feature 群 | 例 |
|----------|----|
| 構文 | 単文 / 複文 / コード fence / shell-like token / path token |
| 意図 | 疑問、要約、調査、修正、実行、再試行、再実行 |
| リスク | shell_exec 候補、書き込み候補、ネットワーク候補、secret 候補 |
| 文脈 | `--new`、既存 conversation 継続、retry / rerun、TTY 有無 |
| ログ | recent error、recent failure、recent approval、recent memory hint |
| 既存機能 | 0041 / 0042 の feature に寄るヒント候補 |

feature は boolean / enum / small integer を中心にし、長い自由文は持たない。

## 6. Classifier

### 6.1 方針

classifier は **feature hashing + multi-head logistic regression** を基本とする軽量分類器とする。

入力は redacted / bounded な feature のみで構成し、学習済みの重みは固定する。オンライン学習や自己改善は行わない。

ハードルールは classifier の前段に置く安全ゲートであり、モデルの代替ではない。

- ハードルール: secret / destructive / write / network / approval などの即時保守分岐
- 共有表現: feature hashing で固定次元の sparse vector に写像する
- heads: intent / safety / gate を分けた multi-head logistic regression で判定する
- 失敗時: モデル不整合・重み未読込・入力欠損は `route_turn` に落とす

### 6.2 Feature hashing

feature hashing は、低コストで安定した入力表現を作るために使う。

- 文字列 feature は token 化したうえで hash bucket に写像する
- 絶対パス、secret らしい値、長大文字列は hash 前に redaction する
- bucket 数と seed は config で固定する
- collision は許容するが、観測ログには raw feature を残さない
- hash 版数を上げるときは model version と同時に上げる

### 6.3 出力クラス

classifier は次の intent クラスを返す。

- `simple_chat`
- `inspect`
- `debug`
- `memory_lookup`
- `memory_recipe_hint`
- `shell_command_candidate`
- `retry`
- `rerun`
- `ambiguous`
- `unknown`

各 head は intent を直接返すのではなく、確率またはスコアを返し、最終判定は gate が行う。

### 6.4 スコアリング

各 head に対して、hashed feature からスコアを計算する。

- 明示的な手掛かりを高く評価する
- `retry` / `rerun` / `--new` は優先度が高い
- `memory` 系は kind / recipe の有効状態がないと弱める
- destructive / write / network の手掛かりは自動短絡しない
- safety head は secret / destructive / write / network を保守的に重く見る
- gate head は confidence と safety を合成して short-circuit 可否を決める

推論は次の順序で行う。

1. ハードルールを先に適用する
2. feature hashing を行う
3. intent / safety / gate の各 head をスコアリングする
4. `ambiguous` の場合は route_turn を必須にする
5. 高信頼でも安全条件に合わなければ route_turn に落とす

### 6.5 classifier の責務外

classifier は次をしない。

- shell_exec の実行可否を最終決定する
- memory write を実行する
- feature action を直接投げる
- `route_turn` の LLM 出力を上書きする

## 7. Confidence Gate

### 7.1 役割

confidence gate は、classifier の結果を「そのまま使ってよいか」を判断する。

gate は intent ではなく、**実行モード** を決める。

### 7.2 Gate の判定

| 条件 | 結果 |
|------|------|
| confidence が閾値未満 | `route_turn` 必須 |
| safety feature が 1 つでも未確定 | `route_turn` 必須 |
| policy / approval に触る可能性がある | `route_turn` 必須 |
| 長文 / 多段 / 不確実 | `route_turn` 必須 |
| `--preset` / `--tools` / `--yes-exec` 明示あり | CLI 明示を優先し、preprocessor は補助のみ |
| deterministic で安全、かつ短い bounded input | local assist または short-circuit 候補 |

### 7.3 閾値

閾値は config で調整できるが、MVP の既定は保守的にする。

- short-circuit 閾値
- assist 閾値
- ambiguous 閾値
- max evidence bytes
- feature hash bucket 数
- feature hash seed
- model version
- model の内部 score は basis points、config の閾値は 0.0-1.0 の比率で扱う

閾値を上げるほど `route_turn` 依存は増え、下げるほど short-circuit は増える。MVP では後者を抑える。

## 8. Fallback

fallback は 2 層ある。

### 8.1 Classifier fallback

classifier が失敗したら、`route_turn` に落とす。

- parse failure
- signal extraction failure
- redaction failure
- threshold ambiguity
- config 不整合

### 8.2 Route fallback

`route_turn` 自体が失敗したら、既存の text-only fallback に落とす。

この順序を固定する。

1. local preprocessor
2. `route_turn`
3. text-only one-shot

preprocessor はこの 2 段 fallback の前段にすぎない。

## 9. Decision DTO

### 9.1 位置づけ

Decision DTO は **ai ローカルのドメイン DTO** である。

MVP では wire の正本にしない。必要最小限の subset のみ、将来 `aibe-protocol` に昇格させる。

### 9.2 形状

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmartPreprocessDecision {
    pub version: u32,
    pub model_version: Option<String>,
    pub feature_hash_version: u32,
    pub mode: SmartPreprocessMode,
    pub intent: SmartIntentClass,
    pub confidence_bps: u16,
    pub head_scores: SmartHeadScores,
    pub gate: SmartConfidenceGate,
    pub route_turn_required: bool,
    pub route_turn_hints: SmartRouteTurnHints,
    pub safety: SmartSafetySummary,
    pub evidence: Vec<SmartEvidence>,
}
```

### 9.3 補助型

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SmartPreprocessMode {
    Off,
    Shadow,
    Assist,
    Gate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SmartConfidenceGate {
    ForceRouteTurn,
    AssistRouteTurn,
    ShortCircuitAllowed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmartHeadScores {
    pub intent_bps: u16,
    pub safety_bps: u16,
    pub gate_bps: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmartRouteTurnHints {
    pub recent_summary: Option<String>,
    pub new_conversation: bool,
    pub conversation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmartSafetySummary {
    pub requires_approval: bool,
    pub contains_secret_risk: bool,
    pub contains_write_risk: bool,
    pub contains_network_risk: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmartEvidence {
    pub kind: String,
    pub value: String,
}
```

### 9.4 DTO の制約

- `confidence_bps` / `head_scores` は 0..=10000 の basis points で表す
- `value` は短く redacted にする
- `evidence` は bounded にする
- raw command / raw output の全文は入れない
- version は additive に上げる
- `route_turn_required=false` でも policy を bypass しない

### 9.5 wire への載せ方

MVP では DTO は `ai` 内部で完結させる。

将来、`route_turn` に compact hint を載せる必要が出た場合だけ、`aibe-protocol` に additive な optional field を追加する。`aibe` 側の既存契約は壊さない。

## 10. Safety

### 10.1 バイパス禁止

preprocessor は次を決して bypass しない。

- shell_exec approval
- safe tools policy
- memory write policy
- `route_turn` の advisory 正規化
- `ai` の CLI 明示値優先

### 10.2 Redaction

preprocessor が扱う evidence は、保存前に redaction する。

- 絶対パスは mask する
- secret らしい値は mask する
- 長文は truncate する
- command line は要約に置き換える

### 10.3 高リスク入力

次の入力は常に保守的に扱う。

- secret / token / credential を含む疑いがある
- destructive shell を含む疑いがある
- write / delete / network を伴う疑いがある
- 未知の tool 実行を示す

この場合、short-circuit はしない。

## 11. Observation Log

### 11.1 目的

Observation Log は classifier の挙動を後から検査するための、**観測のみの append-only log** である。

学習用途ではなく、評価・監査・デバッグ用途に限定する。

### 11.2 記録内容

各レコードには次を含める。

- `timestamp_ms`
- `ai_session_id`
- `conversation_id`
- `history_id`
- `model_version`
- `feature_hash_version`
- `mode`
- `intent`
- `confidence_bps`
- `head_scores`
- `gate`
- `decision_path`
- `route_turn_used`
- `fallback_reason`
- `signal_counts`
- `redaction_stats`

`decision_path` の値は次のいずれかとする。

- `shadow`
- `assist`
- `gate_short_circuit`
- `route_turn`
- `route_turn_fallback`
- `text_only_fallback`

### 11.3 記録禁止

Observation Log に入れてはいけないもの。

- raw shell log 全文
- raw LLM 出力全文
- raw tool output 全文
- secret 文字列
- path の未 redaction な全文

### 11.4 保存先

保存先は `ai` の local state に置く。`aish` と `aibe` はここを書き換えない。

local history の既存 payload は turn の正本であり、Observation Log は classifier の観測正本である。両者は役割を分ける。

## 12. Config

### 12.1 所属

preprocessor の config は `ai` 側に置く。

`aish` に新しい config セクションは追加しない。

### 12.2 例

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

### 12.3 解釈

- `enabled = false` は完全無効
- `shadow` は記録のみ
- `assist` は hint 生成まで
- `gate` は short-circuit 候補を有効化するが、policy は超えない
- threshold は保守的な既定を持つ

### 12.4 既存 config との関係

- `route_turn` の profile 設定はそのまま使う
- `memory.enabled` はそのまま使う
- `shell_exec` approval 設定はそのまま使う
- preprocessor は既存設定を読み、上書きしない

## 13. Modes

### 13.1 `off`

preprocessor を使わない。

- 既存の `route_turn` だけを使う
- 既存挙動と完全互換

### 13.2 `shadow`

判定と観測ログだけ行う。

- 実行経路は変えない
- `route_turn` は従来どおり呼ぶ
- MVP の最初の到達点

### 13.3 `assist`

bounded な hint を作る。

- `route_turn` は引き続き呼ぶ
- `RouteTurnConversation.recent_summary` など既存入力の範囲で補助する
- CLI 明示値は変えない

### 13.4 `gate`

高信頼かつ安全なごく狭いケースだけ短絡候補にする。

- 不確実なら `route_turn`
- approval / policy には触らない
- ここが `route_turn` の置き換えではないことを厳密に守る

### 13.5 既存実行フロー

preprocessor の各 mode と既存実行フローの対応は次のとおりである。

| mode | route_turn | feature_executor | 備考 |
|------|------------|------------------|------|
| `off` | 実行する | 実行する | 現行互換 |
| `shadow` | 実行する | 実行する | 観測のみ |
| `assist` | 実行する | 実行する | bounded hint を補強 |
| `gate` | 条件付きで省略 | 条件付きで省略 | short-circuit 条件不成立時は通常経路、成立時は両方省略 |

## 14. Phases 1-4 MVP

### 14.1 Phase 1

**MVP 1**。観測のみで、実行挙動は変えない。

- signal extractor を追加する
- feature hashing と multi-head classifier を shadow で実行する
- Decision DTO を追加する
- Observation Log を書く
- mode は `shadow`

この段階では `route_turn` への入力は変えない。

### 14.2 Phase 2

**MVP 2**。安全な hint のみを `route_turn` の前段に入れる。

- `assist` mode を追加する
- bounded な `recent_summary` を作る
- retry / rerun / simple inspect の局所補助を入れる
- それでも `route_turn` は原則呼ぶ
- 影響評価は observation log を使って行う

### 14.3 Phase 3

**MVP 3**。ごく狭い short-circuit を許す。

- 高信頼の `simple_chat` のみを短絡候補にする（`retry` / `rerun` / `memory_lookup` は transcript または memory 経路が必要なため短絡対象外）
- shell / write / network / memory write は短絡しない
- 不確実なら必ず `route_turn`
- `route_turn` の fallback 経路は維持する
- short-circuit の条件は model version と gate confidence が両方揃った場合に限る

### 14.4 Phase 4

**follow-up**。MVP 外。

- observation log を使った閾値調整
- reviewed observation log を使った offline fitting / calibration
- ルール更新のオフライン検証
- intent 辞書の増補
- 追加 DTO の wire 昇格検討

Phase 4 でも学習機構は導入しない。人間がレビューした設定更新のみを対象とする。

## 15. 受け入れ条件

1. `ai` は local intent を deterministic に抽出できる
2. classifier の出力は redacted / bounded である
3. 失敗時は必ず `route_turn` に落ちる
4. `route_turn` は前段補助として残り、置き換えられない
5. shell approval / memory policy / CLI 明示値を bypass しない
6. `aish` に LLM / HTTP を持ち込まない
7. Observation Log だけが残り、学習機構は実装されない
8. `memory.enabled=false` や 0043 の pack 境界を壊さない
9. 既存の `ai` smart entry と history / retry / rerun の UX を維持する

## 16. テスト方針

| 種別 | 内容 | 置き場所の目安 |
|------|------|----------------|
| unit | signal extractor、feature hashing、multi-head classifier、confidence gate、redaction | `ai/src/application/` または `ai/src/domain/` |
| unit | Decision DTO の serde roundtrip | `ai` の DTO 定義近傍 |
| integration | `ai ask` で preprocessor が `route_turn` の前段に入ること | `ai/tests/*.rs` |
| integration | shadow / assist / gate の mode 切替 | `ai/tests/*.rs` |
| integration | 失敗時に `route_turn` fallback へ落ちること | `ai/tests/phase_a_cli.rs` または新規テスト |
| regression | feature hash seed / bucket 数の変更で version が変わること | `ai/src/application/` または `ai/src/domain/` |
| regression | `--preset` / `--tools` / `--yes-exec` 明示値優先 | 既存 smart entry テスト |
| docs | `docs/testing.md` と `docs/manual/ai-smart-entry.md` を同一変更で同期 | docs 変更時 |

### 16.1 重点検証

- classifier が raw 長文を食べないこと
- `route_turn` の既存契約を壊さないこと
- `route_turn` が失敗しても text-only fallback が残ること
- high-risk 入力で short-circuit しないこと
