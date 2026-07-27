# 0073 Task Completion Control Function 設計書

> **種別**: 設計書（`docs/spec/`）
> **状態**: 設計確定（実装済み）
> **起票**: 2026-07-26
> **関連**: [`0068_task-completion-phase1-spec.md`](0068_task-completion-phase1-spec.md)、[`0071_task-completion-hardening-spec.md`](0071_task-completion-hardening-spec.md)、0072 evaluation 欠落 Recovery（別変更）、[`docs/feature-development-policy.md`](../feature-development-policy.md)、[`0045_pack-composition-spec.md`](0045_pack-composition-spec.md)

## 0. Core outcome

Task Completion Active turn が、assistant text の有無に依存せず provider native function calling の専用 `set_task_contract` control function で Task Contract を副作用前に一度だけ提出・固定できる。

## 1. Minimum vertical slice

```text
ai ask --task-completion
→ aibe が request 開始時に eligibility=Active と判定
→ client allowlist と独立に、server が set_task_contract を provider tool list へ注入
→ Gemini 風の assistant step（text なし、set_task_contract functionCall のみ）
→ aibe が functionCall args を TaskContract として検査・固定
→ set_task_contract の成功 tool result を provider へ返す
→ assistant が write_file を呼ぶ
→ assistant が read_file を呼び、副作用後 Evidence を得る
→ assistant が Contract field のない aish_task_completion.evaluation text envelope を返す
→ 既存 Evaluator / Evidence / terminal 判定により Done
```

この縦断経路では Contract 生成だけを目的とする `complete()` / Task Completion query を Query Loop の外へ追加しない。`set_task_contract` は既存 Query Loop の通常 tool round で扱い、Contract 固定後の provider round、Evidence、Done、Task Completion query budget 2、既存 tool round 上限は 0068–0071 の契約を維持する。Gemini の `functionCall` Part だけで assistant text が空の step を正常入力とし、text 併記を要求しない。

### 1.1 One Novelty

本 spec の大きな新規機構は、**Active Task Completion turn へ server 所有の Contract 提出専用 control function を一つ注入すること**だけである。Task Contract の意味、`ContractGate` 相当の検査、Evidence ledger、Completion evaluation、終端、query budget、provider adapter の一般的な function calling transport は既存機構を再利用する。

## 2. Fault model

### 2.1 保証対象

標準 Fault Model に従う。単一ホスト・単一ユーザー・正常な `ai` / `aibe` process 生存中に、次を保証する。

- `set_task_contract` は Active Task Completion turn にだけ公開され、明示 opt-in なしまたは非eligible の Inactive turn では provider tool list に現れない
- Active turn では Contract の初回提出元を `set_task_contract` の functionCall args に限定し、assistant text の Contract を固定経路として受理しない
- control call は schema、`expected_kind`、元要求非空を検査し、成功した Contract を effect tool 実行前に一度だけ固定する
- 同一 assistant step に control call と effect calls が並んでも、control を先に処理し、固定成功時だけ同じ step の effect calls を続行する
- control 欠落、不正、重複、固定後の再提出を fail-closed とし、その step の effect tool を一つも実行しない
- functionCall-only を正常とする Gemini と、既存共通 `ToolCall.arguments` へ変換できる OpenAI-compatible provider の両方で、同じ server-side Contract 検査へ到達する
- control args と結果は既存の bounded / sanitize 契約に従い、CompletionReport やログへ raw args を複製しない

### 2.2 保証対象外

- process crash、socket 切断、OS 再起動後の固定済み Contract の復元
- 複数 process / request / host 間での Contract coordination
- provider が function calling 自体を実装していない、または宣言済み function schema を無視する場合の自動修復
- provider ごとの並列 tool scheduling の性能最適化や exactly-once
- control 成功後に実行した外部副作用の transaction / rollback / 結果不明状態の自動解消
- LLM が生成する Goal / criterion の自然言語上の意味的網羅性（0068 と同じ構造完全性保証に留める）

## 3. Non-goals

- evaluation 専用 control function
- provider structured output への Completion evaluation 全面移行
- 旧 text Contract envelope の互換レイヤまたは二重受理
- Contract 未固定時だけを対象にする専用 recovery query、assistant text 併記の強制、既存 tool round 上限を超える provider retry
- OpenAI parallel tool calls の provider 固有 shortcut / 特殊最適化
- 新しい永続 store、RPC、CLI flag、設定 toggle、schema migration
- Evidence / Done / query budget / 0070–0071 の意味論変更
- 0072 の再実装、または Contract 未固定を 0072 recovery の対象へ広げること
- secondary agent loop、crash recovery、lease、heartbeat、reconciler、journal、exactly-once

## 4. Complexity inventory

| 項目 | 数・内容 |
|------|----------|
| 実行主体 | 2（既存 `ai` client、既存 `aibe` request / Query Loop） |
| 状態機械 | 2（既存 Query Loop、既存 Task Completion lifecycle。新規状態機械なし） |
| 永続 aggregate | 0 |
| 外部副作用 | 2（既存 provider 呼び出し、既存 effect tool 実行。control function 自体は 0） |
| プロセス境界 | 1（既存 `ai` ↔ `aibe`） |
| 新規基盤機構 | 1（server-owned Task Contract control function の注入・preflight） |
| 他機能統合 | 2（0068–0071 Task Completion、既存 provider function calling transport） |

`scripts/feature-scope.toml` の `actors=2`、`state_machines=2`、`persistent_aggregates=0`、`external_effects=2`、`process_boundaries=1`、`novel_mechanisms` 1件、`integrations` 2件と一致させる。

## 5. Complexity Gate

- 判定: **Yellow**
- 理由: 既存 Task Completion lifecycle と Query Loop の二つを横断し、既存 Task Completion と provider function calling の二統合を持つため Yellow 閾値に達する。新規状態機械、永続 aggregate、process boundary、外部副作用は追加しない
- scope review: **approved** — transport 変更を一つの control function と既存 gate の preflight 再配置に限定する
- 分割判断: Contract の tool 公開、args 検査、同一 step の control-first 処理、text Contract 廃止は、functionCall-only vertical slice を成立させる不可分の変更として 0073 に含める。evaluation transport、互換層、provider 固有最適化は Deferred へ分離する
- 承認例外: 不要（Red ではない）

## 6. Complexity budget

| 項目 | 追加可能な上限 |
|------|----------------|
| 実行主体 | +0 |
| 状態機械 | +0 |
| 永続 aggregate | +0 |
| 外部副作用 | +0（control function は process / filesystem / shell / network effect を持たない） |
| プロセス境界 | +0 |
| 新規基盤機構 | +0（`set_task_contract` 以外を追加しない） |
| 他機能統合 | +0 |

既存 `ToolDefinition` / `ToolCall` / tool result transport と request-local `ContractGate` 相当を再利用する。新しい client wire DTO、RPC、provider 呼び出し、CLI option は予算外である。

## 7. Split triggers

実装中に次のいずれかが必要になった時点で STOP-THE-LINE とし、Complexity Gate と `scripts/feature-scope.toml` の revision を再評価する。MVP から除外できなければ別 spec へ分割する。

- evaluation 用を含む二つ目の control function
- 新しい状態機械、agent loop、実行主体、provider retry / recovery loop
- Contract / control result の永続化、crash recovery、migration、journal、idempotency key
- 新 RPC / socket / CLI flag / config toggle
- 旧 text Contract と control function の並行受理
- functionCall-only provider のための text 合成・再問い合わせ
- provider 固有の並列実行 scheduler、OpenAI 専用 dispatch、Gemini 専用 Contract schema
- external effects が 3、process boundaries が 2、integrations が 3 に増える変更

## 8. パック構成の適用

**No** — 0045 §6 の5条件を検討した。既存 Query Loop への横断 preflight には該当するが、`set_task_contract` は optional な機能束ではなく、Task Completion Active 経路の「副作用前に Contract を固定する」という core 安全契約の transport であり、別 basic profile、専用 RPC / CLI / turn hook の束、重い dependency、optional deployment の4条件には該当しない。Inactive 時の非公開は既存 eligibility に従う公開判定で足りる。Active / Basic Pack を追加すると Contract 固定の正本が分岐するため、Pack 境界ではなく既存 Task Completion application/domain 境界に置く。

## 9. Control function 契約

### 9.1 公開と所有権

- 論理 tool 名は **`set_task_contract`** とする。`write_file` / `agent_task` と同じ snake_case 命名に合わせる
- description は「Task Completion 制御専用で、外部副作用を実行せず、最初の effect tool より前に一度だけ呼ぶ」ことを明示する
- `context.task_completion=true` かつ `classify_task_completion_eligibility` が Active の request にだけ、server が provider 向け tool definitions へ追加する
- Inactive、明示 opt-in なし、effect tool 不在による非eligible の request では追加しない。0073 は新しい feature toggle を設けない
- client が指定する `@edit` / `@exec` / literal tool allowlist は effect tool の権限であり、`set_task_contract` の公開可否・実行許可には使わない。Active なら client allowlist と独立に server が追加し、client は個別拒否できない
- `set_task_contract` は server reserved name とし、client-provided tool や通常 executor に同名を委譲して上書きさせない。同名 `client_tools` spec を受けた request は definition を結合する前に `InvalidRequest` とする。client request の公開 tool allowlist、`KNOWN_TOOLS`、新 RPC を増やすための名前ではない
- provider wire 上は既存 tool definitions / function declarations の一件として送信し、応答は既存 `ToolCall` と通常 tool result の相関 ID を使う

### 9.2 args schema と検査

functionCall の `args` object 自体を `TaskContract` とする。`aish_task_completion` や `contract` の外側 wrapper は置かない。schema は Gemini / OpenAI-compatible の共通 function calling subset で表現できる JSON object とし、0068–0071 で定義済みの `TaskContract` field、必須 / default、入れ子、上限、unknown-field policy を正本とする。server-owned definition builder は `additionalProperties=false` を各 object に指定し、required field と optional field を Rust DTO と一致させる。schema drift は field 一覧と valid / unknown / missing field の table test で検出する。provider schema validation は補助であり、server の deserialize / validate を最終権威とする。

control function は次を検査する。

1. args が JSON object で、compact JSON の serialized total が既存 `ENVELOPE_MAX_BYTES` と同じ **64 KiB 以下**であること。その後に schema / bounded field / stable criterion ID / TaskContract 内部不変条件を検査する
2. request 開始時に固定済み eligibility の `expected_kind` との厳密一致
3. gate が保持する元ユーザー要求が非空であること、および既存 `validate_contract_covers_request` 相当の構造検査
4. request-local に Contract が未固定であること。同一 step の複数 control call は全件拒否するが、過去 step の不正な一件は固定成功に数えず、§9.4 の通常 tool round 内で一回目の valid call を再提出できる

成功時だけ request-local Contract を固定し、固定 JSON shape の bounded な成功 tool result（`{"status":"contract_fixed"}`）を返す。成功結果に Contract 全文を反射しない。実行記録へ control args を保持する必要がある場合も `{"redacted":true}` に置換する。失敗時および step 全体の fail-closed 時は、§9.4 に従い **その step の全 functionCall occurrence に対してちょうど1件ずつ** 相関 tool result を返す。control call は approval 対象外で、filesystem / shell / process / network を起動せず、Evidence ledger の Evidence にしない。

### 9.3 同一 assistant step の処理順

tool calls を受けた application 層は、外部副作用を開始する前に step 全体を preflight する。

```text
assistant text が禁止された Contract field を持つ Task Completion envelope でないか検査
→ assistant tool calls を全件分類
→ set_task_contract の件数を検査
→ 未固定なら control call を先に deserialize / validate / fix
→ control 成功時だけ、同じ step の残りの tool calls を既存規則で実行
→ text 違反・control 欠落・不正・重複なら Contract を固定せず、残りを実行せず fail-closed
```

preflight と原因 code の選択順は次で固定する。まず text 違反の有無を記録し、全 call の name / ID を収集して相関可能性を検査し、control 件数と固定状態を分類する。相関可能なら「原因選択」行を上から評価し、最初に一致した code だけを代表原因にする。この表は provider family、call の出現順、Contract 内容によって分岐させない。

| preflight 順 | 種別 | 判定 | step の代表原因 / 終端 | 競合時の例 |
|---------------|------|------|------------------------|------------|
| 1 | 検出 | assistant text JSON に禁止された `aish_task_completion.contract` member があるかを記録する | この時点では state を変更せず、call ID 検査を続ける | text 違反を下位の control 違反より先に確定候補にする |
| 2 | transport gate | call ID が空、または step 内で重複し、安全に全 call を相関できない | provider protocol error の `InvalidRequest`。相関 tool result は作らず request を終端する | どの意味上の違反を併発しても executor は0件。text 違反の検出結果を受理成功へ変えない |
| 3 | 原因選択 1 | 手順1で text 違反を検出した | `text_task_contract_forbidden` | control 重複 / invalid / 固定後再提出 / control 欠落のすべてに優先する |
| 4 | 原因選択 2 | `set_task_contract` が2件以上ある | `duplicate_task_contract_control` | args の一部 / 全部が invalid、または Contract 固定済みでも duplicate を選ぶ |
| 5 | 原因選択 3 | Contract 固定済みで `set_task_contract` が1件ある | `task_contract_already_fixed` | 再提出 args が invalid でも deserialize / validate せず already-fixed を選ぶ |
| 6 | 原因選択 4 | Contract 未固定で `set_task_contract` が1件あり、args / schema / bounded invariant / `expected_kind` / 元要求検査のいずれかが失敗する | `invalid_task_contract` | 複数の validation error は bounded な決定的検査順で最初の一件だけを message にする |
| 7 | 原因選択 5 | Contract 未固定で `set_task_contract` が0件、かつ functionCall が1件以上ある | `task_contract_required` | effect / read-only / unknown tool 自体の validation より先に required を選ぶ |
| 8 | 成功 | 上記に一致しない | control preflight 成功。既存 tool validation / approval / execution へ進む | Contract 固定済みで control のない通常 step を含む |

`text_task_contract_forbidden` は相関可能な step における原因 code の最上位である。transport gate は原因 code の競合規則ではなく、結果相関不能時の終端条件である。たとえば text 違反と重複 ID が同居する場合、step の意味上の違反は先に検出するが、tool result へ安全に載せられないため `InvalidRequest` で終端し、executor は0件とする。

- **assistant text の禁止 Contract field 検査は、control 固定と effect 実行のどちらよりも前**に行う。有効な `set_task_contract`・text Contract・effect call が同一 step に混在していても、text 違反だけで step 全体を fail-closed とし、Contract は固定せず、effect / read-only executor 呼び出しは 0 件とする。ここでいう text Contract は JSON object の `aish_task_completion.contract` member であり、通常文中の同名文字列だけを Contract と誤認しない
- Contract 未固定で effect tool だけが来た場合は、現行どおり `task contract required before tool execution` 相当で副作用前に拒否する
- 同じ step に `set_task_contract` と一つ以上の effect / read-only tool がある場合、provider が並列 functionCall として返した順番にかかわらず control を論理的に先行させる。固定成功後は同じ step の残りを実行してよい
- 同じ step に `set_task_contract` が2件以上あれば、Contract 内容が同一でも全件を失敗とし、effect tool を実行しない
- request 内で Contract 固定後に `set_task_contract` が再度現れた場合、同一 Contract でも二回目として fail-closed とする。異なる Contract の再提出も当然拒否する
- control preflight 成功後の effect tool 自体の validation / approval / execution error は既存挙動に従い、control 成功を rollback しない
- Evidence の実行順・ID は control を除く既存 tool execution record に基づき、0068–0071 の意味を変えない

### 9.4 Fail-closed 時の全 call 相関結果

Gemini / OpenAI-compatible ともに、同一 assistant step の各 functionCall / tool call には **ちょうど1件** の相関結果が必要である。step 全体を fail-closed にするとき（禁止 text Contract、control 欠落、不正 args、同一 step の control 重複、固定後の再提出など）は次を満たす。

- preflight に入る call ID は非空かつ step 内で一意でなければならない。これを満たさず安全に相関できない provider 応答は executor を一切呼ばず、provider protocol error の `InvalidRequest` で request を終端する。相関結果を捏造しない
- 相関可能な step では、全 call occurrence に対して元 ID を保つちょうど1件ずつの tool result を、元 call 順で conversation へ追加する
- 原因 code は `task_contract_required`、`invalid_task_contract`、`duplicate_task_contract_control`、`task_contract_already_fixed`、`text_task_contract_forbidden` の固定語彙とする。原因結果は `{"status":"error","code":"<cause>","message":"<bounded>"}`、skip 結果は `{"status":"error","code":"skipped_before_execution","cause":"<cause>"}` とし、provider ごとの差分を作らない
- 複数の protocol 違反が同じ step にある場合の `<cause>` は §9.3 の優先順位表だけで決める。下位違反ごとの追加原因結果は作らず、残りの call は選択済み原因を参照する skip 結果とする
- **control call が1件以上ある場合**: 最初に出現した control call に上記の原因結果を載せ、他の control / 未実行 effect・read-only call には同じ原因 code を参照する skip 結果を返す
- **control call が0件の場合**（未固定で effect のみなど）: その step の先頭 call に固定 error code と原因メッセージ（例: `task_contract_required`）を載せ、残りの各 call には同じ error code を参照する `skipped_before_execution` 付き bounded error を返す。代表 control を捏造しない
- 結果件数と call 件数の不一致、一部 call だけ結果なし、は禁止する
- どの結果にも raw control args、秘密値、元要求全文を載せない
- rejected / skipped record は control success と同様に Evidence ledger、CompletionReport の tool Evidence、外部 executor 件数へ含めない
- 相関結果を追加した後は既存 Query Loop の通常 tool round として provider を継続できる。不正な試行で Contract は固定されず、次 step の valid な一回目は受理できるが、専用 recovery query、query budget 増加、tool round 上限の緩和は行わない。固定後の再提出を受理可能に戻すことはない
- call が0件で text だけが禁止 Contract を含む場合は、既存の最終応答 fail-closed（`InvalidRequest` 相当）とし、捏造 tool result や追加 provider round は作らない

## 10. Text envelope と system instruction

### 10.1 Contract の text 経路廃止

Active Task Completion turn では、assistant text JSON の `aish_task_completion.contract` member を初回固定にも再提出にも使用しない。Contract member を持つ envelope は、二重経路を残さないため Active control protocol 違反として拒否する。Inactive turn は従来どおり Task Completion control text を解釈しない。

固定済み Contract を server が continuation context へ提示することは、モデルから Contract を提出する transport ではないため許可する。ただし continuation instruction も Contract の再提出を要求せず、再度 `set_task_contract` を呼ばせない。

### 10.2 evaluation の維持

Phase 1 の最終 assistant 応答は tool call を伴わない text が正常であるため、`aish_task_completion.evaluation` と `deliverable` の text transport を維持する。ただし envelope schema は本 spec で `contract` を禁止する次の形へ置換する。Active decoder は固定済み Contract を server state から参照して evaluation を検査し、unknown field を拒否する。旧 Contract 付き shape との二重受理は行わない。evaluation 専用 control function と structured output 移行は Deferred とする。

```json
{
  "aish_task_completion": { "evaluation": { "criteria": [] } },
  "deliverable": "..."
}
```

`evaluation` は Active final response で必須とする。`aish_task_completion` object と top-level は `deny_unknown_fields` 相当、`deliverable` の既存上限、envelope total 64 KiB、`CompletionEvaluation::validate_bounded` を維持する。正本の `deliverable` は top-level sibling とするが、一部 provider が `aish_task_completion.deliverable` へネストした場合は decode 時に top-level へ正規化して受理する（top-level 非空を優先）。`contract` やその他の unknown field は引き続き拒否する。Contract 固定後の continuation / second query も同じ shape だけを受理する。

0073 が固定する互換境界は、Contract なし final shape を読む単一の Active decoder と、その decoder が返す「evaluation 欠落」と「その他の decode / validation error」を区別できる結果型までとする。second query を含む通常 final response は必ずこの decoder を通す。将来 0072 evaluation 欠落 Recovery を統合する場合、Recovery instruction builder は decoder の「Contract 固定済みかつ evaluation 欠落」という結果だけを入力にし、同じ Contract なし final shape の text 再提出だけを要求しなければならない。text Contract、`set_task_contract` の再呼び出し、Contract 未固定の recovery は要求してはならない。**0072 の recovery loop / instruction builder 自体と、その統合試験は本 spec の AC ではなく Deferred** とし、0073 は 0072 未マージでも成立する。

### 10.3 system instruction 更新方針

Active Task Completion system instruction は、現行の「最初の assistant JSON text に Contract を載せ、後続 envelope でも反復する」という指示を廃止し、次を明示する。

- 最初の effect tool より前に `set_task_contract` を一度だけ呼ぶ
- functionCall-only でよく、Contract 提出時の assistant text は不要
- provider が同一 step に複数 functionCall を返せる場合、`set_task_contract` と effect calls を同じ step に含めてもよい
- Contract 固定後は `set_task_contract` を再度呼ばず、Contract を text envelope に含めない
- 最終応答は Contract field のない evaluation + deliverable text envelope とする
- Evidence ID、post-observation、delegated verification、query 2 の規則は既存どおりとする

pre-tool recovery で text 併記を強制する指示は追加しない。

## 11. Provider transport

Gemini adapter は candidate の `functionCall` Part を、text Part がなくても `assistant.content=""` と `ToolCall` に変換できる既存経路を使用する。`set_task_contract` args は他 tool と同じ `functionCall.args` から取得し、provider 固有の envelope を作らない。

OpenAI-compatible adapter も既存の tool/function call arguments を共通 `ToolCall.arguments` へ変換した後、同じ control preflight へ渡す。provider が複数 call を一 step に返すか、control 成功後の次 round で effect call を返すかはどちらも正常とする。OpenAI 固有の parallel-tool ordering optimization は行わない。

provider 宣言用 schema の `maxLength` は JSON Schema 定義どおり文字数、server 側 bound は UTF-8 byte 長である。multibyte Contract は schema を通過しても server が fail-closed で拒否し得るが、正本は server 側 validation とし schema 側は補助的ヒントに留める。

両 provider family で、provider 宣言用 schema は互換性のある共通 subset、server-side `TaskContract` validation は同一正本とする。空 assistant content を異常扱いする provider-neutral validation を追加しない。Gemini が ID を省略した場合は既存の turn / part index 由来 synthetic ID を使い、OpenAI-compatible は provider ID を使う。いずれも §9.4 の非空・step 内一意性を preflight で検査する。

## 12. セキュリティと観測

- system instruction / description で、Contract の goal、criteria、constraints、deliverables、verification に API key、token、credential、環境変数値、秘密本文を載せないよう要求する
- validation error は field / invariant を特定できる bounded message にし、args 全文や元要求全文を反射しない
- provider HTTP body を扱う既存 sanitizer、tool output 上限、streaming buffer、CompletionReport の bounded / sanitize 契約を維持する
- `set_task_contract` は外部 effect executor、client-provided tool、approval callbackへ流さない
- control 成功は Evidence ではなく、Done / verified を直接成立させない。Evidence / evaluation の安全不変条件は 0068–0071 を維持する

raw control args の保持可否は、`ToolCall.arguments` という共通型名ではなく、次の **sink** で区分する。同じ request-local message を provider protocol のために再送するときと、監査用 `serialized tool_calls` として記録するときは別 sink である。

| sink | raw control args | 必須の扱い |
|------|------------------|------------|
| request-local provider wire / message | **許可** | provider 応答の一時 decode buffer、request-local assistant message 内の共通 `ToolCall.arguments`、相関 tool result 後を含む current request 内の後続 provider request body に限り、protocol transcript を成立させるため exact args を保持・再送してよい。current request 終了までに破棄し、通常ログへ body を複製しない |
| request-local Contract state | **raw 文字列は不許可** | validation 成功後の typed `TaskContract` として意味処理に必要な field 値だけを request 終了まで保持できる。元の args bytes / JSON text を別フィールドへ複製しない |
| ログ / trace / command line / diagnostic dump | **不許可** | control args 部分を `{"redacted":true}` または同等の固定 sentinel に置換する |
| 監査・実行記録用の serialized `tool_calls` | **不許可** | provider wire/message の直列化を流用せず、control call の `arguments` を固定 redaction sentinel に置換する |
| 永続 ConversationStore / cache / journal | **不許可** | raw args を保存しない。保存対象に control call を含める場合は arguments を固定 redaction sentinel に置換する |
| CompletionReport / Evidence ledger / Evidence summary | **不許可** | control success / rejected / skipped 自体を Evidence にせず、raw args も載せない |
| max-round terminator への入力会話 | **不許可** | terminator の生成結果はそのまま client 応答本文になるため、`ConversationReplay` / `SummaryPrompt` いずれへ渡す会話でも control call の `arguments` を固定 redaction sentinel に置換する。`provider_extras` は adapter が解釈する provider 固有 part であり sentinel を入れると誤読され得るため、置換ではなく省略する。call 自体は相関のため残す |
| validation error / provider 相関 tool result / client error | **不許可** | field / invariant 名と bounded 固定 message のみを返し、invalid value、args 全文、元要求全文を反射しない |

許可 sink から redact 必須 sink への変換は明示的な sanitizer / audit DTO 境界で行う。request-local provider message の debug formatting、error wrapping、recording provider の assertion failure もログ sink とみなし raw args を出さない。

## 13. Acceptance Criteria

| ID | 条件 |
|----|------|
| `task_contract_control_function_vertical_e2e` | deterministic Gemini 風 mock provider の縦断試験で、Active turn が text なしの `set_task_contract` functionCall → 成功 tool result → `write_file` → `read_file` → Contract なし text evaluation の順に進み、post-observation Evidence を伴う Done に到達する。Query Loop 外の Contract 専用 provider call、text Contract、Task Completion query budget 増加は発生しない |
| `control_tool_visibility_is_server_owned` | `task_completion=true` かつ eligibility Active のときだけ `set_task_contract` definition が client allowlist と独立に一件追加され、明示 opt-in なしまたは非eligible の Inactive では0件となる。同名 client tool は definition 結合前に InvalidRequest となり、server 定義を上書きできない |
| `control_args_fix_contract_before_effect` | functionCall args を直接 `TaskContract` とし、64 KiB total、schema / required / unknown field、bounded field、stable ID、`expected_kind`、元要求非空を検査する。definition schema と Rust DTO の field drift を table test で検出し、valid な一回目だけを最初の effect tool より前に固定する。control 自体は approval、外部副作用、Evidence を発生させず、成功結果は固定 shape、audit args は redacted となる |
| `mixed_step_control_precedes_effects` | 一つの assistant step に `set_task_contract` と複数 effect/read-only calls が任意順で含まれても、control 固定成功後にだけ残りを同じ step で実行でき、既存 call correlation と control を除く Evidence 順を維持する |
| `missing_invalid_or_duplicate_control_fails_closed` | 未固定で effect call のみ、不正 / 64 KiB 超過 args、同一 step の control 2件以上、固定後の同一または異なる Contract 再提出の各 case で、該当 step の effect / read-only executor 呼び出しが0件となり、その step の全 call occurrence に元順・元IDでちょうど1件ずつ相関 tool result を返す。さらに text+duplicate、duplicate+invalid、duplicate+already-fixed、already-fixed+invalid の競合 case と、control 欠落時の effect / read-only / unknown tool case を含め、§9.3 の優先順位どおり代表 code がそれぞれ `text_task_contract_forbidden`、`duplicate_task_contract_control`、`duplicate_task_contract_control`、`task_contract_already_fixed`、`task_contract_required` へ一意に決まることを検査する。原因 / skip JSON shape と固定 code 語彙、通常 tool round 内の次 step で未固定の valid 初回提出だけを受理すること、固定後再提出を受理しないことも検査する。空 / 重複 call ID は結果を捏造せず InvalidRequest。Gemini 風および OpenAI-compatible 風の recording provider の両方で件数・ID・provider round・executor 0件を確認する |
| `active_text_contract_path_is_rejected` | Active turn の assistant text JSON に `aish_task_completion.contract` member があっても Contract を固定せず protocol error とし、control function との二重受理が存在しない。有効な `set_task_contract`・text Contract・effect call が同一 step に混在しても、text 検査が先行し executor 呼び出しは0件、全 call ID に相関結果が付く。通常文中の同名文字列と Inactive turn は Contract control として解釈しない |
| `text_evaluation_transport_remains_compatible` | 固定後の tool なし最終応答と second query が単一の Active decoder を使い、Contract field を禁止した `aish_task_completion.evaluation` + deliverable shape、64 KiB total、unknown-field 拒否として検査できる。旧 Contract 付き shape は拒否し、decoder 結果が「Contract 固定済みかつ evaluation 欠落」とその他の error を区別する。0072 recovery loop / instruction builder の存在はこの AC の前提にしない |
| `provider_function_call_args_are_portable` | Gemini の functionCall-only response と OpenAI-compatible の tool/function call response が、ともに同じ `ToolCall.arguments` / server-side TaskContract validation へ到達し、空 assistant content を理由に拒否されない |
| `control_security_and_task_completion_regressions_are_preserved` | secret fixture を含む raw control args が current request の provider assistant message の共通 `ToolCall.arguments` と、protocol transcript を再送する後続 provider wire で exact 値のまま保持でき、request 終了後は保持されないことを検査する。typed request-local `TaskContract` には意味処理に必要な field 値だけがあり、元 args bytes / JSON text の複製がないことも確認する。ログ / trace / diagnostic、監査・実行記録用 serialized `tool_calls`、ConversationStore、CompletionReport、Evidence ledger / summary、max-round terminator 入力会話、validation error、相関 tool result、client error の各 sink では raw / secret fixture が不在で control arguments が固定 sentinel に redact されるか記録自体が省略されることを sink 別に検査する。MaxToolRounds + `ConversationReplay` に到達し terminator が入力を応答へ引用しても client 応答へ secret fixture が出ないことを含む。既存 sanitizer / bound が働き、0068–0071 の Evidence、Done、query budget、delegated verification、Failed / Cancelled の代表回帰試験が不変である |

### 13.1 AC の検証方針

各 ID は実装開始時に `scripts/spec-acceptance.toml` の単一同名 Rust test function と 1:1 に Scope Lock する。現 Step 1 では registry へ pending case を追加せず、`scripts/feature-scope.toml` の status は `draft` のままとする。vertical slice は `task_contract_control_function_vertical_e2e` である。

外部 API、実 API key、wall-clock sleep、手動 text 併記には依存せず、recording provider / executor により tool definitions、assistant content、call 順、effect executor 呼び出し件数、provider round/query 数、report/log を観測する。実 Gemini の最終手動確認は実装指示書で別途定義し、automated AC の代替にはしない。

## 14. Deferred specs

- Completion evaluation 専用 control function
- provider structured output への evaluation / deliverable 移行
- 旧 text Contract envelope の互換レイヤ（本 spec では意図的に採用しない）
- OpenAI parallel tool calls の差分を利用する provider 固有最適化
- function calling 非対応 provider の fallback transport
- Contract / evaluation の永続化、crash recovery、cross-command resume
- 専用 pre-tool retry / recovery query（text 併記強制、tool round 上限緩和を含む）
- 0072 evaluation 欠落 Recovery の loop / instruction builder と、0073 decoder との統合試験

## 15. Scope change log

| Revision | 分類 | 変更 | 理由 |
|----------|------|------|------|
| 1 | INITIAL | Active Task Completion の初回 Contract transport を text JSON envelope から server-owned `set_task_contract` functionCall args へ一本化。Contract なし evaluation shape、優先順位付き control preflight、全 call 相関 error、64 KiB args total、reserved name / sink 別 redaction を固定し、Evidence、Done、query budget は維持。0072 統合試験は Deferred | Gemini 公式 function calling で正常な functionCall-only 応答を受理し、副作用前 Contract 固定と provider 会話の相関を native transport で保証するため |

## 16. `docs/architecture.md` への影響

実装時に Task Completion Loop 節を更新し、Active turn の server-owned control tool 公開、functionCall args による Contract 固定、同一 step の control-first preflight、text Contract 廃止、evaluation text 維持を正本化する。provider 節には Gemini の functionCall-only と OpenAI-compatible の共通 `ToolCall.arguments` 経路を同期する。新 RPC / client wire schema / CLI flag は追加しないため、stdio JSON request schema は変更しない。

## 17. 未確定事項

- なし。tool 名、公開範囲、同一 step 順序、重複時 fail-closed、text Contract 廃止、evaluation text 維持は本 spec で固定済みである
