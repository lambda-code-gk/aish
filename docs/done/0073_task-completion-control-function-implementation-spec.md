# 0073 Task Completion Control Function 実装指示書

> **種別**: 実装指示書（`docs/done/`）
> **状態**: 実装済み（全 AC green、verify / smoke 成功）
> **正本**: [`../spec/0073_task-completion-control-function-spec.md`](../spec/0073_task-completion-control-function-spec.md)
> **関連**: [`../feature-development-policy.md`](../feature-development-policy.md)、[`../spec/0068_task-completion-phase1-spec.md`](../spec/0068_task-completion-phase1-spec.md)、[`../spec/0071_task-completion-hardening-spec.md`](../spec/0071_task-completion-hardening-spec.md)

## 0. 目的

Task Completion Active turn の初回 Contract transport を assistant text JSON から provider native function calling の server-owned `set_task_contract` へ一本化する。既存 Query Loop の通常 tool round 内で functionCall args を検査し、同一 assistant step の effect/read-only tool より先に Contract を一度だけ固定する。Gemini の text なし functionCall と OpenAI-compatible の tool call は既存共通 `ToolCall` transport を通し、固定後の Completion evaluation は Contract field のない text envelope のまま維持する。

実装内容と挙動が本書と設計書で矛盾する場合は、設計書 0073 を優先する。

### 0.1 Core outcome / Minimum vertical slice

Core outcome:

```text
Task Completion Active turn が assistant text の有無に依存せず、
provider native set_task_contract functionCall で Task Contract を
副作用前に一度だけ提出・固定する。
```

Phase 1 で次の本番 composition を一本通す。

```text
ai ask --task-completion 相当の一 request
→ RequestService が eligibility=Active を固定
→ server が set_task_contract definition を注入
→ Gemini 風 text なし functionCall
→ control args 検査・Contract 固定・成功 tool result
→ write_file
→ read_file
→ Contract field なし text evaluation
→ 既存 Evidence / terminal 判定で Done
```

Contract 専用の Query Loop / provider call は追加しない。既存 Task Completion query budget `2` と Query Loop 内 tool round 上限を増やさない。

### 0.2 Non-goals

- evaluation 専用 control function、Completion evaluation の structured output 全面移行
- 旧 text Contract envelope の互換レイヤ、text / control の二重受理
- Contract 未固定専用 recovery query、text 併記強制、provider retry / tool round 上限緩和
- OpenAI parallel tool calls の provider 固有 scheduler / shortcut
- function calling 非対応 provider の fallback transport
- 新しい永続 store、Conversation schema migration、RPC、CLI flag、設定 toggle
- Evidence / Done / query budget / 0068–0071 / 0072 の意味論変更または再実装
- secondary agent loop、crash recovery、lease、heartbeat、reconciler、journal、idempotency、exactly-once、transaction / rollback
- `ai` / `aish` への provider 分岐または Contract 判定の移動

### 0.3 Pack Composition

**No** — 設計書 §8 の 0045 §6 判定を維持する。`set_task_contract` は optional な機能束ではなく、Task Completion Active 経路の副作用前 Contract 固定という core 安全契約の transport である。新しい Pack trait、Active / Basic Pack、runtime toggle、Cargo feature、別 composition root は追加しない。

## 1. Scope Lock（実装開始時に最初に行う）

現時点の `scripts/feature-scope.toml` は `status = "draft"`、`scope_revision = 1` であり、`scripts/spec-acceptance.toml` に 0073 は未登録である。実装コードを変更する前に、次を一つの Scope Lock 変更として行う。

1. `aibe/tests/0073_task_completion_control_function_red.rs` を追加し、§8 の9個の同名 test function を `#[test]` または `#[tokio::test]` + `#[ignore = "0073 pending implementation"]` で先置きする。`todo!()` / 常時成功 assertion ではなく、期待する観測点を持つ実 RED test にする。
2. `scripts/spec-acceptance.toml` に9件を追加する。各 entry は `spec = "0073"`、同名 `test`、`file_glob = "aibe/tests/0073_task_completion_control_function_red.rs"`、`pending = true` とし、AC と test function を 1:1 にする。
3. `scripts/feature-scope.toml` の 0073 を `status = "locked"`、`scope_revision = 2` にし、`scope_change_reason` を「Step 4 Scope Lock: 9 AC の ignored RED test / acceptance registry を固定（scope 数値不変）」相当に更新する。
4. 同 feature に次の `locked_ac_ids` を設計書順で追加する。

```toml
locked_ac_ids = [
  "task_contract_control_function_vertical_e2e",
  "control_tool_visibility_is_server_owned",
  "control_args_fix_contract_before_effect",
  "mixed_step_control_precedes_effects",
  "missing_invalid_or_duplicate_control_fails_closed",
  "active_text_contract_path_is_rejected",
  "text_evaluation_transport_remains_compatible",
  "provider_function_call_args_are_portable",
  "control_security_and_task_completion_regressions_are_preserved",
]
```

5. `actors=2`、`state_machines=2`、`persistent_aggregates=0`、`external_effects=2`、`process_boundaries=1`、novel mechanism 1件、integration 2件は変更しない。
6. `./scripts/check-feature-scope.py` と `./scripts/check-spec-acceptance.py` を通す。checker を通すために AC 名や scope 数値を設計書から黙って変えない。

実装中に AC を変える場合は `scope_revision` をさらに増やし、設計書 §15 の change log と `scope_change_reason` を同期する。発見事項は `BLOCKER_ORIGINAL_AC` / `REGRESSION` / `SAFETY_WITHIN_FAULT_MODEL` / `NEW_REQUIREMENT` / `HARDENING` / `OUT_OF_FAULT_MODEL` に分類する。

## 2. Phase 分割

| Phase | Vertical slice / hardening | 対象 AC | Gate |
|-------|-----------------------------|---------|------|
| 1 | server-owned definition 注入、functionCall-only decode、control-first 固定、text Contract 固定経路の除去、evaluation-only decoder、write→read→text evaluation→Done の最小縦断 | `task_contract_control_function_vertical_e2e`、`control_tool_visibility_is_server_owned`、`control_args_fix_contract_before_effect`、`mixed_step_control_precedes_effects` | 4 AC が非ignored / `pending=false` で green になるまで Phase 2 に進まない |
| 2 | fail-closed 全相関、text Contract拒否の競合表、evaluation decoder境界、provider portability、sink別 redaction、既存回帰、docs / smoke | 残る5 AC | 全9 AC、targeted、verify、smoke が green の後だけ完了処理 |

**Vertical Slice Gate**: Phase 1 の本番 `RequestService` / `AgentTurnService` / `ToolRoundExecutor` composition を通る E2E が成功する前に、汎用 control framework、provider 固有最適化、追加 integration、永続化、recovery を実装しない。

## 3. 変更対象と責務配置

### 3.1 `aibe` application / domain

| パス | 変更 | 責務 |
|------|------|------|
| `aibe/src/application/completion_envelope.rs` | `ContractGate` を control args 固定 API と step preflight に改修。Active decoder を evaluation-only shape へ変更 | `TaskContract` deserialize / validation、expected kind / 元要求 coverage、一度だけ固定、text Contract field 検出、固定後 evaluation decode。provider / executor は呼ばない |
| `aibe/src/application/tool_defs.rs` | server-owned `set_task_contract` definition builder を追加 | `TaskContract` DTO と一致する共通 JSON Schema。`additionalProperties=false`、required / optional / bounds を一元化。`KNOWN_TOOLS` や client allowlistへ追加しない |
| `aibe/src/application/tool_round/executor.rs` | provider 呼出し前の definition 結合と、executor loop 前の step 全体 preflight / control dispatch / fail-closed 相関結果生成 | control を外部 `ToolExecutor`、client tool gate、approvalへ流さず、成功時だけ同一 step の残りを実行する |
| `aibe/src/application/tool_round/rejected.rs` | 必要なら fixed-shape control error / skip tool result helperを追加 | 元 call ID・元順を保つ bounded provider correlation result。raw args を message に含めない |
| `aibe/src/application/tool_round/mod.rs` | preflight result / helper の module exportだけを必要最小限追加 | 新状態機械や provider別分岐を置かない |
| `aibe/src/application/agent_turn.rs` | tool definitions に server control definition を渡せる内部引数・composition を最小拡張 | client allowlistとserver control公開を分離し、既存 Query Loop / max roundsを維持 |
| `aibe/src/application/request_service.rs` | request開始時 eligibility によるActive構成、reserved name拒否、system instruction更新、固定済みContractからevaluationを処理 | Inactive 0件、Active 1件の公開。text Contractでの初回固定処理を削除し、Contract固定済み/evaluation欠落を区別する |
| `aibe/src/application/task_completion.rs` | `system_instruction` をcontrol function前提に更新 | `set_task_contract`を最初のeffect前に一度だけ呼ぶこと、以後Contractをtextへ含めないこと、evaluation shape、秘密値非格納を指示 |
| `aibe/src/domain/task_completion.rs` | 原則既存型・validatorを再利用。schema生成上不足する公開定数/純粋helperだけ補足 | TaskContract invariantの正本。transportやprovider型を持ち込まない |
| `aibe/src/domain/message.rs` / `aibe/src/domain/llm_step.rs` | 原則変更なし。必要なら共通 `ToolCall` の非空/一意ID検査 helperのみ | Gemini/OpenAI-compatibleの共通call表現、functionCall-only assistantを正常入力として保持 |

`set_task_contract` は論理上 control function であり通常 executor toolではない。`aibe-protocol` の `KNOWN_TOOLS`、CLI tool allowlist、`ToolRegistry`、`ToolExecutionContext`、approval policyへ追加しない。

### 3.2 provider adapter / protocol transcript

| パス | 変更 | 責務 |
|------|------|------|
| `aibe/src/adapters/outbound/gemini.rs` | 原則transport変更なし。必要な最小補正と回帰testのみ | `parametersJsonSchema`、textなし`functionCall`、missing IDの既存synthetic ID、`functionResponse` name/ID相関、part順を維持 |
| `aibe/src/adapters/outbound/openai_compatible.rs` | 原則transport変更なし。必要な最小補正と回帰testのみ | provider IDとJSON argumentsを共通`ToolCall`へ変換し、assistant content `None` / 空を許可 |
| `aibe/src/ports/outbound/llm.rs` | 原則変更なし | `ToolDefinition` / `LlmStepResult` のprovider-neutral portを維持。空content必須化を追加しない |
| `aibe-protocol/src/executed_tool.rs` | control audit recordをwireへ載せる必要が判明した場合だけ固定redaction境界を追加 | raw control argsをresponse `tool_calls`へ出さない。controlはEvidence対象外 |

provider adapter内にTask Completion validationやprovider別Contract schemaを作らない。両providerは同じ`ToolDefinition.parameters`と`ToolCall.arguments`を使い、server-side validationへ到達させる。

### 3.3 テスト / docs

| パス | 変更 |
|------|------|
| `aibe/tests/0073_task_completion_control_function_red.rs`（新規） | 9 ACの1:1受け入れテスト。recording Gemini風/OpenAI-compatible風providerとrecording executorを使用 |
| `aibe/tests/gemini_llm.rs` | functionCall-only、schema送信、synthetic ID、fail-closed tool result再送のadapter補助回帰 |
| `aibe/tests/openai_compatible_llm.rs` | contentなしtool call、arguments/ID、tool result再送のadapter補助回帰 |
| `aibe/tests/0068_task_completion_vertical_e2e.rs`、`0068_task_completion_phase1_red.rs`、`0070_task_completion_phase3_verification_red.rs`、`0071_task_completion_hardening_red.rs` | 旧text Contract fixtureを新control call + evaluation-only fixtureへ更新し、既存意味論の回帰を維持 |
| `aibe/tests/agent_turn_loop.rs`、`aibe/tests/agent_turn_tools.rs`、`aibe/tests/request_tool_validation.rs` | Inactive通常turn、tool loop、reserved name、相関結果の回帰 |
| `docs/architecture.md` | §9 の内容を正本化（詳細は§10） |
| `docs/security.md` | control argsのsink別保持/redaction、bounded error、秘密値非格納を同期 |
| `docs/testing.md` | 0073 recording provider / executor、functionCall-only、相関結果、直列実行方針を同期 |
| `docs/manual/0073_task-completion-control-function.md`（新規） | 実 Geminiでの最終確認手順。実施は完了条件にせず残リスクとして報告 |
| `scripts/feature-scope.toml`、`scripts/spec-acceptance.toml` | Scope Lock、Phaseごとのpending解除、完了時status/revision更新 |

`ai` / `aish` / stdio JSON request schema / CLI flagは変更しない。既存CompletionReport DTOのshapeも変更しない。

## 4. Phase 1 — Minimum Vertical Slice

### 4.1 server-owned control definition / visibility

1. `SET_TASK_CONTRACT: &str = "set_task_contract"` 相当のserver内部定数とdefinition builderをTask Completion application境界へ置く。
2. descriptionに「Task Completion制御専用」「外部副作用なし」「最初のeffect toolより前」「一度だけ」を明記する。
3. function args object自体を`TaskContract`とし、外側の`contract` / `aish_task_completion` wrapperを置かない。
4. `TaskContract`全fieldのJSON SchemaをRust DTOと一致させる。各objectで`additionalProperties=false`、required、array/item、文字列/件数boundを明示し、Gemini/OpenAI-compatible共通subsetに留める。
5. `context.task_completion=true`かつ`classify_task_completion_eligibility`がActiveの場合だけ、通常`definitions_for(allowed)`と`client_tool_definitions`から独立して一件追加する。Inactive、opt-inなし、effect tool不在では0件とする。
6. requestの`client_tools`に同名があればdefinition結合前に`InvalidRequest`とする。現在のnamespace validationだけに偶然依存せず、reserved server nameとして明示的に拒否する。
7. server control definitionは`KNOWN_TOOLS`、client allowlist、通常executor registryへ追加しない。Activeならclientが個別拒否できず、Inactiveならclientが公開できない。

### 4.2 control args validation / Contract固定

1. compact JSON serialized totalをdecode前に測り、`ENVELOPE_MAX_BYTES`と同じ64 KiB以下を必須にする。
2. `serde` unknown/missing/type validation、既存`TaskContract::validate`、stable criterion ID、bounded fieldを同じserver validatorで検査する。
3. request開始時に固定した`TaskCompletionEligibility::Active { expected_kind }`との厳密一致、元ユーザー要求trim後nonempty、既存`validate_contract_covers_request`を検査する。
4. validな初回だけrequest-local gateへtyped `TaskContract`を固定する。raw args bytes/JSON textをgateへ複製しない。
5. 成功結果は固定shape `{"status":"contract_fixed"}` としContract全文を反射しない。approval callback、filesystem/shell/process/network executor、Evidence ledgerへ流さない。
6. schema field一覧、valid、unknown、missing、境界64 KiB / +1 byteをtable testにする。provider schema validationを最終権威にしない。

### 4.3 同一step control-first処理

provider stepを受けた後、外部executorを一件も呼ぶ前に次を行う。

```text
禁止text Contract member検出
→ 全ToolCallのname / ID / occurrenceを収集
→ ID非空・step内一意を検査
→ control件数と固定状態を分類
→ 未固定かつcontrol 1件ならargsを検査して固定
→ control成功時だけ同一stepの残りを元順で既存executorへ渡す
```

control callがeffect/read-only callの後ろに出現しても論理的に先行させる。control成功結果をconversationへ追加し、残りの既存call結果とともに全callを元のassistant stepへ相関させる。Evidenceとclient向け実行記録はcontrolを除く実executor callの順を維持する。control成功後に通常tool validation / approval / executionが失敗してもContractはrollbackしない。

### 4.4 system instruction / evaluation text

`system_instruction(expected_kind)`を次へ変更する。

- 最初のeffect/read-only toolより前に`set_task_contract`を一度だけ呼ぶ
- argsはTaskContractそのもので、textへContractを出さない
- control callはtext併記不要で、functionCall-onlyを許可する
- 固定後の各query最終応答は`aish_task_completion.evaluation`と`deliverable`だけの単一JSON objectにする
- goal / criteria / constraints / deliverables / verificationへAPI key、token、credential、環境変数値、秘密本文を載せない
- Evidence ID / post-observation / delegated verification / gapの既存instructionは意味を変えない

旧「毎回同一Contractをtext envelopeへ反復する」指示は削除する。

この縦断に必要なdecoder変更もPhase 1で行う。Activeの最終text shapeを`aish_task_completion.evaluation` + `deliverable`だけにし、`request_service.rs`の`inspect_before_tools(content, false)`による初回Contract固定と、Contract反復比較を本番経路から除去する。固定後のtoolなし最終応答を同decoderへ通してDoneまで到達させる。旧Contract fieldの全競合表、second query、欠落/error分類の網羅はPhase 2で完成させるが、Phase 1時点で旧text Contractを互換受理してはならない。

### 4.5 Phase 1 E2E

`task_contract_control_function_vertical_e2e`はfake application service直呼びではなく、`RequestService`が組み立てる本番`AgentTurnService` / Query Loop / `ToolRoundExecutor`を通す。recording Gemini風providerは順に次を返す。

1. assistant content空 + valid `set_task_contract` functionCallのみ
2. control成功resultを受信後に`write_file`
3. write成功resultを受信後に`read_file`
4. Contract fieldなしevaluation text + deliverable

assertionはdefinition公開1件、client allowlist非依存、provider round順、control result shape、write/read executor順、post-observation Evidence、Done、Contract専用provider call 0、text Contract 0、Task Completion query budget不変を含む。

Phase 1 targeted gate:

```bash
./scripts/verify-targeted.sh --package aibe --test 0073_task_completion_control_function_red
cargo test -p aibe --test gemini_llm -j 1 -- --test-threads=1
./scripts/check-feature-scope.py
./scripts/check-spec-acceptance.py
```

4 ACがgreenになった同じ変更でだけ対応testの`#[ignore]`を外し、registryを`pending=false`にする。

## 5. Phase 2 — fail-closed / transport / security / regression

### 5.1 決定的preflight優先順位

相関可能なstepでは設計書 §9.3 の順序をそのまま純粋preflight/table testで固定する。

| 順 | 条件 | 代表結果 |
|----|------|----------|
| 1 | text JSONに`aish_task_completion.contract` member | `text_task_contract_forbidden` |
| 2 | control 2件以上 | `duplicate_task_contract_control` |
| 3 | 固定済み + control 1件 | `task_contract_already_fixed` |
| 4 | 未固定 + control 1件 + args/invariant不正 | `invalid_task_contract` |
| 5 | 未固定 + control 0件 + callあり | `task_contract_required` |
| 6 | 上記なし | preflight成功 |

意味上の原因選択より前に全call IDを検査する。空IDまたはstep内重複IDなら相関結果を捏造せずexecutor 0件、provider protocol `InvalidRequest`でrequestを終端する。text violationの検出は保持しても、安全に結果を対応付けられない以上tool resultは作らない。

### 5.2 fail-closed時の全call相関

- 相関可能なstepでは全call occurrenceへ元ID・元順でちょうど1件ずつtool resultを作る。
- controlが1件以上なら最初に出現したcontrolへ原因結果、残りへskip結果を付ける。control 0件なら先頭callへ原因結果、残りへskip結果を付ける。
- 原因shapeは`{"status":"error","code":"<cause>","message":"<bounded>"}`、skip shapeは`{"status":"error","code":"skipped_before_execution","cause":"<cause>"}`に固定する。
- error code語彙は上表5件だけとし、複数違反でも代表原因を一つだけ選ぶ。invalid値、raw args、元要求全文をmessageへ反射しない。
- text+duplicate、duplicate+invalid、duplicate+already-fixed、already-fixed+invalid、control欠落時のeffect/read-only/unknown toolをtable testにする。
- 不正controlは固定成功回数に数えない。通常tool round内の次stepでvalid初回提出を許可する。一度固定後の同一/異なる再提出は常に拒否する。
- Gemini風/OpenAI-compatible風recording provider双方でresult件数、ID、順、次provider round、executor 0件をassertする。

### 5.3 text Contract廃止 / evaluation decoder

1. Phase 1で導入した`aish_task_completion.evaluation` + `deliverable`だけのActive decoderを、final / second query / error tableの全経路で共通利用する。
2. 64 KiB total、`deny_unknown_fields`、bounded deliverable/evaluationの境界値を網羅する。
3. `aish_task_completion.contract` memberは明示的な禁止fieldとしてpreflightで検出する。通常文中の同名文字列だけなら違反扱いしない。Inactiveはdecoderを起動しない。
4. 固定後のtoolなし最終応答とsecond queryを同じActive decoderへ通す。
5. decoder結果は少なくとも「Contract固定済みだがevaluation欠落」とその他schema/errorを区別し、0072 recovery loop / instruction builderの存在を前提にしない。
6. `request_service.rs`やtest helperに旧`inspect_before_tools(content, false)`によるtext Contract固定経路、同一Contract反復比較、Contract付きfixtureが残っていないことを回帰testと検索で確認する。互換受理は残さない。

### 5.4 provider portability

- Gemini: `functionCall` Partのみでassistant textが空でも`LlmStepResult::with_tool_calls("", ...)`として正常に返す。ID省略時は既存turn/part index synthetic IDを用い、thoughtSignature/provider extrasとfunctionResponse nameを維持する。
- OpenAI-compatible: `content=null` / 空 + `tool_calls`を正常に共通`ToolCall`へ変換し、provider call IDとJSON argumentsを保持する。
- 共通層に「assistant content非空」を要求するvalidationを追加しない。
- adapter固有TaskContract validator/schema/dispatchは作らない。両adapterの補助testに加え、AC testで同一server validator到達をassertする。

### 5.5 sink別redaction / bounded data

| sink | 実装条件 |
|------|----------|
| request-local provider message / 後続provider wire | protocol transcript成立のためexact argsをrequest終了まで保持・再送可。通常log/debugへ出さない |
| request-local Contract state | typed `TaskContract`のみ。元args bytes/JSON textの別複製なし |
| log / trace / diagnostic / assertion failure | `{"redacted":true}`相当の固定sentinel、または記録省略 |
| client向け/監査用serialized `tool_calls` | control argumentsを固定sentinelへ置換。provider message serializeを流用しない |
| ConversationStore / cache / journal | raw control args保存禁止。必要ならredacted sentinelまたはcontrol call省略 |
| CompletionReport / Evidence ledger / summary | control success/error/skipをEvidenceにせずraw argsを含めない |
| validation error / 相関result / client error | field/invariant名とbounded固定messageのみ |

secret fixtureをraw control argsへ入れ、許可sinkでは後続provider wireまでexact、禁止sinkでは不在であることをsink別にassertする。request-local transcriptのdebug formattingやrecording providerのpanic messageもlog sinkとして扱い、fixture全文を出さない。

### 5.6 回帰

- 0068: Evidence、post-observation、Done / NeedsUser / Blocked / BudgetExhausted、query budget `2`
- 0069–0070: delegated plan固定、Agent Task結果を未検証扱い、独立verification、follow-up最大1
- 0071: Failed / Cancelled report、Gap、event buffer、sanitizer / bounds
- Task Completion ActiveのHuman Task Suspended / continuation、provider failure / cancellation、max tool rounds
- Inactive通常turn、client-provided tool、approval、streaming、tool result相関
- control successがEvidence / verified / Doneを直接作らないこと

Phase 2 targeted gate:

```bash
./scripts/verify-targeted.sh --package aibe --test 0073_task_completion_control_function_red
cargo test -p aibe --test gemini_llm -j 1 -- --test-threads=1
cargo test -p aibe --test openai_compatible_llm -j 1 -- --test-threads=1
cargo test -p aibe --test 0068_task_completion_vertical_e2e -j 1 -- --test-threads=1
cargo test -p aibe --test 0068_task_completion_phase1_red -j 1 -- --test-threads=1
cargo test -p aibe --test 0070_task_completion_phase3_verification_red -j 1 -- --test-threads=1
cargo test -p aibe --test 0071_task_completion_hardening_red -j 1 -- --test-threads=1
cargo test -p aibe --test agent_turn_loop -j 1 -- --test-threads=1
./scripts/verify-targeted.sh --docs --architecture
./scripts/check-feature-scope.py
./scripts/check-spec-acceptance.py
```

存在しないtest targetを完了手順へ残さない。残る5 ACがgreenになった同じ変更でだけ`#[ignore]`と`pending=true`を解除する。

## 6. Security / compatibility 不変条件

- control args、text envelope、provider call IDはuntrusted inputとして、executor前に件数・ID・size・schema・invariantを検査する。
- invalid value、raw args、元要求全文、secret fixtureをerror、trace、report、Evidence、client responseへ反射しない。
- control functionはapproval、filesystem、shell、process、network、client tool gate、通常ToolExecutorへ流さない。
- server definitionをclient allowlist / schemaで上書きさせない。
- fail-closed stepで一部callだけ実行しない。相関不能なら結果を捏造しない。
- provider wireに必要なrequest-local transcriptと、監査/永続/client表示用serializationのsinkを分離する。
- functionCall-onlyを正常入力とし、text合成や余分なprovider roundで補わない。
- control成功をEvidenceにせず、0068–0071のverified / terminal判定を維持する。
- stdio JSON request/response、CLI、`ai` presenter、`aish`責務を変更しない。

## 7. STOP-THE-LINE / Complexity budget

0073はYellow承認済みで、追加予算は全項目`+0`である。次が必要になった時点で実装を止め、分類・scope revision更新・Complexity Gate再判定を行う。

- 二つ目のcontrol function（evaluation用を含む）
- 新actor、状態機械、agent loop、provider retry / recovery loop
- Contract/control resultの永続化、Conversation schema migration、journal、idempotency key、crash recovery
- 新RPC / socket / CLI flag / config toggle / Pack / Cargo feature
- 旧text Contractとcontrol functionの並行受理
- functionCall-onlyのためのtext合成、再問い合わせ、tool round上限緩和
- provider固有parallel scheduler、OpenAI専用dispatch、Gemini専用Contract schema
- external effects 3、process boundaries 2、integrations 3への増加

STOP-THE-LINE報告:

```text
STOP-THE-LINE

分類: BLOCKER_ORIGINAL_AC / REGRESSION / SAFETY_WITHIN_FAULT_MODEL /
      NEW_REQUIREMENT / HARDENING / OUT_OF_FAULT_MODEL
発見した要因:
現在scopeへの影響:
Complexity Gateの変化:
MVPからの削除案:
別spec案:
```

禁止事項:

- control functionを通常tool registryへ登録してexecutor側で偶然処理する
- provider adapterごとにContract schema / validationを複製する
- fail-closed時に先頭callだけへ結果を返し、残りのIDを欠落させる
- duplicate / already-fixed競合をcall出現順やinvalid args内容で変える
- raw control argsをExecutedToolCall / ConversationStore / traceへそのままserializeする
- ignored test / `pending=true`を残したままPhaseまたは機能を完了扱いにする
- `verify.sh`成功前に本書を`docs/done/`へ移動する

## 8. AC → テスト対応

全entryの`test`はAC IDと同名にし、一つのRust test functionへ1:1で登録する。補助unit/adapter testを追加してもregistry testを置換しない。

| ID | Phase | テスト関数 | file_glob | green条件 |
|----|-------|------------|-----------|-----------|
| `task_contract_control_function_vertical_e2e` | 1 | 同名 | `aibe/tests/0073_task_completion_control_function_red.rs` | 本番compositionでGemini風functionCall-only→control result→write→read→evaluation-only→post-observation Done。専用provider call/text Contract/query増加なし |
| `control_tool_visibility_is_server_owned` | 1 | 同名 | 同上 | Activeのみdefinition 1、Inactive/opt-inなし/noneligible 0、allowlist非依存、同名client toolは結合前InvalidRequest |
| `control_args_fix_contract_before_effect` | 1 | 同名 | 同上 | args直TaskContract、64 KiB、schema/field drift/expected kind/元要求、初回一度固定、approval/effect/Evidence 0、固定success shape、audit redaction |
| `mixed_step_control_precedes_effects` | 1 | 同名 | 同上 | 任意call順でcontrol先行、成功後のみ同step残り実行、全ID相関、control除外Evidence順維持 |
| `missing_invalid_or_duplicate_control_fails_closed` | 2 | 同名 | 同上 | 欠落/invalid/oversize/duplicate/already-fixedと競合表、executor 0、全occurrenceへ1 result、固定shape/code、次step retry、空/重複ID InvalidRequest、両provider風trace |
| `active_text_contract_path_is_rejected` | 2 | 同名 | 同上 | Active text Contract禁止、valid control混在でも固定/executor 0・全ID結果、通常文中文字列とInactiveは非解釈 |
| `text_evaluation_transport_remains_compatible` | 2 | 同名 | 同上 | 固定後final/second queryが同decoder、evaluation+deliverable、64 KiB/unknown拒否、旧Contract shape拒否、evaluation欠落を区別、0072非依存 |
| `provider_function_call_args_are_portable` | 2 | 同名 | 同上 | Gemini functionCall-onlyとOpenAI-compatible tool callが同じToolCall/validatorへ到達し、空contentで拒否されない |
| `control_security_and_task_completion_regressions_are_preserved` | 2 | 同名 | 同上 | 許可sink exact/禁止sink secret不在・redacted、typed stateにraw複製なし、0068–0071 Evidence/Done/budget/delegation/Failed/Cancelled回帰 |

`pending=false`は同名testが本番経路または指定境界を実assertionで検査し、非ignoredで成功した同じ変更でのみ設定する。

## 9. 検証 / smoke / 完了処理

実装中はPhaseごとのtargeted commandを使う。全AC非pending・非ignored後、完了直前に順番どおり実行する。

```bash
./scripts/verify-targeted.sh --package aibe --test 0073_task_completion_control_function_red
./scripts/verify-targeted.sh --docs --architecture
./scripts/check-feature-scope.py
./scripts/check-spec-acceptance.py
./scripts/verify.sh
./scripts/smoke-mock.sh
```

`verify.sh`は一度実行し、失敗時は該当検査だけで修正を回してから最後に再実行する。成功・失敗を問わず`.verify-timing-last`のtiming summaryをユーザー向け完了報告へ転記する。`smoke-mock.sh`は実API / API key / networkを使わない既存mock通常turnの回帰であり、0073のAC E2Eを代替しない。

完了条件:

1. 9 locked ACが同名testでgreen、`pending=false`、`#[ignore]`なし
2. Phase 1 E2Eが本番compositionとfunctionCall-onlyを通る
3. Gemini / OpenAI-compatible、0068–0071、通常tool turnの代表回帰がgreen
4. architecture / security / testing / manualが実装と同期
5. `check-feature-scope.py` / `check-spec-acceptance.py` / `verify.sh` / `smoke-mock.sh`成功
6. 上記後だけ`feature-scope.toml`を`status="done"`へ更新し、`scope_revision`とreasonを完了内容に合わせる
7. 上記後だけ本書を`docs/done/`へ移動し、`docs/0000_spec-index.md`のtasks行削除・done行追加・設計状態を「設計確定（実装済み）」へ更新する

## 10. `docs/architecture.md` 更新箇所

実装と同じ変更で次を正本化する。

1. `Task Completion Loop（0068 Phase 1）`節:
   - Active turnへのserver-owned `set_task_contract`公開条件
   - client allowlist / client-provided toolから独立した所有権とreserved name
   - functionCall argsによるrequest-local Contract固定、外部effect 0、Evidence非対象
   - 同一stepのtext検出→ID検査→control分類/固定→残りtool実行の順序
   - fail-closed原因優先順位、全call correlation result shape、相関不能InvalidRequest
   - text Contract廃止、固定後evaluation text維持、query budget `2`不変
2. `LLMプロバイダ（aibe内）`節:
   - provider-neutral `ToolDefinition.parameters` / `ToolCall.arguments`を正本とすること
   - Gemini functionCall-only / synthetic ID / functionResponse相関
   - OpenAI-compatible contentなしtool call / provider ID相関
   - provider固有Contract validatorを置かないこと
3. security / request-local data説明:
   - provider transcriptではraw args保持可、typed gateへraw複製禁止
   - log / trace / audit tool_calls / ConversationStore / CompletionReport / Evidence / errorではredactまたは省略
4. stdio JSON節:
   - 新RPC、request/response DTO、CLI flag、CompletionReport shapeの変更がないことを明記する。control functionをwire `KNOWN_TOOLS`へ追加しない。

## 11. 手動確認（実 Gemini）

`docs/manual/0073_task-completion-control-function.md`へ、実 Gemini profileをユーザーが設定済みの場合だけ行う任意確認を書く。API keyや本番設定をrepo、fixture、command logへ保存しない。

1. 一時workspaceで`ai ask --task-completion`に小さなwrite + read-back要求を与える。
2. safe diagnosticで初回assistant stepがtextなし`set_task_contract` functionCallであること、definitionが1件であることを確認する。raw args / secretは表示しない。
3. control成功後にwrite、post-observation read、Contractなしevaluationへ進みDoneとなることを確認する。
4. opt-inなしturnではdefinitionが公開されず通常応答が維持されることを確認する。
5. stdout / stderr / logs / CompletionReport / persisted conversationにraw control argsや秘密値がないことを確認する。

実 Gemini確認は外部APIの非決定性とcredentialを伴うためautomated ACや完了ゲートの代替にしない。未実施なら完了報告の「残リスク」に明記する。

## 12. 仕様との差分 / 未確定事項

- 仕様との差分はない。
- **推測:** 9 ACは一つのintegration test fileに集約し、schema/preflightの細粒度testは各application module内、provider wire詳細は既存`gemini_llm.rs` / `openai_compatible_llm.rs`へ補助追加する。実装時にtest fileを分ける場合も、registryの各ACは単一同名functionと1:1を維持する。
- **推測:** control definitionは`application/tool_defs.rs`にbuilderを置くが、通常`definitions_for(allowed)`の入力や`KNOWN_TOOLS`には含めず、Active compositionが別枠で結合する。
- **未確定:** 現行`ExecutedToolCall` / ConversationStoreへのassistant tool-call transcriptの変換境界のうち、どこまでcontrol callが到達するか。raw provider message serializationの流用が必要になる場合は実装を止め、設計書 §12 のsink分離を満たす明示DTO/sanitizerの最小追加を`SAFETY_WITHIN_FAULT_MODEL`として再評価する。
