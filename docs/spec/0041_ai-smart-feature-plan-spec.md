# 0041 — `ai` Smart Feature Plan 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **起票**: 2026-06-16  
> **関連**: [0030_ai-smart-entry-spec.md](0030_ai-smart-entry-spec.md)、[0037_aibe-contextual-memory-runtime-v1-spec.md](0037_aibe-contextual-memory-runtime-v1-spec.md)、[0039_aish-memory-pack-externalization-spec.md](0039_aish-memory-pack-externalization-spec.md)、[0040_generic-recipe-cli-aish-name-cleanup-spec.md](0040_generic-recipe-cli-aish-name-cleanup-spec.md)、[architecture.md](../architecture.md)、[testing.md](../testing.md)

## 0. 目的

`ai "..."` を日常の既定入口のまま維持しつつ、AIBE の `route_turn` が返す情報を **RoutePlan の拡張として構造化し**、`ai` が安全な機能を自動適用し、危険な機能は承認付きで実行できるようにする。

本仕様の狙いは、ユーザーが `--tools`、`--log-tail`、`--preset`、`ai mem run ...` のような細かい操作を毎回覚えなくてもよいことにある。AIBE は query/context を見て、会話ルーティングに加えて **feature plan** を提案する。`ai` はその提案を CLI 上で解釈し、実行する。

本書の正本は次の 4 点である。

1. `RoutePlan` に `feature_actions: Vec<FeatureAction>` を追加する
2. `ai` は `route_turn` の返却を `feature_executor` に渡し、構造化された feature action として処理する
3. 読み取り系は自動適用し、副作用系は承認ゲートを通す
4. CLI 明示値は advisory を上書きする escape hatch として残す

`feature_executor` は aibe を自動起動しない。memory 系の action は、現在接続できる aibe / memory context を使って best-effort に処理し、失敗しても turn 全体を止めない。

## 1. 非目標

- `ai` バイナリを LLM から再帰呼び出しすること
- `ai mem run ...` のような CLI 文字列を LLM に組み立てさせること
- feature action を汎用スクリプト実行の入口にすること
- `aish` に feature plan の責務を持たせること
- shell 実行や file mutation を `route_turn` の提案だけで無条件実行すること
- Windows 対応

## 2. 0030 / 0037 / 0039 との関係

### 2.1 0030 との関係

0030 は `ai` の smart entry と `route_turn` の基盤を定義した。本仕様はその上に **機能実行の段階** を追加する。

0030 では `RoutePlan` の主な役割は、`recommended_preset`、`recommended_tools`、`log_tail_bytes`、`require_shell_approval` を advisory として返すことだった。0041 では、それに加えて **構造化された `feature_actions`** を返し、`ai` 側が実行可能な単位で扱う。

### 2.2 0037 との関係

0037 の contextual memory runtime は、`memory_query`、`memory_recipe_run`、`memory_subscribe` などの正本を AIBE 側に置いた。本仕様では、その runtime を `route_turn` の提案と結び付ける。

特に MVP では、`route_turn` が memory を直接書き換えるのではなく、`memory_query` と `memory_recipe_run(apply=false)` を feature action として提案する。書き込み系や apply 系は将来拡張として分離する。

### 2.3 0039 との関係

0039 で contextual memory の pack 外部化が進み、memory recipe は registry ベースで扱えるようになった。本仕様は、その recipe を `route_turn` の機能提案に昇格させる。

つまり、0039 が「recipe を generic に扱える」状態を作り、0041 が「その recipe を smart feature として自動/承認付きで実行する」段階を定義する。

## 3. 要約

`route_turn` は従来どおり会話ルーティングを決めるが、同時に `FeatureAction` の配列を返す。`ai` は `feature_executor` を使って、各 action を安全に実行または保留する。

実行順は次の通りである。

1. `route_turn` を 1 回呼ぶ
2. `RoutePlan` を受け取る
3. `feature_executor` が `feature_actions` を正規化する
4. 自動適用できる action を適用する
5. 承認が必要な action を gate に送る
6. 承認済み action を実行する
7. 最終的な turn request を組み立てて `agent_turn` を呼ぶ

`ai` は LLM に「次に `ai mem run ...` を実行してください」のような再帰的な CLI 文字列を返させない。LLM の出力は構造化データに限定する。

## 4. クレート境界

| クレート | 責務 |
|---------|------|
| **aibe-protocol** | `RoutePlan` / `FeatureAction` / wire DTO の定義、serde 互換の維持 |
| **aibe** | `route_turn` が `feature_actions` を生成するロジック、feature action の提案ポリシー |
| **ai** | `route_turn` の結果を解釈し、`feature_executor` と approval gate を通して `agent_turn` へ接続する |
| **aish** | 既存どおりシェル起動と記録のみ。feature plan の判断や実行は持たない |

依存方向は変えない。`ai` は `aibe` 本体へ直接依存せず、`aibe-client` / `aibe-protocol` 経由で機能する。

## 5. `FeatureAction` 定義

### 5.1 設計原則

`FeatureAction` は `route_turn` が返す「会話ルーティング以外の、構造化された実行提案」である。

設計原則は次のとおり。

- CLI 文字列ではなく構造体で表現する
- 安全な読み取りは自動適用する
- 副作用を持つものは承認ゲート必須とする
- MVP で不要な action は定義しても reserved とし、実行対象にしない
- 将来拡張は additive にする

### 5.2 MVP スコープの enum

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FeatureAction {
    MemoryQuery {
        query: MemoryQuerySpec,
    },
    MemoryRecipeRun {
        recipe_id: String,
        #[serde(default)]
        apply: bool,
    },
    SetLogTailBytes {
        bytes: u64,
    },
    SetRecommendedTools {
        tools: Vec<String>,
    },

    #[serde(other)]
    Unsupported,
}
```

#### `MemoryQuery`

`MemoryQuery` は AIBE の contextual memory を読み取る提案である。MVP では query そのものは read-only とみなし、自動適用する。

`MemoryQuerySpec` は `aibe_protocol::MemoryQueryDto` に 1:1 で対応する薄い wire DTO とする。公開フィールドは既存 query DTO と同じで、少なくとも次を持つ。

- kind
- scope
- status
- active_only
- include_archived
- limit
- include_prompt_block
- user_query

`alias` / `keyword` / ルーティング用の trigger metadata は `route_turn` 側の内部解決にのみ使い、`FeatureAction` の wire には載せない。

#### `MemoryRecipeRun`

`MemoryRecipeRun` は recipe 実行の提案である。

- MVP では `apply=false` のみを対象とする
- `apply=true` は将来拡張であり、非 goal の副作用 action になる
- `apply=false` は summary / proposals を得るだけなので read-only 扱いとする
- `feature_executor` は `MemoryRecipeRunRequestBody.user_instruction` に元のユーザー入力を渡す。`user_instruction` が無いと recipe の再現性が落ちるため、`recipe_id` だけでは実行しない

#### `SetLogTailBytes`

`SetLogTailBytes` は会話用コンテキストとして log tail を増やす提案である。

- 読み取り系
- 既存の `log_tail_bytes` より優先されることはない
- CLI 明示値がある場合は上書きしない

#### `SetRecommendedTools`

`SetRecommendedTools` は使う tools の提案である。

- `recommended_tools` と同等の意味を持つ
- 既存の tools CLI 明示値がある場合は上書きしない
- 提案のうち read-only で approval 不要な tool のみ自動採用する

`SetRecommendedTools` の safe 判定は厳密に行う。`shell_exec` と、その時点で安全性を静的に保証できない tool は自動採用しない。未知の tool は safe に分類しない。

### 5.3 将来拡張として予約する enum

以下は本仕様では定義してよいが、MVP の実行対象にはしない。

- `MemoryApply`
- `MemoryRecipeRun { apply: true }`
- `ShellExec`
- `WriteFile`
- `RunCli`
- `RunScript`
- `NetworkCall`

これらは **副作用あり** として承認ゲートの対象になるか、あるいは `route_turn` から返さない。

## 6. `RoutePlan` 拡張

### 6.1 wire 互換性

`RoutePlan` に `feature_actions` を追加しても、旧クライアントが壊れないようにする。

方針は次のとおり。

- 既存フィールドは保持する
- `feature_actions` は optional 扱いにし、`serde(default)` で空配列に落とす
- 旧クライアントが `feature_actions` を無視しても `RoutePlan` の既存意味は保つ
- 新クライアントは `feature_actions` を見て feature executor を起動する
- 旧 advisory フィールド（`recommended_preset` / `recommended_tools` / `log_tail_bytes`）は compatibility fallback とし、同種の feature action がある場合は feature action を優先する

### 6.2 DTO 形状

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoutePlan {
    pub conversation_id: String,
    pub new_conversation: bool,
    pub route_kind: RouteKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_preset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_tail_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub feature_actions: Vec<FeatureAction>,
    pub require_shell_approval: bool,
    pub log_tail_escalation: bool,
    pub route_reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
}
```

### 6.3 `serde` の扱い

- `feature_actions` は `Vec::default()` に落ちる
- 空配列はシリアライズ時に省略できる
- `RoutePlan` 旧フィールドは既存テストの roundtrip を維持する
- `FeatureAction::Unsupported` は将来の未対応 action を no-op として扱うための予約である

`推測`: 実装では、旧 binary が新 `RoutePlan` を読んだときの破損を避けるよりも、まず「`feature_actions` が空なら旧挙動に完全一致する」ことを重視するのが最小リスクである。

## 7. 自動適用と承認必須

### 7.1 自動適用

次は自動適用してよい。

- `SetLogTailBytes`
- `SetRecommendedTools` のうち safe tool のみ
- `MemoryQuery`
- `MemoryRecipeRun { apply: false }`

### 7.2 承認必須

次は承認必須とする。

- shell 実行
- file write / delete
- memory write
- `MemoryRecipeRun { apply: true }`
- tool のうち mutating / external side effect を持つもの
- `ai` を再帰的に起動する action

### 7.3 拒否

次は許可しない。

- `ai` 文字列の再帰実行
- 文字列ベースの汎用スクリプト生成
- feature action からの任意コマンド組み立て

MVP では、`route_turn` からこれらを返す設計自体をしない。

## 8. `feature_executor`

### 8.1 役割

`feature_executor` は `RoutePlan` の `feature_actions` を解釈し、turn の最終入力に反映する内部段階である。smart entry の TTY 経路でのみ動かし、非 TTY fallback では呼ばない。

責務は次のとおり。

- `feature_actions` を正規化する
- safe action を即時適用する
- approval が必要な action を gate に送る
- 実行結果を turn context に反映する
- `agent_turn` 用の最終 request を生成する
- CLI 明示値は最優先で維持し、feature action は explicit `--preset` / `--tools` / `--log-tail` を上書きしない

### 8.2 実行順

1. `route_turn` から `RoutePlan` を受け取る
2. `cli_overrides` と `RoutePlan` を統合する
3. `feature_executor` が `feature_actions` を並べ替える
4. safe action を実行する
5. 必要なら approval gate を開く
6. 承認済み action を実行する
7. `agent_turn` に渡すメッセージ、tools、log tail、memory context を確定する

### 8.3 失敗時

- safe action の失敗は、その action だけを無効化する
- MVP では memory query / recipe run(read-only) の失敗は turn 全体の致命傷にせず、その action を落として stderr に警告し、続行する
- approval 後の side effect 実行に失敗した場合は、turn を中断して理由を返す

## 9. UX

### 9.1 1 行表示

`ai` は route plan を長く表示しすぎない。smart plan の表示は 1 行を基本とする。

例:

```text
ai: smart plan: chat | tools=read_file,grep | log_tail=16KiB | memory=query,recipe:clarify-goal
```

```text
ai: smart plan: tool_assisted | approval=required(shell_exec) | safe=memory_query
```

### 9.2 承認表示

承認が必要な場合は、何を承認するかを action 名で示す。

例:

```text
ai: feature plan requires approval: shell_exec, memory_write
```

### 9.3 CLI 明示値

CLI 明示値がある場合は、route plan より優先することを明示する。

例:

```text
ai: smart plan: overridden by CLI preset=fast
```

`--tools`、`--log-tail`、`--preset` は escape hatch であり、feature plan より優先される。

## 10. 実行フロー

### 10.1 標準フロー

1. `ai` が `route_turn` を呼ぶ
2. AIBE が `RoutePlan` と `feature_actions` を返す
3. `ai` が CLI 明示値を適用する
4. `feature_executor` が safe action を適用する
5. approval gate が必要な action を止める
6. 承認後に action を実行する
7. `agent_turn` を実行する

### 10.2 失敗フロー

- `route_turn` の失敗は既存の retry / fallback を使う
- `feature_executor` の失敗は action 単位で切り分ける
- approval が取れない action は実行しない
- 何も危険なことをせずに 1 shot ask へ落とせるなら、そちらを優先する

## 11. MVP スコープ

MVP で `feature_actions` に含めるのは次だけとする。

- `MemoryQuery`
- `MemoryRecipeRun { apply: false }`
- `SetLogTailBytes`
- `SetRecommendedTools`

MVP ではやらないこと。

- `ai` 再帰呼び出し
- 汎用スクリプト実行
- `MemoryApply`
- `MemoryRecipeRun { apply: true }`
- file write / delete
- shell side effect の自動実行

`route_turn` が v1 で返す `feature_actions` は上の 4 種類だけに限定する。`MemoryApply`、`ShellExec`、`WriteFile`、`RunScript`、`RunAiCommand` などの reserved action は、MVP では `route_turn` から返さないし、`ai` も解釈しない。

また、smart feature plan は smart entry の TTY 経路でのみ有効とし、非 TTY fallback では `route_turn` と `feature_executor` を経由しない。

MVP の意味は、`route_turn` が memory と tool の提案を構造化できることの証明である。副作用の自動実行は第 2 段階でよい。

## 12. 将来拡張

### 12.1 Phase 2 候補

将来は次を追加できる。

- `MemoryApply`
- `MemoryRecipeRun { apply: true }`
- `ShellExec`
- `WriteFile`
- `RunScript`
- `RunAiCommand`

### 12.2 feature registry 化

`推測`: 将来は feature action の trigger を TOML で設定化し、kind / query keyword / route reason から action を組み立てる `feature registry` を AIBE 側に持てる。

ただし本仕様では非目標とし、今は `route_turn` の提案ロジックに閉じる。

## 13. セキュリティ考慮

### 13.1 無限再帰防止

- `ai` 自身を action として呼び戻さない
- `route_turn` の結果に CLI 再帰文字列を含めない
- `feature_executor` は構造化 action だけを受ける

### 13.2 履歴汚染防止

- raw transcript に feature action の内部表現をそのまま混ぜない
- history には redacted summary を残す
- approval 待ちの途中状態は turn の再実行に耐える程度に限定する

### 13.3 承認境界

- read-only は自動
- write / execute は承認
- `never` 相当の拒否ポリシーは最上位で維持する
- CLI 明示値は advisory を上書きできるが、拒否ポリシーは越えない

### 13.4 文字列ベースの危険経路排除

LLM に `ai mem run ...` や `sh -c ...` のような文字列を出力させない。`FeatureAction` は構造化 DTO のみとし、実行層が安全に分岐する。

## 14. テスト方針

| 種別 | 対象 |
|------|------|
| unit | `RoutePlan` / `FeatureAction` serde、`feature_executor` の safe/approval 分岐、CLI override 優先順位、`SetRecommendedTools` の safe 判定 |
| integration | `ai "..."` の smart feature plan、`route_turn` → `feature_executor` → `agent_turn` の流れ、approval gate の経路、non-TTY fallback の退避 |
| aibe integration | `route_turn` が `feature_actions` を返すこと、memory query / recipe proposal の生成 |
| manual | 1 行表示、承認表示、CLI 明示値の上書き表示 |

## 15. 受け入れ条件

- `RoutePlan` に `feature_actions` が追加され、空配列のデフォルトで既存挙動を壊さない
- `route_turn` が `memory_query` と `memory_recipe_run(apply=false)` を構造化して返せる
- `ai` が `feature_executor` を通して safe action を自動適用できる
- 危険な action は承認ゲートを通らない限り実行されない
- `ai` バイナリの再帰呼び出し経路がない
- CLI 明示値が advisory を上書きする
- `./scripts/verify.sh` が通る
- `SetRecommendedTools` は `shell_exec` を自動採用しない
- `feature_executor` は `MemoryRecipeRunRequestBody.user_instruction` を recipe へ渡せる

## 16. 未確定事項

### 推測

- `MemoryQuerySpec` の最終 DTO は、既存 memory query DTO への薄いラッパーにするのが最も実装差分が小さい
- `feature_executor` は `ai/src/application/` か `ai/src/domain/` のどちらかに置けるが、実行順の副作用を持つため application 層に置くのが自然である
- `feature_actions` の将来拡張で `Unsupported` を no-op にするか error にするかは、互換性と厳格性のどちらを優先するかで最終判断が必要である

### 非目標として固定

- `ai` から LLM を直接呼ぶこと
- `ai` 再帰実行の解禁
- 汎用スクリプト実行
- memory write の自動承認
- shell side effect の無条件実行
