# 0070 Task Completion Phase 3 Independent Verification 実装指示書

> **種別**: 実装指示書（`docs/done/`）
> **状態**: 実装済み（Phase 1–3、全 AC 緑、verify + smoke 成功）
> **正本**: [`0070_task-completion-phase3-verification-spec.md`](../spec/0070_task-completion-phase3-verification-spec.md)
> **関連**: [`0068_task-completion-phase1-implementation-spec.md`](0068_task-completion-phase1-implementation-spec.md)、[`0069_agent-task-delegation-implementation-spec.md`](0069_agent-task-delegation-implementation-spec.md)、[`feature-development-policy.md`](../feature-development-policy.md)、[`architecture.md`](../architecture.md)、[`security.md`](../security.md)

## 0. 目的

設計書 0070 を唯一の正本として、既存 Task Completion の query budget 2 の内側に `delegated-result-verification-cycle` を実装する。親は最初の `agent_task` 実行前に structured Verification Plan を Task Contract とともに固定し、Worker の `verified=false` Result / Artifact references を受け取った後、自身の既存 tool 経路で成果物・Git・固定 command を再観測する。未達時は actionable criteria を一つの bounded Gap へまとめ、0069 の既存 `AgentTaskRequest` field へ変換して同じ Worker へ一度だけ再委譲し、同じ plan の再実行後に理由付き終端へ到達する。

独立 verifier Agent、評価専用 provider call、3回目 query、永続 store、開発・PR・CI 専用 workflow は作らない。`human_task` は未検証自己申告を Done 根拠にしない既存安全契約の回帰対象に留め、cross-suspend verification へ接続しない。

## 0.1 Scope Lock

- Feature scope registry: `scripts/feature-scope.toml`
- Status: `locked`
- Scope revision: `3`
- Complexity class: Yellow（`scope_review = "approved"`）
- Vertical slice AC ID: `delegated_verification_vertical_e2e`
- Query budget: 0068 の固定値 `2` を維持
- Follow-up budget: 元 Agent Task ごとに固定値 `1`
- Locked AC IDs:
  - `delegated_verification_vertical_e2e`
  - `parent_contract_owns_delegated_completion`
  - `parent_reobserves_artifacts_and_external_state`
  - `evidence_precedence_and_conflicts_fail_closed`
  - `criterion_evaluation_is_exhaustive_and_structured`
  - `gap_follow_up_is_single_bounded_and_same_worker`
  - `follow_up_repeats_verification_and_detects_stagnation`
  - `verification_terminal_outcomes_are_distinct`
  - `verification_preserves_existing_boundaries_and_human_task`
  - `verification_report_is_bounded_and_auditable`

Scope Lock 後に本 spec をブロックできる指摘は `BLOCKER_ORIGINAL_AC` / `REGRESSION` / `SAFETY_WITHIN_FAULT_MODEL` だけである。`NEW_REQUIREMENT` / `HARDENING` / `OUT_OF_FAULT_MODEL` は本書へ追加せず、設計書 §10 の Deferred または別 spec へ送る。AC の追加・削除・言い換えが必要なら実装を止め、設計書と `feature-scope.toml` の revision を先に更新する。

## 1. Phase 分割

| Phase | 内容 | 対応 AC | ゲート |
|-------|------|---------|--------|
| 1 | structured Verification Plan、親 Contract ownership、四状態評価、初回委譲→親再観測→一つの Gap→同一 Worker follow-up→同一 plan 再検証→Done の Minimum Vertical Slice | `delegated_verification_vertical_e2e`、`parent_contract_owns_delegated_completion`、`parent_reobserves_artifacts_and_external_state`、`criterion_evaluation_is_exhaustive_and_structured`、`gap_follow_up_is_single_bounded_and_same_worker`、`follow_up_repeats_verification_and_detects_stagnation` | 6件を観測可能な実テストへ置換し、`#[ignore]` 解除と同じ変更で `pending=false`。Vertical Slice が緑になるまで Phase 2 へ進まない |
| 2 | Evidence precedence / conflict、7終端と wire projection、bounded report、0068 / 0069 / `human_task` 回帰 | `evidence_precedence_and_conflicts_fail_closed`、`verification_terminal_outcomes_are_distinct`、`verification_preserves_existing_boundaries_and_human_task`、`verification_report_is_bounded_and_auditable` | 残る4件を緑にし、全10 AC を `pending=false` にする |
| 3 | architecture / security / testing / manual 同期、targeted test、全体検証、完了処理 | 新規 AC は追加しない | `./scripts/verify.sh` 成功後だけ本書を `docs/done/` へ移す |

**Vertical Slice Gate**: Phase 1 は query 1 の初回 Agent Task と検証、query 2 の Gap follow-up と再検証を一本で通す。verification command の全 failure matrix、7終端 snapshot、Human Task 全回帰など Phase 2 hardening を先に実装しない。一方、Contract / plan の tool 前固定、通常 shell 非信頼、同一 Worker / cwd / timeout / approval、`verified=false` は Vertical Slice 自体の安全不変条件なので Phase 1 から省略しない。

## 2. 変更対象と責務配置

現行 0068 / 0069 の配置へ最小拡張する。新しい top-level framework や service tree は作らない。

### 2.1 `aibe` domain

| パス | 変更 | 責務 |
|------|------|------|
| `aibe/src/domain/task_completion.rs` | `CompletionCriterion.applicability`、structured plan item、四状態 `CriterionStatus`、`applicability_evidence_ids`、Gap / GapEntry、`VerificationTerminal`、follow-up count を追加 | bounded schema、ID集合、criterion coverage、not_applicable 条件、Evidence参照、終端順、progress / stagnation を純粋関数で検査 |
| `aibe/src/domain/agent_task.rs` | 原則変更しない。既存 `AgentTaskRequest` / validation constants を follow-up変換側から再利用 | `gap` wire field、verified=true、retry state を Agent Task domain へ追加しない |
| `aibe/src/domain/mod.rs` | 追加した Task Completion 型だけを re-export | application が domain 型を利用可能にする |

`TaskContract.delegated_verification` は 0070 非適用 turn では省略可能な additive fieldとする。Agent Task を呼ぶ場合は non-empty、stable plan ID 一意、Agent Task criterion ID が親 Contract ID の部分集合、各委譲 criterion が1件以上の plan itemで覆われることを Worker 起動前に要求する。

`CriterionStatus` は `Satisfied / Unsatisfied / Unknown / NotApplicable` の4値へ拡張する。`NotApplicable` は Contract 固定時から non-empty `applicability` を持ち、evaluation の `applicability_evidence_ids` が存在する verified / non-stale Evidenceだけを参照する場合に限る。条件と Evidence の意味的対応は既存 Completion Evaluator envelopeに委ね、集合・参照・verified / stale はコードで検査する。

新しい件数・byte設定や公開 config は追加しない。plan / Gap は既存 `CONTRACT_MAX_*` と 0069 `MAX_*` のうち厳しい側へ収める。安全指示または criterion を truncation して通さず、変換不能なら `Blocked` とする。既存上限だけでは一意に実装できないと判明した場合は推測で定数を増やさず STOP-THE-LINE とする。

### 2.2 `aibe` application

| パス | 変更 | 責務 |
|------|------|------|
| `aibe/src/application/completion_envelope.rs` | assistant envelope の additive Contract / evaluation fieldをdecode | plan、applicability、四状態、参照集合を untrusted input として parseし、既存 ContractGateへ渡す |
| `aibe/src/application/task_completion.rs` | cycle本体、Evidence分類、Gap生成、follow-up request変換、report構築を追加 | request-local plan / Evidence / evaluation / follow-up countを所有し、既存 Query Loop 2回だけを使う |
| `aibe/src/application/request_service.rs` | query 1 / 2 の既存 orchestrationに cycleを接続 | query 1で初回委譲+検証、query 2でfollow-up+再検証。評価専用 queryを作らない |
| `aibe/src/application/agent_task.rs` / `agent_task_tool.rs` | 本番契約は変更しない。必要なら既存実効 cwd / timeoutをTask Completion側が記録できる読み取り境界だけ追加 | registry、approval、depth、cwd、timeout、Worker実行、Result normalizationの0069責務を維持 |
| `aibe/src/application/tool_round/executor.rs` | 原則既存 executed call / gateを再利用 | plan固定前、plan不正時、follow-up budget超過時のtool実行0を保証。新loopを持たせない |

処理順を次で固定する。

```text
query 1 assistant step
→ Task Contract + non-empty DelegatedVerificationPlan decode
→ ContractGate: plan ID / criterion coverage / AgentTask request / canonical cwd 整合
→ 初回 agent_task（0069 approvalを含む）
→ fixed command plan（既存 shell_exec allowlist / approval）
→ command後の trusted read-only observation / Git観測
→ 全criterion四状態評価
→ 未達なら一つの Gapを構築
→ query 2 continuationへ固定Contract / plan / Gap / Evidenceを渡す
→ Gapを既存 AgentTaskRequestへ決定的変換
→ 同じWorker / canonical cwd / effective timeout、再approvalで一度だけ実行
→ 同じplanを command → observation 順で再実行
→ 再評価 → Done または理由付き終端
```

Gap follow-up の `AgentTaskRequest` 変換は設計書 §1.3 の表をそのまま実装する。`worker`、canonical cwd、実効 timeout は初回と同じ値、objective は元目的+bounded gap目的、instructions は元指示+Gap entries、completion criteria は元 Contract ID / description とする。0069 request schemaへ fieldを追加せず、変換後も既存 shape / registry / cwd / depth / timeout / approval validationを再実行する。

通常の `shell_exec` は引き続き `UnknownShellEffect` である。0070 Active、委譲後、固定 plan item の exact command / args / canonical cwd一致、allowlist、approval、timeout、`status=Ok` をすべて満たす場合だけ、指定 criterionに限定した `Verification` 候補とする。commandは過去 observationをstale化し、成果物内容 criterionはcommand後の直接観測も要求する。Worker / Artifact / Gap由来commandは受理しない。

### 2.3 `aibe-protocol` / wire

| パス | 変更 | 責務 |
|------|------|------|
| `aibe-protocol/src/response.rs` | optional `CompletionReport.verification_terminal` と `CompletionCriterionReport.evaluation_status` を追加 | 既存 `CompletionOutcome` 4 variant、`satisfied: bool`、top-level `Error` / `Cancelled` を維持した additive projection |
| `aibe-protocol/src/lib.rs` | 新 enum / DTOをre-export | `aibe` / `ai` / testsがleaf wire型を参照 |

wire mappingは設計書 §9.2から変更しない。Stagnatedは既存 `outcome=blocked` + typed `verification_terminal=stagnated`、Failedは既存 `ClientResponse::Error`、Cancelledは既存 `ClientResponse::Cancelled`。Phase 1で必要なのはDone projectionと四状態の最小経路までであり、全7終端表はPhase 2で完成させる。0068非適用reportでは新optional fieldを省略し、既存serde fixtureとclient decodeを維持する。

### 2.4 `ai` presenter

| パス | 変更 | 責務 |
|------|------|------|
| `ai/src/adapters/outbound/stdout_presenter.rs` | Phase 2で新terminal / criterion状態、Gap、follow-up回数、bounded verification結果をhuman / JSON / TSV / env既存経路へ追加 | domain判定は行わずwire DTOだけを表示。raw command / path / output / secretを複製しない |
| `ai/tests/0070_task_completion_phase3_verification_red.rs` | presenter acceptance | 5 report終端と既存 Error / Cancelled の表示、sanitize、上限をsnapshot / structured assertionで固定 |

`ai` は一request送信のままとし、client側でquery / retryを回さない。`aish` は変更対象外である。

### 2.5 テスト / fixture

| パス | 変更 | 責務 |
|------|------|------|
| `aibe/tests/0070_task_completion_phase3_verification_red.rs` | 9 ACのignored REDを段階的に実assertionへ置換 | domain table、application integration、production composition vertical E2E、0068/0069/Human Task regression |
| `ai/tests/0070_task_completion_phase3_verification_red.rs` | report ACのignored REDを実assertionへ置換 | presenter / structured output |
| `aibe/tests/fixtures/0069_agent_task_worker.sh` | 可能なら既存deterministic modeを再利用 | 実Worker製品/API/networkなしで同一Worker 2回とartifact変更を再現 |
| `aibe/tests/fixtures/0070_*` | 既存fixtureでGap修正を表現できない場合だけ追加 | 初回未達、follow-up後成功を入力回数または固定stdin内容で決定的に再現。production adapterを迂回しない |

## 3. Phase 1 実装手順 — Minimum Vertical Slice

1. Phase 1の ignored 6 testを、設計書の観測点を持つ実RED assertionへ置換する。まだ `#[ignore]` / `pending=true` は維持する。
2. `TaskContract` / criterion / evaluationの additive serde fieldとdomain invariantを実装する。既存0068 fixture struct literal / JSONを省略時defaultで通し、Agent Task call時だけnon-empty planを要求する。
3. `DelegatedVerificationPlan`のstable ID、criterion subset / coverage、canonical effective cwd、command shapeをContractGateで最初のWorker起動前に検査する。invalid caseはWorker / shell spawn 0をassertする。
4. 四状態evaluationを実装し、unknown ID、欠落・重複ID、invalid Evidence参照、applicabilityなしNotApplicable、stale applicability Evidenceを拒否する。
5. fixed command exact-match分類をEvidence ledgerへ追加する。通常shellは既存どおりUnknownShellEffect、command後read-only observationだけがfile contentを検証できることをtable testで固定する。
6. すべてのactionable未達を1 Gap / entriesへまとめ、既存 `AgentTaskRequest`へ決定的変換する。same Worker / cwd / timeout、follow-up count 1、新approvalをrecording fakeでassertする。
7. request-local cycleを既存 query budget 2へ接続する。query 1と2の中でそれぞれAgent Task→command→observation→evaluationを行い、3回目provider callが0であることを記録する。
8. scripted provider、production RequestService / tool registry、production AgentTaskService / Worker registry、deterministic fixture、recording approval、fake read/Git/verification toolsを使うvertical E2Eを完成させる。初回Workerは未達artifact、follow-upはGapを受けて修正、親は同じplanを2回実行し、最後に独立Evidence付きDoneを返す。
9. Phase 1の6 testを緑にして `#[ignore]`を外し、同じ変更で対応6 caseだけを `pending=false`にする。Vertical Sliceが緑になる前にPhase 2へ進まない。

Phase 1 targeted gate:

```bash
cargo test -p aibe --test 0070_task_completion_phase3_verification_red -j 1 -- --test-threads=1
cargo test -p aibe --test 0068_task_completion_vertical_e2e -j 1 -- --test-threads=1
cargo test -p aibe --test 0069_agent_task_delegation_red -j 1 -- --test-threads=1
./scripts/check-spec-acceptance.py
./scripts/check-feature-scope.py
```

## 4. Phase 2 実装手順 — Conflict / terminal / report / regression

1. 残る4 ignored testを実RED assertionへ置換する。
2. 同じcriterion / targetのEvidence優先順位を、直接観測 / Git diff → fixed command → changed-file observation → Worker reportの順でtable test化する。異なるtargetの要求は片方を捨てず、同順位矛盾、stale、unrelated success、missing EvidenceをUnknownまたはinvalidにする。
3. 設計書 §9.1 の7終端順をdomain table testで固定する。non-zero verification、missing artifact、Evidence矛盾はcriterion結果であり、検証経路自体の非回復errorだけをFailedにする。
4. 設計書 §9.2 のwire projectionをserde / compatibility testで完成させる。既存4 `CompletionOutcome`を削除・改名せず、Stagnatedをtyped additive fieldで区別し、Failed / Cancelledは既存top-level responseを使う。
5. human / structured presenterを完成させる。AgentTurnResultの5終端では四状態、Evidence provenance、unverified items、Gap、Worker ID、follow-up使用数、bounded command result、理由を表示する。Error / Cancelledでは既存bounded reasonだけを表示する。
6. 0068 query budget / normal completion、0069 registry / approval / cwd / depth / timeout / redaction、`human_task` normal Done / Suspended evaluation skip / checkpoint / resume / continuationをregression testで確認する。Human TaskへVerification PlanやGapを接続しない。
7. 残る4 testを緑にし、`#[ignore]`解除と同じ変更で対応4 caseを `pending=false`にする。全10件が非pendingになるまで完了扱いにしない。

Phase 2 targeted gate:

```bash
cargo test -p aibe --test 0070_task_completion_phase3_verification_red -j 1 -- --test-threads=1
cargo test -p ai --test 0070_task_completion_phase3_verification_red -j 1 -- --test-threads=1
cargo test -p aibe --test 0068_task_completion_phase1_red -j 1 -- --test-threads=1
cargo test -p aibe --test 0069_agent_task_delegation_red -j 1 -- --test-threads=1
./scripts/check-spec-acceptance.py
./scripts/check-feature-scope.py
```

## 5. 受け入れ条件レジストリ

| ID | Phase | テストファイル | 現在 | 解除条件 |
|----|-------|----------------|------|----------|
| `delegated_verification_vertical_e2e` | 1 | `aibe/tests/0070_task_completion_phase3_verification_red.rs` | pending / ignored RED | production Task Completion / Agent Task composition、query 2回、Worker 2回、plan再実行2回、1 Gap、独立Evidence付きDone |
| `parent_contract_owns_delegated_completion` | 1 | 同上 | pending / ignored RED | 親Contract / ID不変、Worker claim / exit 0 / artifact locatorはverified=falseで非Done |
| `parent_reobserves_artifacts_and_external_state` | 1 | 同上 | pending / ignored RED | tool前non-empty plan、ID/coverage/canonical cwd、exact command、直接観測、escape/failure拒否 |
| `criterion_evaluation_is_exhaustive_and_structured` | 1 | 同上 | pending / ignored RED | 四状態、集合一致、NotApplicable applicability + verified/non-stale Evidence、全applicable satisfiedだけ成功 |
| `gap_follow_up_is_single_bounded_and_same_worker` | 1 | 同上 | pending / ignored RED | 1 Gap entries、既存request変換、same Worker/cwd/timeout、再approval、上限/2回目/切替/再帰拒否 |
| `follow_up_repeats_verification_and_detects_stagnation` | 1 | 同上 | pending / ignored RED | 同一plan再実行、progress fingerprint、進展なしStagnated、ユーザー依存NeedsUser |
| `evidence_precedence_and_conflicts_fail_closed` | 2 | 同上 | pending / ignored RED | criterion/target単位の順位、同順位矛盾、stale/unrelated/missing参照を非Done |
| `verification_terminal_outcomes_are_distinct` | 2 | 同上 | pending / ignored RED | 7終端一意判定と既存4 outcome / top-level Error・Cancelledへのadditive projection |
| `verification_preserves_existing_boundaries_and_human_task` | 2 | 同上 | pending / ignored RED | query 2、0069契約、Human Task既存経路、新actor/loop/3回目query不在 |
| `verification_report_is_bounded_and_auditable` | 2 | `ai/tests/0070_task_completion_phase3_verification_red.rs` | pending / ignored RED | 5 report終端とError/Cancelled表示、四状態/Gaps/provenance/回数、sanitize/bounds |

`pending=false` は型やbranchを追加した時点ではない。同名testが上表の本番経路または指定domain / wire / presenter境界を実assertionで検証し、非ignoredで成功した同じ変更でだけ切り替える。

## 6. Mock / fake E2E の骨子

実API、API key、network、実Agent製品を使わない。Vertical Slice fixtureは同一Worker IDで呼び出しを記録し、初回はartifactを不完全な内容で生成して`reported_complete=true`を返し、2回目はinstructions中のGap entryと同一criterion ID / plan item IDを検査してartifactを修正する。両Resultと全Worker Evidenceは`verified=false`のままとする。

scripted providerは4つのassistant stepを返す。

```text
query 1: fixed Contract + Verification Plan + initial agent_task
query 1 final: parent command/observation Evidenceを参照してunsatisfied
query 2: unchanged Contract / plan + Gap follow-up agent_task
query 2 final: repeated command/observation Evidenceを参照してsatisfied
```

recording tools / gatesで次をassertする。

- provider query 2回、Worker spawn 2回、Agent Task approval 2回、follow-up 1回
- Worker ID、canonical cwd、effective timeout、Contract、planは2回で同一
- verification commandは委譲後に exact command / args / cwdで2回、既存shell approval経由
- commandより後のread/Git observationが2回あり、初回EvidenceだけではDoneにならない
- Worker report内のcommandや追加planは実行されない
- 3回目provider call、別Worker、recursive Agent Task、永続file、verifier Agentは0

MockWorker直呼びだけでVertical Sliceを代替しない。production `RequestService`、Task Completion gate、tool registry、AgentTaskService / registry、既存approval境界を通す。socketを含むE2Eがsandbox差異で不安定な場合は0068同様のin-process fallbackを許すが、production service compositionを迂回しない。

## 7. Pack Composition

**No** — 設計書 §8どおり、0070はoptional adapterではなく、Task Completion対象turnが委譲結果を自己申告だけでDoneにしないcore完了意味論である。専用RPC / CLI / turn hook bundle、重いdependency、別配備単位を追加しない。新しいPack trait、Active / Basic実装、runtime toggle、Cargo feature、composition rootを作らない。0069 `AgentTaskPack`がdisabledならAgent Task tool自体が非公開となり、本cycleも起動しない。既存0069 Packの意味を変更しない。

## 8. Security / 不変条件

- Parent ownership: Worker Result / Artifact / GapからContract、criterion、plan、commandを追加・変更しない。
- Command boundary: 委譲前fixed planとのexact command / args / canonical cwd、allowlist、approval、timeout、statusをコードで検査する。通常shellはUnknownShellEffectのまま。
- Observation order: commandは過去観測をstale化する。成果物criterionにはcommand後のtrusted read-only observationを要求する。
- Worker boundary: follow-upも0069のregistry、depth、cwd、timeout、approval、kill/reap、redaction、bounded outputを再利用し、Resultをverified=trueへ書き換えない。
- Bounded data: plan、Gap、Evidence、command result、terminal reason、reportは既存上限内でsanitizeする。raw output、command全文、環境値、credential、workspace外pathをprompt / log / reportへ無制限に複製しない。
- Human Task: normal Done、Suspended evaluation skip、checkpoint / resume / continuation、approvalを変更しない。cross-suspend verificationは実装しない。
- Wire compatibility: 既存field / enum variant / top-level responseを削除・改名しない。additive field省略時の0068 JSON roundtripを保持する。

## 9. ドキュメント同期 / Phase 3

Phase 1–2の実装と同じ変更で以下を同期する。

- `docs/architecture.md`: Task Completion節へDelegatedVerificationPlan、tool前固定、2 query内cycle、Gap変換、command→observation順、四状態、7終端projectionを追記
- `docs/architecture.md` stdio JSON: optional `verification_terminal` / `evaluation_status` と既存fieldの互換関係を追記。Task Contract control envelopeの`delegated_verification` / `applicability`も内部schemaとして同期
- `docs/security.md`: Worker/Artifact/command非信頼、exact-match条件、shell approval、cwd、stale化、bounded/redaction、Human Task非統合を追記
- `docs/testing.md`: fixture Worker、scripted provider、recording tools/gates、query / spawn回数、直列 `-j 1`を追記
- `docs/manual/0070_task-completion-phase3-verification.md`: 任意のlocal Workerでapproval 2回、初回未達→Gap→再作業、同一plan再検証、Stagnated/NeedsUser、秘密非表示を確認する手順。実API / keyは不要。手動確認は自動ACの代替にせず、未実施なら完了報告の残リスクへ記載
- `docs/0000_spec-index.md`: 実装中は本tasks行とAC pending状態を維持し、全AC + verify成功後だけdoneへ更新

## 10. STOP-THE-LINE / やってはいけないこと

0070のComplexity budgetは全項目`+0`である。実行主体3、状態機械2、永続aggregate 0、外部副作用4、process boundary2、新規機構1、integration3を増やさない。

次が必要と判明した時点で実装を停止する。

- 親Task Completion / Query Loop / 外部Worker以外のactor、verifier / critic Agent、secondary agent loop、別conversation
- 既存Task Completion lifecycleとは別のstate machine、汎用retry controller、評価専用または3回目query/provider call
- follow-up 2回以上、Worker切替、複数Worker、parallel / async / queue、Agent間通信、recursive delegation
- Contract / Evidence / plan / Gap / follow-up countの永続化、checkpoint、resume、migration、GC
- lease / heartbeat、reconciler、watcher、journal、idempotency、exactly-once、crash recovery
- Human Task cross-suspend verification / auto follow-up、Human/Agent共通aggregate
- PR / CI / GUI adapter、長期監視、Task Graph / DSL、開発専用workflow / state machine
- 新Pack、runtime toggle、Cargo feature、RPC / socket / process boundary
- process boundary 3以上、external effects 5以上、integration 4以上、novel mechanism 2以上

報告形式:

```text
STOP-THE-LINE

分類: BLOCKER_ORIGINAL_AC / REGRESSION / SAFETY_WITHIN_FAULT_MODEL / NEW_REQUIREMENT / HARDENING / OUT_OF_FAULT_MODEL
発見した要因:
現在scopeへの影響:
Complexity Gateの変化:
MVPからの削除案:
別spec案:
```

禁止事項を「将来必要そう」という理由で前倒ししない。継続が必要なら、設計書のScope change log、`feature-scope.toml` revision / metrics / Gateを先に更新する。

## 11. 完了条件と Cursor の実装順

Cursorの最初の着手点は `aibe/tests/0070_task_completion_phase3_verification_red.rs::delegated_verification_vertical_e2e` をignored panicから、production service compositionの呼び出し回数・Worker Result false・初回Gapまで観測する実RED testへ置換することである。その後、次の順を守る。

1. Phase 1 ignored testsを観測可能なREDへ置換
2. Task Contract / plan / applicability /四状態のdomain invariant
3. ContractGateのWorker前plan検査
4. Evidence exact-match command / observation順
5. Gap生成と既存AgentTaskRequest変換
6. query budget 2内cycleとVertical Slice GREEN
7. Phase 1 `#[ignore]` / pending解除
8. Phase 2 conflict / 7終端 / wire / presenter / regression
9. docs / manual同期、targeted tests
10. 全10 AC非pending・非ignored確認後に`./scripts/verify.sh`
11. verify成功後だけfeature status=`done`、本書を`docs/done/`へ移動、indexを実装済みに更新

完了には次をすべて満たすこと。

1. 全10 locked ACが同名testで緑、`pending=false`、`#[ignore]`なし
2. Vertical Sliceがproduction Task Completion / Agent Task compositionとdeterministic Workerを通り、query 2・Worker 2・follow-up 1・plan再実行を観測
3. 0068 / 0069 / Human Task regressionがない
4. architecture / security / testing / manual / wire docsが実装と同期
5. `./scripts/check-feature-scope.py` と `./scripts/check-spec-acceptance.py` 成功
6. `./scripts/verify.sh` 成功。完了報告へ`.verify-timing-last`のsummaryを転記
7. 上記後だけdone移動とindex/status更新

## 12. 仕様との差分 / 未確定事項

仕様との差分はない。本書と実装が矛盾する場合は設計書 0070 を優先する。

- **推測:** verification commandは設計書 §13どおり、親Contractに委譲前固定され、既存`shell_exec` allowlist / approvalを通るものとして手順化した。Worker出力からのcommand追加は不可
- **推測:** 0070 MVPの新規経路はAgent Taskに限定し、Human Taskは回帰のみとした。cross-suspend verificationはDeferred
- **未確定:** structured plan / Gapの個別byte定数。まず既存Task Contract / Agent Task上限の厳しい側を再利用する。新定数が不可避ならGREEN実装を始めず、設計書に上限と理由を追記してscope分類する
- Git非管理workspaceでは設計書どおりGit failureをUnknownとし、Artifact直接観測と固定commandを続行する。必要Evidenceが得られなければNeedsUser / Blockedで終端し、新fallback機構は追加しない
