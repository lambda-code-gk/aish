# 0068 Task Completion Phase 1 実装指示書

> **種別**: 実装指示書（`docs/done/`）  
> **状態**: 実装済み  
> **正本**: [`0068_task-completion-phase1-spec.md`](../spec/0068_task-completion-phase1-spec.md)  
> **関連**: [`feature-development-policy.md`](../feature-development-policy.md)、[`architecture.md`](../architecture.md)、[`security.md`](../security.md)

## 0. 目的

設計書 0068 を正本として、一つの `ai` command / 一つの aibe request の生存中に、既存 Query Loop を最大2回直列実行する Task Completion Loop を実装する。初回 tool 実行前に Task Contract を固定し、tool execution record と副作用後の再観測を Evidence ledger に集約し、各 Query Loop の assistant envelope に含まれる Completion Evaluator の意味判定をコードで fail-closed 検査する。未達時は元要求を再送せず gap-driven continuation を行い、Done / NeedsUser / Blocked / BudgetExhausted のいずれかと Evidence・未検証事項を返す。

既存 Query Loop の provider round、tool round、approval、timeout、streaming、`max_tool_rounds` は置換しない。Completion Evaluator 専用の provider 呼び出し、別 conversation、別 agent loop は作らない。

## 0.1 Scope Lock

- Feature scope registry: `scripts/feature-scope.toml`
- Status: `done`
- Scope revision: `3`
- Complexity class: Yellow（`scope_review = "approved"`）
- Vertical slice AC ID: `task_completion_vertical_e2e`
- Query budget: 固定値 `2`（config / env を追加しない）
- Locked AC IDs:
  - `task_completion_vertical_e2e`
  - `task_contract_is_stable_and_complete`
  - `assistant_claim_is_not_verified_evidence`
  - `side_effect_requires_post_observation`
  - `completion_evaluator_is_structured_and_fail_closed`
  - `continuation_is_gap_driven_and_detects_plan_only`
  - `terminal_outcomes_are_distinct`
  - `progress_and_stall_are_bounded`
  - `final_report_lists_evidence_and_unverified_items`

Scope Lock 後の指摘は `BLOCKER_ORIGINAL_AC` / `REGRESSION` / `SAFETY_WITHIN_FAULT_MODEL` のみ0068をブロックできる。`NEW_REQUIREMENT` / `HARDENING` / `OUT_OF_FAULT_MODEL` は本書へ黙って追加せず Deferred または別 spec とする。

## 1. Phase 分割

| Phase | 内容 | 対応 AC | ゲート |
|-------|------|---------|--------|
| 1 | Domain invariant、assistant envelope、初回 tool 前 Contract gate、Evidence ledger、既存 Query Loop 2回直列再利用、gap-driven Done の最小縦断 | `task_completion_vertical_e2e`、`task_contract_is_stable_and_complete`、`side_effect_requires_post_observation`、`completion_evaluator_is_structured_and_fail_closed`、`continuation_is_gap_driven_and_detects_plan_only` | Vertical Slice AC を含む5件を実テストへ置換して緑にし、各 `pending=false` / `#[ignore]` 解除後のみ Phase 2へ進む |
| 2 | 自己申告排除、4終端、budget / stall、全終端report、sanitize、既存経路regression | `assistant_claim_is_not_verified_evidence`、`terminal_outcomes_are_distinct`、`progress_and_stall_are_bounded`、`final_report_lists_evidence_and_unverified_items` | 残る4件を緑にし、0068の pending を全解除する |
| 3 | docs / protocol 正本同期、必要なsmoke、全体検証、完了処理 | 新規 AC は追加しない | `./scripts/verify.sh` 成功後のみ `docs/done/` へ移動する |

**Vertical Slice Gate**: Phase 1 の `task_completion_vertical_e2e` が成功する前に、Phase 2 hardening、永続化、設定化、汎用 workflow 化、Human Task 統合、並列化へ進まない。Phase 1 E2E は一つの `ai` command が一つの request を送り、その request 内で既存 Query Loop が2回動く本番 composition を通す。Task Completion Loop をテスト専用facadeだけで成立させない。

## 2. 変更対象と責務配置

以下は現行コード調査に基づく具体的な配置である。既存名称との整合でファイルを統合する場合も、依存方向と責務は変えない。

### 2.1 `aibe` domain

| パス | 変更 | 責務 |
|------|------|------|
| `aibe/src/domain/task_completion.rs`（新規） | Task Contract、criterion ID、Evidence、evaluation、progress fingerprint、4終端を定義 | 純粋なschema invariant、criterion集合不変、Evidence参照・provenance・effect順序検査、終端判定順、query budget `2` |
| `aibe/src/domain/mod.rs` | 新domain moduleを公開 | applicationからdomain型を利用可能にする |
| `aibe/src/domain/tool_execution_summary.rs` / `aibe/src/domain/tool.rs` | 必要最小限の既存metadataを再利用または補足 | tool名、status、risk/read-only区分、実行順をEvidence候補へ変換可能にする。Task Completion固有判定をtool domainへ逆流させない |

Domain の最低型:

- `TaskContract`: Goal / Completion Criteria（安定した一意ID）/ Constraints / Deliverables / Verification
- `EvidenceRecord`: `evidence_id`、`criterion_ids`、`source`（tool / observation / verification / deliverable）、`observed_after_effect`、bounded `summary`、`verified`
- `CompletionEvaluation`: criterionごとの satisfied / unsatisfied、required evidence、Evidence参照、単一 `next_objective`、NeedsUser / Blocked 候補理由
- `CompletionOutcome`: Done / NeedsUser / Blocked / BudgetExhausted
- `CompletionReport`: outcome、criterion別Evidence、unsatisfied criteria、未検証事項、query使用数
- `ProgressSnapshot`: unsatisfied集合、Evidence fingerprint、正規化failure

`verified=true` は「コードで検査可能なprovenance・tool status・順序が妥当で、Evaluatorがcriterionとの意味的充足を認めた」の積である。assistantの終了文や自己申告だけでtrueにしない。`source=deliverable` はContractがassistant生成本文そのものをDeliverableとしたcriterionだけに許可する。

### 2.2 `aibe` application / internal boundary

| パス | 変更 | 責務 |
|------|------|------|
| `aibe/src/application/task_completion.rs`（新規） | Task Completion Loopを実装 | request-localなContract / Evidence ledger / 評価履歴 / query使用数を所有し、既存 Query Loop を最大2回直列起動、gap prompt生成、進展・停滞・終端を制御 |
| `aibe/src/application/completion_envelope.rs`（新規） | assistant envelope codec / validator | 初回Contract envelopeと各query最終evaluation envelopeをdecodeし、domain validationへ渡す。schema不正はfail-closed。providerは呼ばない |
| `aibe/src/application/agent_turn.rs` | 既存 Query Loop の内部結果をTask Completionへ返せるよう最小拡張 | provider / tool round責務は維持し、assistant step、executed tool records、statusを含むrequest-local `QueryLoopResult` を返す内部入口を設ける。従来public wrapperの挙動はregression testで維持 |
| `aibe/src/application/tool_round/executor.rs` | 最初のassistant stepをtool実行前gateへ通知し、query traceを返す | 初回tool callを実行する直前にContract envelopeの固定成功を必須化。不正・欠落・2回目以降のContract改変ならtoolを実行せずfail-closed |
| `aibe/src/application/tool_round/mod.rs` | `RoundOutcome` / internal traceを必要最小限拡張 | 全assistant stepと累積tool recordsをTask Completionへ観測可能にする。raw outputの無制限複製はしない |
| `aibe/src/application/request_service.rs` | 通常AgentTurn requestのcompositionをTask Completionへ接続 | 一つのrequest内でTask Completion Serviceを起動する。route / memory / Human Task lifecycle等の他request種へ波及させない |
| `aibe/src/application/server.rs` | 既存composition rootで純粋なcodec / serviceを配線 | pack、runtime toggle、別composition rootを追加しない |

初回 provider step の処理順は必ず次とする。

```text
provider assistant step受信
→ Contract envelope decode / schema・ID検査
→ command-local Contractを一度だけ固定
→ 固定成功を確認
→ 同じstepに含まれる既存tool callをToolRoundExecutorが実行
```

Contractが欠落・不正なままtool callが存在する場合、tool callを実行してからエラーにしてはならない。Contract生成だけのQuery Loop / provider callも開始しない。toolなしで初回応答が完結する場合、同じassistant stepが初回Contractと最終evaluationを兼ねられるschemaにする。

Completion Evaluator は独立port / adapter / providerではない。LLMの意味判定は各 Query Loop の最終assistant envelope内に既に含まれ、`completion_envelope.rs` とdomain validatorが構造・集合・参照・終端不変条件を検査する。テスト差し替えが必要なら純粋なcodec / validator traitを内部境界として注入してよいが、新しいexternal effect portにはしない。

### 2.3 `aibe-protocol`

| パス | 変更 | 責務 |
|------|------|------|
| `aibe-protocol/src/response.rs` | Task Completion最終report DTOと4 outcomeを追加し、`AgentTurnResult`から返す | human / structured両表示が同じ正本DTOを使えるようにする。既存fieldの意味を破壊しない |
| `aibe-protocol/src/lib.rs` | 新DTOをre-export | `ai` / testsからleaf DTOのみ参照 |
| `aibe-protocol/src/executed_tool.rs` | 既存metadataでEvidence provenanceが不足する場合のみ拡張 | secretsやraw全文を新たにwireへ載せない |

推奨は `AgentTurnResult` に後方互換なoptional `completion_report` を追加し、通常responseのassistant本文・tool summaryとTask Completion reportを分離すること。ただし実装前に既存serde fixtureと全pattern matchへの影響を確認する。別variantが既存clientのfail-closed性を高めると判明した場合は、設計書と `docs/architecture.md` のwire schemaを先に同期して選択理由を記録する。

assistant envelopeは内部制御情報であり、raw JSONやEvidence raw outputを `AssistantStreaming` としてユーザーへ漏らさない。既存streaming protocolを再実装せず、envelopeを確定・検査するまでcontrol payloadをbufferし、ユーザー向けDeliverable本文だけを既存streaming / final responseへ流す。buffer境界で既存cancel / timeoutを無効化しない。

### 2.4 `ai` client

| パス | 変更 | 責務 |
|------|------|------|
| `ai/src/adapters/outbound/stdout_presenter.rs` | CompletionReportのhuman / JSON / TSV等の既存structured mode表示を追加 | outcome、criterion別Evidence source / verified、unsatisfied、未検証、query使用数をbounded / sanitized表示。assistant自己申告を観測済み表示しない |
| `ai/src/ports/outbound/presenter.rs` | 必要なら既存 `show_response` のままDTOを解釈 | Task Completion domain logicをclientへ移さない |
| `ai/src/application/ask.rs` | 既存一request送信を維持し、新reportをpresenterへ渡す | client側でQuery Loopを回さない。LLM APIを直接呼ばない |
| `ai/src/adapters/outbound/aibe_client.rs` | wire DTO追加に伴うdecode fixture更新のみ | 新socket / RPC / retryを追加しない |

`aish` は変更しない。シェル起動・実行・ログ記録という既存責務の外にLLM、Contract、Evidence評価を持ち込まない。

## 3. Phase 1 実装手順 — Minimum Vertical Slice

1. `aibe/tests/0068_task_completion_phase1_red.rs` のPhase 1対象4件と `ai/tests/0068_task_completion_phase1_red.rs::task_completion_vertical_e2e` を、`todo!()` ではなく観測可能な失敗assertionを持つRED testsへ置換する。まだ `#[ignore]` / `pending=true` は維持する。
2. Domain型とtable testsを追加する。criterion IDの一意性・完全性、未知/欠落ID、存在しないEvidence参照、矛盾終端、effect前観測、自己申告Evidenceを拒否する。
3. assistant envelope schema / codecを実装する。初回stepはContract、最終stepはevaluationを必須とし、初回toolなし応答は両方を同じenvelopeに持てるようにする。
4. `ToolRoundExecutor` に副作用前Contract gateを設ける。deterministic fake LLM + recording fake toolで「Contract固定がtool実行より先」「不正Contract時はtool呼び出し0」を検証する。
5. 既存AgentTurnServiceから、Task Completionが一つのQuery Loopのassistant steps・tool records・statusを受け取れる内部結果型を返す。既存 `AgentTurnStatus::Ok` はQuery正常終了であってDoneではない。
6. request-local Task Completion Serviceを実装し、固定budget `2` で既存 Query Loopを直列再利用する。1回目未達時はContract、unsatisfied criteria、required evidence、単一 `next_objective`だけからcontinuationを構築し、元要求をそのまま再送しない。
7. tool execution recordをEvidence候補へ変換する。write-like成功はeffect事実に留め、その後のread-only observation / verificationが関連付くまでcriterionを満たさない。
8. protocol report DTOとpresenterのDone最小表示を接続し、mock socket / scripted provider / recording toolsを通る `task_completion_vertical_e2e` を緑にする。fake application service直呼びだけでE2Eを代替しない。
9. Phase 1の5テストから `#[ignore]` を外し、同じ変更で `scripts/spec-acceptance.toml` の対応5件を `pending=false` にする。`task_completion_vertical_e2e` が緑になる前にPhase 2へ進まない。

Phase 1 の targeted gate:

```bash
cargo test -p aibe --test 0068_task_completion_phase1_red -j 1 -- --test-threads=1
cargo test -p ai --test 0068_task_completion_phase1_red task_completion_vertical_e2e -j 1 -- --test-threads=1
./scripts/check-spec-acceptance.py
./scripts/check-feature-scope.py
```

## 4. Phase 2 実装手順 — 終端・停滞・表示

1. 残る4 ignored testを実assertionへ置換する。
2. 各評価後に unsatisfied ID集合、canonicalなEvidence fingerprint、正規化failureを保存する。新Evidenceまたはcriterion改善を進展とし、Evidence不変または同一failureの2回目をstallとして記録する。fingerprintへraw secretを入れない。
3. 設計書 §9.1 の順序をdomain table testで固定する。
   1. 全criterionにvalid verified Evidenceがある場合のみDone
   2. 未達でユーザーだけが解消できる具体的入力・承認・手動操作が必要ならNeedsUser
   3. command内解消不能failureまたは同一failureの2回目ならBlocked
   4. それ以外の未達で2 query使用済みならBudgetExhausted
   5. 上記以外だけcontinuation
4. `MaxToolRounds` / provider error / tool timeout / approval拒否をDoneへ写像しない。Fault Model内でEvaluator / stall入力または理由付き非達成にする。command外retry / resumeは行わない。
5. 変更・検証要求に対するplan-only assistantを未達にする一方、ContractのDeliverable自体が計画文書ならbounded本文Evidenceを許す対照testを完成させる。
6. 全4 outcomeのhuman / structured presenter snapshotを追加する。必須fieldを省略せず、raw tool output、path、command、secret、assistant control envelopeを無条件表示しない。
7. 既存通常AgentTurn、`max_tool_rounds`、approval、client tool、Human Task suspend、streaming、cancelのregression testsを実行し、0068が既存責務を奪っていないことを確認する。
8. 残る4テストから `#[ignore]` を外し、同じ変更でregistryを `pending=false` にする。0068の全9件が非pendingでなければ完了扱いにしない。

Phase 2 の targeted gate:

```bash
cargo test -p aibe --test 0068_task_completion_phase1_red -j 1 -- --test-threads=1
cargo test -p ai --test 0068_task_completion_phase1_red -j 1 -- --test-threads=1
cargo test -p aibe --test agent_turn_loop -j 1 -- --test-threads=1
cargo test -p ai --test ask_integration -j 1 -- --test-threads=1
./scripts/check-spec-acceptance.py
./scripts/check-feature-scope.py
```

## 5. 受け入れ条件レジストリ

| ID | Phase | テストファイル | 現在 | 解除条件 |
|----|-------|----------------|------|----------|
| `task_completion_vertical_e2e` | 1 | `aibe/tests/0068_task_completion_vertical_e2e.rs` | 実装済み / non-ignored | 本番compositionの一command E2Eが2 query、gap prompt、post-observation、Doneを検証 |
| `task_contract_is_stable_and_complete` | 1 | `aibe/tests/0068_task_completion_phase1_red.rs` | 実装済み / non-ignored | 完全Contract、tool前固定、ID改変拒否、専用provider callなしを検証 |
| `side_effect_requires_post_observation` | 1 | 同上 | 実装済み / non-ignored | effectのみ未達、effect後read-only observationで充足を検証 |
| `completion_evaluator_is_structured_and_fail_closed` | 1 | 同上 | 実装済み / non-ignored | invalid envelope表、機械判定優先、追加provider callなしを検証 |
| `continuation_is_gap_driven_and_detects_plan_only` | 1 | 同上 | 実装済み / non-ignored | gap prompt、元要求非再送、plan deliverable対照例を検証 |
| `assistant_claim_is_not_verified_evidence` | 2 | 同上 | 実装済み / non-ignored | claim / plan / 未実行tool要求がverifiedにならないことを検証 |
| `terminal_outcomes_are_distinct` | 2 | 同上 | 実装済み / non-ignored | 同一入力から優先順どおり一意の4 outcomeを検証 |
| `progress_and_stall_are_bounded` | 2 | 同上 | 実装済み / non-ignored | 最大2 query、fingerprint、failure counterを検証 |
| `final_report_lists_evidence_and_unverified_items` | 2 | `ai/tests/0068_task_completion_phase1_red.rs` | 実装済み / non-ignored | 全outcomeの必須fieldとsanitizeをsnapshot検証 |

`pending=false` は「stubを消した」時点ではなく、対応する同名testが本番経路または指定したdomain/application境界を実assertionで検証し、`#[ignore]` なしで成功した同じ変更でのみ設定する。Phase単位の一括解除でも、全対応testが緑でなければならない。

## 6. Security / bounded data

- Evidence summary、required evidence、next objective、failure reason、Deliverable本文には既存のbounded output方針を適用する。上限値はprotocol既存上限を再利用し、無制限`String`複製を増やさない。
- raw tool output、shell command全文、ファイル内容、API key、環境変数値をEvidence fingerprint、trace、final report、errorへ複製しない。
- assistant envelopeはuntrusted provider outputとしてparseし、未知field方針、件数・文字数、重複ID、参照整合を検査してから使う。
- write-like tool成功をverification扱いしない。read-only分類は既存tool risk / registry正本を使い、LLM申告のtool種別を信用しない。
- malformed envelope時に副作用を先行させず、Doneへfail-openしない。
- Evidence表示のsanitize / bounded契約は `docs/security.md` に正本化する。

## 7. ドキュメント同期

Phase 1–2の実装と同じ変更で次を更新する。

- `docs/architecture.md`: 既存 Query Loop外側のTask Completion Loop、request-local state、Contract固定順、Evidence ledger、assistant envelope validator、gap continuation、4終端、query budgetと`max_tool_rounds`の差、`ai` / `aibe` / `aish`境界
- `docs/architecture.md` のstdio JSON schema: CompletionReport DTO、outcome、必須field、optional compatibilityを実装と一致させる
- `docs/security.md`: Evidence provenance、post-effect observation、assistant claim非信頼、control envelope非表示、bounded / sanitize / fingerprint規則
- 必要なら `docs/testing.md`: 0068 E2Eのmock provider / tool / socket構成と直列実行方針
- `docs/0000_spec-index.md`: 実装中はtasks行を維持し、全AC + verify成功後のみdoneへ更新

## 8. 手動検証 / smoke

必須受け入れはdeterministic automated testsで完結させ、実provider/API keyをCIや完了条件に要求しない。自動E2Eでsocket sandbox差異を吸収できない場合だけ、`docs/manual/0068_task-completion-phase1.md` を追加し、次を記録する。

1. 一時directoryで変更→read-only再観測が必要な要求を一つ実行
2. 2回目が元要求の再送でなくgap / next objectiveになっていることをverboseではなく安全なdiagnosticで確認
3. Done reportのEvidenceとquery使用数を確認
4. plan-only、approval拒否、budget到達の各理由付き非達成を確認
5. stdout / stderr / logsにcontrol envelope、raw secret、無制限tool outputが出ないことを確認

手動検証を追加した場合、未実施なら完了報告の「残リスク」に明記する。実API keyや本番設定をrepoへ保存しない。

## 9. Non-goals / STOP-THE-LINE

0068へ実装しないもの:

- Contract / Evidence / evaluationの永続store、resume、履歴検索、migration、GC
- query budgetのconfig / env公開、適応的増減
- Agent委譲、side agent、secondary agent loop、別conversation、並列query / tool
- Human Task 0062–0066 lifecycleとの新規統合
- Planner DSL、汎用workflow engine、開発工程state machine
- watcher、reconciler、lease / heartbeat、journal、idempotency key、exactly-once、transaction / rollback
- filesystem変更対象の完全推論、command外の結果不明状態自動照合
- 既存 Query Loop、provider adapter、approval、timeout、streaming、`max_tool_rounds`の再実装
- Contract生成またはCompletion評価専用のQuery Loop / provider呼び出し
- すべてのcriterionをコードだけで意味判定する仕組み
- `aish`へのLLM / aibe依存追加

新しい実行主体、Task Completion lifecycle以外の新しい状態機械、永続aggregate、external effect、新RPC / socket / process、二次agent、2つ目のintegrationが必要になったらSTOP-THE-LINEとする。実装を止め、`scope_revision`を増やしてComplexity Gateを再判定し、MVPから外すか別specへ分割する。

## 10. 完了条件と実装順序

推奨実装順序:

1. ignored stubを観測可能なRED testsへ置換
2. domain invariant / outcome table
3. assistant envelope codecと副作用前Contract gate
4. Query Loop internal result / Evidence capture
5. Task Completion orchestrationとgap continuation
6. protocol DTOとDone E2E（Phase 1 gate）
7. stall / 4終端 /全report / sanitize（Phase 2）
8. architecture / security / testing docs同期
9. targeted tests、`./scripts/verify.sh`
10. 全9 ACの `pending=false` と非ignoredを確認し、実装完了コミット時に本書を `docs/done/` へ移動してindex更新

完了には次のすべてを満たすこと。

1. 全9 locked ACが同名testで緑、`pending=false`、`#[ignore]`なし
2. Phase 1 E2Eが本番compositionを通り、追加provider callなし・query最大2を観測
3. 通常AgentTurn / max rounds / approval / streaming / Human Task既存testsにregressionなし
4. `docs/architecture.md` / `docs/security.md` / wire schemaが実装と同期
5. 手動検証が必要なら手順と実施状況を記録
6. `./scripts/verify.sh` が成功
7. 上記完了後のみ `scripts/feature-scope.toml` を `done`、本書を `docs/done/`、indexを実装済みに更新

## 11. 仕様との差分

なし。本書と実装が矛盾する場合は設計書 `docs/spec/0068_task-completion-phase1-spec.md` を優先する。実装上の発見で正本を変える必要がある場合は、Scope Lock分類とrevision更新を先に行う。

## 12. 未確定事項

- **未確定**: 汎用shell commandの副作用対象と再観測対象を関連付ける最小metadata。Phase 1の下限は既存tool種別、実行順、criterion / Evidence IDの明示関連付けとし、完全推論へ拡張しない。
- **未確定**: wire表現を `AgentTurnResult.completion_report: Option<_>` とするか専用response variantとするか。後方互換、既存clientのfail-closed性、streaming control payload非表示を比較し、実装前に `aibe-protocol` testとarchitecture schemaで固定する。新RPCは作らない。
- **推測**: assistant envelopeはprovider共通のassistant content内に構造化control payloadとDeliverable本文を分離して載せ、aibeでbuffer / decodeする方式を第一候補とする。provider固有structured-output APIや評価専用callは導入しない。
- **推測**: Issue #18の `AI_MAX_QUERIES=2` 相当は公開envではなく、Task Completion Loopが既存 Query Loopを開始できる固定上限2を意味する。
