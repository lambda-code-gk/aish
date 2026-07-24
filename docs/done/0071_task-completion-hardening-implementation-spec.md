# 0071 Task Completion Hardening 実装指示書

> **種別**: 実装指示書（`docs/done/`）  
> **状態**: 実装済み（全 AC green、verify / smoke 成功）  
> **正本**: [`0071_task-completion-hardening-spec.md`](../spec/0071_task-completion-hardening-spec.md)  
> **関連**: [`0070_task-completion-phase3-verification-implementation-spec.md`](../done/0070_task-completion-phase3-verification-implementation-spec.md)、[`feature-development-policy.md`](../feature-development-policy.md)、[`architecture.md`](../architecture.md)、[`security.md`](../security.md)、[`testing.md`](../testing.md)

## 0. 目的

設計書 0071 を唯一の正本として、0068–0070 の production Task Completion 経路を hardening する。新しい actor / lifecycle / provider call / Worker / observer service は作らず、既存 report builder、Gap builder、`CompletionEventBuffer`、Workspace Observer を補正する。Task Completion 対象 turn の Cancelled / verification infrastructure Failed を bounded `CompletionReport` へ接続し、verification command の実行後 nonzero は Failed / InvalidRequest へ短絡せず criterion 未達と Gap へ回す。Gap は実 Evidence に由来する `observed` と criterion 固有 `required_work` を持ち、streaming と workspace metadata 観測は有限かつ fail-closed にする。

0068 の query budget `2`、0070 の follow-up budget `1` と command plan item `1`、0069 の Worker registry / approval / cwd / depth / timeout / redaction、Human Task の Suspended / resume / continuation は変更しない。意味的 coverage、expected-success Predicate、複数 Worker、並列化、永続化、crash recovery、長期監視は実装しない。

## 0.1 Scope Lock

- Feature scope registry: `scripts/feature-scope.toml`
- Status: `locked`
- Scope revision: `3`
- Complexity class: Yellow（`scope_review = "approved"`）
- Vertical slice AC ID: `task_completion_hardening_vertical_e2e`
- Query budget: `2`（0068 固定値）
- Agent Task follow-up budget: `1`（0070 固定値）
- Novel mechanisms: `[]`
- Locked AC IDs:
  - `task_completion_cancel_report_vertical_e2e`
  - `verification_infrastructure_failure_reports_failed`
  - `task_completion_hardening_vertical_e2e`
  - `gap_observed_uses_actual_evidence`
  - `gap_required_work_is_per_criterion`
  - `gap_follow_up_request_is_per_criterion_and_bounded`
  - `completion_event_buffer_has_total_byte_limit`
  - `workspace_observer_detects_same_size_rewrite_or_marks_incomplete`
  - `task_completion_hardening_preserves_regressions`
  - `task_completion_hardening_verify_and_smoke`

Scope Lock 後に本 spec をブロックできる指摘は `BLOCKER_ORIGINAL_AC` / `REGRESSION` / `SAFETY_WITHIN_FAULT_MODEL` だけである。`NEW_REQUIREMENT` / `HARDENING` / `OUT_OF_FAULT_MODEL` は本書へ追加せず設計書 §10 の Deferred または別 spec へ送る。AC、定数、complexity inventory を変更する必要が出たら実装を止め、設計書と `feature-scope.toml` の revision / reason を先に更新する。

## 1. Phase 分割

| Phase | 内容 | 対応 AC | ゲート |
|-------|------|---------|--------|
| 1 | Minimum Vertical Slice: nonzero Verification Evidence → 複数 criterion の実 observed / 固有 required_work → bounded Gap → 同一 Worker follow-up 1回 → 同一 plan 再検証 → Done | `task_completion_hardening_vertical_e2e` | この1件を production composition の実 assertion へ置換し、`#[ignore]` 解除と同じ変更で `pending=false`。green になるまで Phase 2へ進まない |
| 2 | Failed / Cancelled report、optional wire互換、Gap provenance / criterion別作業 / request bounds の網羅 | `task_completion_cancel_report_vertical_e2e`、`verification_infrastructure_failure_reports_failed`、`gap_observed_uses_actual_evidence`、`gap_required_work_is_per_criterion`、`gap_follow_up_request_is_per_criterion_and_bounded` | 5件をgreenにし、対応caseだけ `pending=false` |
| 3 | `CompletionEventBuffer` byte/item bound と Workspace Observer metadata fingerprint | `completion_event_buffer_has_total_byte_limit`、`workspace_observer_detects_same_size_rewrite_or_marks_incomplete` | 2件をgreenにし、対応caseだけ `pending=false` |
| 4 | 0068–0070 / Human Task / 通常turn / protocol / presenter回帰、docs、mock smoke、完了処理 | `task_completion_hardening_preserves_regressions`、`task_completion_hardening_verify_and_smoke` | 全10件 `pending=false`、targeted test、docs同期。完了直前だけ `verify.sh` と `smoke-mock.sh` |

**Vertical Slice Gate**: Phase 1 は既存 production `RequestService` / Task Completion gate / Agent Task registry / approval / tool execution / report pathを通す。`build_gap` の純粋関数テストや MockWorker 直呼びだけで代替しない。Phase 1 green 前に Failed 全matrix、wire presenter、buffer、observer の横展開を先行しない。ただし nonzero を `Verification(verified=false)` とし、全 actionable criterionを欠落なく同一 follow-upへ渡す不変条件は vertical slice 自体に必須である。

## 2. 変更対象とクレート境界

### 2.1 `aibe` domain / application

| パス | 変更 | 責務 |
|------|------|------|
| `aibe/src/domain/task_completion.rs` | `CriterionEvaluation.required_work: Option<String>`、Gap構築入力、bounded定数 / validation、terminal helperを補正 | criterion ID集合、required_work、Evidence参照、Failed / nonzero分類後の純粋な状態を検査 |
| `aibe/src/application/completion_envelope.rs` | delegated evaluation の optional additive `required_work`を受理しsystem instructionを同期 | untrusted envelopeをdecode。非委譲turnは省略可、0070 Active actionable criterionは必須 |
| `aibe/src/application/task_completion.rs` | `build_gap`呼出し、`build_report` / terminal report builder、follow-up変換、`CompletionEventBuffer`を補正 | Evidence provenanceからGap/reportを構築し、bounded/sanitize/control非表示を保証 |
| `aibe/src/application/request_service.rs` | Task Completion対象turnのcancel / verification infrastructure failureへrequest-local contextを接続 | top-level `Cancelled / Error`を維持しoptional reportを添付。通常Error一般をFailedへ変換しない |
| `aibe/src/application/agent_turn.rs` / `tool_round/executor.rs` | 必要最小限のtyped cancellation / command execution state伝搬 | command開始前/後、exit/timeout/signal、result acquisition、cleanup/reapを文字列prefixでなく型で区別 |
| `aibe/src/adapters/outbound/agent_task/workspace_observer.rs` | metadata tupleを`WorkspaceEntryFingerprint`相当のnamed型へ変更 | file contentを読まず、Unix metadataとincompleteを収集 |
| `aibe/src/adapters/outbound/agent_task/external_command_worker.rs` | 既存snapshot前後比較結果をそのままResultへ渡す | Worker execution、snapshot順、secret-bearing path除外の0069責務を維持 |

`aibe` は LLM / tool / Worker orchestrationを所有する。`ai`へterminal判定、Gap構築、observerを移さない。Workspace Observerはoutbound adapterのままとし、domain/applicationに`std::fs`を持ち込まない。

### 2.2 `aibe-protocol`

| パス | 変更 | 責務 |
|------|------|------|
| `aibe-protocol/src/response.rs` | `ClientResponse::Error / Cancelled`へoptional `completion_report`、wire `VerificationTerminal::Failed / Cancelled`を追加 | `#[serde(default, skip_serializing_if = "Option::is_none")]`、既存top-level variant / ErrorCode / reason / CompletionOutcome 4値を維持 |
| `aibe-protocol/src/lib.rs` | 既存re-exportを維持。新DTOを作る場合だけ追加 | protocolをleafに保つ |
| `aibe-protocol` unit tests | old/new JSON fixture、field omission/default、unknown fieldを許すlegacy-shape fixtureを追加 | wire compatibilityをコードで固定 |

Rust enum variantのfield追加はsource compatibilityを持たない。workspace内の全constructorと`..`なしpatternを同じPhase 2変更で更新する。旧payloadは新consumerで`None`、Task Completion対象外 / 通常Errorの新payloadはfield省略、現行serde相当のlegacy fixtureは新fieldを無視して従来top-level variantとして読めることをテストする。

### 2.3 `ai` / `aibe-client`

| パス | 変更 | 責務 |
|------|------|------|
| `ai/src/adapters/outbound/stdout_presenter.rs` | Error / Cancelledのoptional reportをhuman / structured表示へ接続 | top-level Error / Cancelledを維持し、typed terminalを優先表示。raw output/control JSONを出さない |
| `ai/src/main.rs`、memory/work CLIのmatch | protocol field追加に伴うconstructor / patternの機械更新のみ | Task Completion対象外のexit code /表示を変更しない |
| `ai/tests/0071_task_completion_hardening_red.rs`（必要なら新規） | presenter / JSON / omission compatibilityの非registry回帰テスト | aibe側AC testを置換せず補完する |
| `aibe-client/src/transport.rs` / tests | compile / decode回帰のみ | query / retry /terminal判定をclientへ追加しない |

`aish` は変更対象外である。`aish`にLLM、Task Completion、protocol判定、observerを追加しない。

### 2.4 Acceptance / fixture / docs

| パス | 変更 | 責務 |
|------|------|------|
| `aibe/tests/0071_task_completion_hardening_red.rs` | 10個のignored panicをPhase順に観測可能な実RED assertionへ置換 | `scripts/spec-acceptance.toml`と同名1:1を維持 |
| `aibe/tests/0070_task_completion_phase3_verification_red.rs`、0068 / 0069 tests | 既存回帰。原則既存testを変更せず通す | 既存契約を0071都合で弱めない |
| `aibe/tests/fixtures/0071_*` | 既存fixtureでspawn failure / nonzero /同サイズrewriteを決定的に表現できない場合だけ追加 | 実API / network /実Agent製品を使わない |
| `docs/architecture.md` / `docs/security.md` / `docs/testing.md` | Phase 4で実装と同期 | protocol、trust boundary、test matrixを正本化 |
| `docs/manual/0071_task-completion-hardening.md` | optional local Worker / deterministic mockの確認手順 | 手動結果は自動ACの代替にしない |

## 3. 上限定数

公開config /環境変数にせず、責務を持つmoduleの`pub(crate)`またはprivate定数へ固定する。

| 定数（推奨名） | 値 | 配置 / 用途 |
|----------------|----|-------------|
| `COMPLETION_REPORT_MAX_BYTES` | `128 * 1024` | `aibe/src/application/task_completion.rs`。serialized report全体 |
| `COMPLETION_REPORT_MAX_EVIDENCE_PER_CRITERION` | `8` | criterion reportごとのEvidence |
| `COMPLETION_REPORT_MAX_EVIDENCE_TOTAL` | `64` | report全体で採用するEvidence |
| `COMPLETION_REPORT_MAX_UNVERIFIED_ITEMS` | `32` | omission marker 1件を含む |
| `COMPLETION_REPORT_TEXT_MAX_BYTES` | `1024` | terminal reason、Evidence / Gap summary、unverified item |
| `CRITERION_REQUIRED_WORK_MAX_BYTES` | `1024` | delegated actionable criterionの`required_work` |
| `GAP_OBSERVED_MAX_BYTES` | `1024` | 実Evidence / typed observation由来の`observed` |
| `GAP_REQUIRED_WORK_MAX_BYTES` | `1024` | Gap entry / follow-up instructionへ渡す作業 |
| `GAP_FOLLOW_UP_MAX_SERIALIZED_BYTES` | `128 * 1024` | 既存`AgentTaskRequest` validation後の追加total bound |
| `COMPLETION_EVENT_BUFFER_MAX_BYTES` | `64 * 1024` | 既存`ENVELOPE_MAX_BYTES`と同値 |
| `COMPLETION_EVENT_BUFFER_MAX_DELTAS` | `1024` | 空delta大量投入もboundedにする |
| `MAX_OBSERVED_PATHS` | 既存`4096`を維持 | Workspace snapshot entry数 |
| `MAX_CHANGED_PATHS` | 既存`256`を維持 | changed path出力数。超過はtruncate + incomplete |

次の既存上限も緩和しない。

- `CONTRACT_MAX_CRITERIA = 32`、`GAP_MAX_ENTRIES = 32`
- `MAX_INSTRUCTIONS = 32`、`MAX_INSTRUCTION_BYTES = 2048`、`MAX_INSTRUCTIONS_BYTES = 16 KiB`
- `MAX_CRITERIA = 32`、`MAX_CRITERIA_BYTES = 32 KiB`
- `ENVELOPE_MAX_BYTES = 64 KiB`

Reportはterminal / outcome / queriesと全criterion ID / statusを先に保持し、Evidence / Gap summary / unverified itemを安定ID順に採用する。省略時は`unverified_items`にbounded omission markerを1件予約する。最小projectionが128 KiBへ収まらないContractは最初のtool実行前に拒否する。Gap follow-upは既存`AgentTaskRequest::validate_shape`と128 KiB totalの両方を通し、criterion / safety instructionをpartial truncationせず、収まらなければ`Blocked`とする。

## 4. Phase 1 — Minimum Vertical Slice

1. `task_completion_hardening_vertical_e2e`をignored panicから実REDへ置換する。まだ`#[ignore]` / `pending=true`は維持する。
2. fixtureはfixed Contractに最低2 criterionと同じWorker、command plan item 1件を持つ。query 1でWorker実行後、exact-match commandが実processとして開始されnonzeroを返す。
3. nonzero callを`InvalidRequest / Failed`へ変換せず、`EvidenceSource::Verification`、`verified=false`、対象criterion / plan item ID付きでledgerへ残す。
4. `CriterionEvaluation.required_work`をoptional additiveでdecodeする。0070 Activeかつ`Unsatisfied / Unknown`のactionable criterionではtrim後nonempty・1024 bytes以下を必須とする。非委譲turnとSatisfied / NotApplicableでは省略可能とし、既存0068 JSONを維持する。
5. `build_gap`を、Contract / evaluationに加えて検査済みEvidence ledgerとtyped verification / observation stateを受け取る形へ補正する。各criterionの`observed`はnon-stale・同criterionのEvidenceから、`required_work`は同criterion evaluationから構築する。
6. 2 criterionが異なるobserved / required_work / plan itemを持つ一つのGapを作り、既存`AgentTaskRequest`へ決定的変換する。同じWorker / cwd / timeout、follow-up 1回、新approval、元criterion IDを維持する。
7. query 2で同じplanを再実行し、独立Evidence付きDoneへ到達する。provider query 2、Worker spawn 2、follow-up 1、3回目query 0をrecording fakeでassertする。
8. green後にこの1 testだけ`#[ignore]`を外し、同じ変更でregistryの`pending=false`へ変更する。Phase 2へ進む前に0070 vertical / nonzero回帰も通す。

Phase 1 targeted gate:

```bash
cargo test -p aibe --test 0071_task_completion_hardening_red -j 1 -- --test-threads=1
cargo test -p aibe --test 0070_task_completion_phase3_verification_red -j 1 -- --test-threads=1
./scripts/check-spec-acceptance.py
./scripts/check-feature-scope.py
```

## 5. Phase 2 — Terminal report / Gap correctness

### 5.1 Failed / nonzero / Cancelled 分岐

実装分岐を次で固定する。error message文字列のcontains / prefixで分類せず、process runner / executed call / cancellation tokenのtyped stateを使う。

| 入力 | 結果 |
|------|------|
| assistant schema / Evidence参照不正、plan mismatch、cwd / allowlist拒否 | 既存`InvalidRequest / ToolNotAllowed`等。Failed reportへ変換しない |
| approval取消、turn cancellation | top-level`Cancelled` + bounded report `verification_terminal=Cancelled` |
| exact-match commandのspawn不能 | top-level`Error` + `verification_terminal=Failed` |
| 起動済みcommandのexit nonzero | `Verification(verified=false)` → criterion未達 / Gap |
| timeout / signal後にcleanup / reap /終了状態取得成功 | `Verification(verified=false)` → `Unknown` / Gap |
| result acquisitionまたはcleanup / reap不能で構造化終了状態なし | top-level`Error` + `verification_terminal=Failed` |
| Workspace observation incomplete | `Unknown` + `observation_incomplete` / Gap。Failedではない |

`Failed`はfixed planを持つContract固定後だけ成立する。CancelledはContract固定前ならcriteria空+bounded unverified marker、固定後なら全criterionをUnknown / Unsatisfiedとして保持する。legacy`CompletionOutcome`は`Blocked`、new consumer / presenterは`verification_terminal`を優先する。通常のprovider error、Human Task取消、Task Completion対象外Error / Cancelledにはreportを添付しない。

### 5.2 Gap 構築

criterionごとに次の順で構築する。

1. Contractとevaluationのcriterion ID集合が完全一致することを既存validationで確認する。
2. `Unsatisfied / Unknown`だけをactionable候補とし、同criterionのnon-stale Evidence IDを安定昇順で解決する。unknown / stale /別criterion IDはInvalidRequest。
3. Evidenceがあればsanitized summaryを`observed`へ採用する。複数時は1024 bytes内でID順に結合し、`required_evidence`文字列は使わない。
4. Evidenceがなければserver-owned typed stateの`observation_incomplete / plan_not_executed / evidence_conflict / no_observation`からobservedを作る。typed reasonもない入力はinvalid evaluationとして拒否する。
5. `required_work`は同じCriterionEvaluationのfieldから取得する。global`next_objective`を全entryへ複製しない。空白、1024 bytes超過、Satisfied / NotApplicableとの矛盾を拒否する。
6. fixed planから当該criterionを覆うplan item IDを安定順で付ける。coverageなしは既存plan invariant違反。
7. 全actionable criterionを`GAP_MAX_ENTRIES=32`内の一つのGapへ入れる。一部だけ落とさない。
8. follow-up instructionはcriterionごとに`criterion_id / observed / required_work / plan_item_ids`を一itemとして作る。既存instructionsと合わせて32件 /各2048 bytes /合計16 KiBを超える、またはserialized 128 KiBを超える場合はtruncationせずBlocked。

Phase 2ではprotocolのoptional field、wire terminal 2値、`ClientResponse::error` helper / constructors、ai presenterも同時に更新し、AC1 / AC2 / AC4–6をgreenにする。

Phase 2 targeted gate:

```bash
cargo test -p aibe --test 0071_task_completion_hardening_red -j 1 -- --test-threads=1
cargo test -p aibe-protocol -j 1
cargo test -p ai --test 0071_task_completion_hardening_red -j 1 -- --test-threads=1
cargo test -p aibe --test 0068_task_completion_phase1_red -j 1 -- --test-threads=1
cargo test -p aibe --test 0070_task_completion_phase3_verification_red -j 1 -- --test-threads=1
./scripts/check-spec-acceptance.py
./scripts/check-feature-scope.py
```

`ai/tests/0071_*`を追加しない場合は対応commandを省く。存在しないtest targetを完了手順へ残さない。

## 6. Phase 3 — Event buffer / Workspace Observer

### 6.1 `CompletionEventBuffer`

- `deltas: Vec<String>`だけでなく`buffered_bytes / delta_count / overflowed`を同じmutex保護stateへまとめる。
- delta受信時はUTF-8 bytesを`saturating_add`し、追加後が64 KiBまたは1024件を**超える**場合、そのdeltaをpartial保存せず既存Vecをclearしてcount / bytesを0、`overflowed=true`にする。境界値ちょうどは保存可。
- overflow後のassistant deltaは空文字を含めすべて破棄し、Vec / count / bytesを増やさない。
- `flush_for_response`はAgentTurnResult / Suspended / Error / Cancelledのどれでもoverflow済みdeltaをreplayしない。control markerが複数deltaに分割されたcaseでも全buffer破棄により表示しない。
- 正常なCompletionReport付き応答で表示するのはenvelope検査後のbounded deliverableだけ。Task Completion fail-closed responseでraw assistant contentを流さない。
- `progress`と`final_response`はoverflow有無にかかわらず既存どおり1回転送する。deadlock回避のためinner sink await中にbuffer mutexを保持しない。

### 6.2 Workspace metadata fingerprint

`WorkspaceSnapshot.entries`のtupleをnamed型へし、Unixの`std::os::unix::fs::MetadataExt`から次だけを記録する。

```text
file_type / is_dir / is_symlink
len
dev / ino
mtime / mtime_nsec
ctime / ctime_nsec
```

- `symlink_metadata`を使いsymlinkをfollowしない。regular file / symlink targetのcontentをopen / read / hashしない。
- size / mtimeが同じでもctime nanoseconds、inode、file typeの差をchanged pathにする。
- `read_dir` iterator item errorを`flatten()`で黙って落とさず`incomplete=true`にする。metadata error、strip_prefix不整合、snapshot中raceもincomplete。
- metadata capabilityが不足しnanosecond change timeを信頼できない対象は、変更なしと断定せずincomplete。Unix専用projectなのでWindows fallbackは作らない。
- 4096 observed path到達、256 changed path超過は既存どおりtruncate + incomplete。path順はsort / dedupで決定的にする。
- `external_command_worker`のsecret-bearing path filterと`observation_incomplete` OR合成を維持し、changed-file Evidenceは`verified=false`のまま。

Phase 3 targeted gate:

```bash
cargo test -p aibe --test 0071_task_completion_hardening_red -j 1 -- --test-threads=1
cargo test -p aibe --test 0069_agent_task_delegation_red -j 1 -- --test-threads=1
./scripts/check-spec-acceptance.py
./scripts/check-feature-scope.py
```

## 7. Phase 4 — Regression / docs / completion

### 7.1 回帰

- 0068: Contract固定、assistant claim非Evidence、query budget 2、通常4 outcome。
- 0069: AgentTaskPack disabled、registry、approval、cwd、depth、timeout、kill/reap、output bounds / redaction、metadata-only changed path。
- 0070: parent plan ownership、nonzero→Gap、follow-up 1、same Worker / plan、seven terminal domain、Human Task非統合。
- Human Task: normal Done、Suspended時evaluation skip、checkpoint / resume / continuation、cancel / approval表示。
- 通常turn: Error / Cancelled `completion_report=None`はfieldをserializeせず、exit code / human / structured output不変。
- protocol: old payload→new consumer、new payload→legacy-shape serde fixture、Failed / Cancelled reportのroundtrip。
- presenter: top-level response typeをError / Cancelledのまま表示し、typed terminal / bounded reportを追加表示。control JSON、raw command output、秘密値を表示しない。

### 7.2 docs同期

- `docs/architecture.md`: Error / Cancelled optional report、terminal precedence、Failed matrix、criterion-level required_work、Gap provenance、event buffer / observer境界、JSON schemaを更新。
- `docs/security.md`: control JSON fail-closed、128 KiB report、64 KiB event buffer、metadata-only / content非read、secret-bearing path、raw state非表示を更新。
- `docs/testing.md`: Phase別test、境界値、legacy fixture、same-size rewrite、`-j 1`、mock/smoke役割を更新。
- `docs/manual/0071_task-completion-hardening.md`: cancel before/after Contract、spawn failure、nonzero Gap、overflow非表示、same-size rewrite/incompleteの確認手順を記載。
- protocol docs: `docs/architecture.md`のstdio JSONを正本として`aibe-protocol` DTOと同期。別schema文書を増やさない。

### 7.3 AC10 / smoke

`task_completion_hardening_verify_and_smoke` Rust testは、`smoke-mock.sh`が依存するmock production compositionの通常turnをprocess内で検査する。testから`verify.sh` / `smoke-mock.sh`を再帰起動しない。全targeted test後、完了直前に別processで次を順に実行する。

```bash
./scripts/verify.sh
./scripts/smoke-mock.sh
```

`verify.sh`後は成功・失敗を問わず`.verify-timing-last`のsummaryを完了報告へ転記する。smokeは実API / API key / networkを使わず、既存mock aibe + ai roundtripがError / Cancelled field追加で壊れていないことを確認する。

## 8. AC → テスト対応

| ID | Phase | テスト関数 | file_glob | 現在 | green条件 |
|----|-------|------------|-----------|------|-----------|
| `task_completion_cancel_report_vertical_e2e` | 2 | 同名 | `aibe/tests/0071_task_completion_hardening_red.rs` | pending / ignored | Contract前後cancel、top-level Cancelled、typed report、128 KiB内、control非表示 |
| `verification_infrastructure_failure_reports_failed` | 2 | 同名 | 同上 | pending / ignored | spawn / result / cleanup failureだけFailed。invalid入力と観測可能nonzero/timeoutの対照 |
| `task_completion_hardening_vertical_e2e` | 1 | 同名 | 同上 | pending / ignored | production composition、nonzero Evidence、2 criterion Gap、same Worker follow-up、再検証Done |
| `gap_observed_uses_actual_evidence` | 2 | 同名 | 同上 | pending / ignored | Evidence / typed state由来、required_evidence非流用、stale / unrelated拒否 |
| `gap_required_work_is_per_criterion` | 2 | 同名 | 同上 | pending / ignored | 異なるrequired_work、ID対応、blank / 1024超過拒否 |
| `gap_follow_up_request_is_per_criterion_and_bounded` | 2 | 同名 | 同上 | pending / ignored | criterion別instruction / plan、既存shape + 128 KiB、超過Blocked |
| `completion_event_buffer_has_total_byte_limit` | 3 | 同名 | 同上 | pending / ignored | 64 KiB /1024境界、overflow全破棄、split marker、progress/final |
| `workspace_observer_detects_same_size_rewrite_or_marks_incomplete` | 3 | 同名 | 同上 | pending / ignored | same size/mtime rewriteのctime/inode検出またはincomplete、content非read |
| `task_completion_hardening_preserves_regressions` | 4 | 同名 | 同上 | pending / ignored | 0068–0070 / Human Task /通常turn / protocol回帰と禁止依存 |
| `task_completion_hardening_verify_and_smoke` | 4 | 同名 | 同上 | pending / ignored | mock composition通常turn + 全AC後のverify / smoke別process成功 |

同名testが上表の観測点を持つ実assertionになり非ignoredで成功した同じ変更でだけ`pending=false`へ切り替える。型やbranchの追加だけ、panic削除だけ、常時成功sentinelだけでgreen扱いにしない。補助unit / protocol / presenter testは追加してよいが、registryの1:1 testを置換しない。

## 9. Mock / 手動検証

実API、API key、network、実Agent製品を使わない。fixture modeは最低限次を決定的に返す。

- command process開始後exit nonzero
- executable missingによるspawn failure
- timeout / signal + cleanup成功と、test adapterによるresult acquisition / cleanup failure
- Worker初回未達、follow-up instructions内の2 criterion Gap確認後に修正
- same-size / same-mtime file rewrite（ctime / inode差を観測）
- streaming delta 64 KiBちょうど / +1 byte、1024件 / 1025件、control marker分割

実装中のmock導通:

```bash
cargo test -p aibe --test 0071_task_completion_hardening_red -j 1 -- --test-threads=1
cargo test -p aibe-protocol -j 1
```

手動checklistは自動testで再現しにくいhuman表示とfilesystem capabilityを確認する。未実施なら完了報告の残リスクへ明記する。手動検証でAPI keyをrepo / logへ書かず、workspace外pathや秘密fileをfixtureに使わない。

## 10. Pack Composition

**No** — 設計書 §8どおり、0071はoptional機能束ではなく既存Task Completion対象turnの安全不変条件を補正するhardeningである。新しいPack trait、Active / Basic Pack、runtime toggle、Cargo feature、composition rootを追加しない。0069 `AgentTaskPack` disabled時はAgent Task tool自体が非公開となる既存挙動を維持する。

## 11. Security / compatibility 不変条件

- assistant schema / required_work / Evidence IDはuntrusted inputであり、bounded・集合・provenanceをコード検査する。
- `required_evidence`を`observed`として表示しない。Worker report / exit 0をverifiedへ昇格しない。
- command exact-match / started / terminal stateをtypedに保持し、文字列解析でFailed / Gapを決めない。
- reportはterminalと全criterion rowを優先し、raw provider/tool outputを複製しない。
- event overflow時は「安全そうなprefix」も再表示せず全deltaを捨てる。
- observerはmetadataだけを読み、file content / digest / symlink targetを読まない。
- optional reportなしのError / Cancelled JSONを変えず、Human Task /通常turnへTask Completion reportを誤添付しない。
- `ai` presenterは判定を行わずwire DTOを表示するだけ。`ai`がLLM APIやretry loopを持たない。

## 12. STOP-THE-LINE / やってはいけないこと

0071のComplexity budgetは全項目`+0`である。次が必要になった時点で実装を止める。

- 新actor、状態機械、provider query、agent loop、Worker、background observer / watcher
- query budget増、command plan item増、follow-up 2回以上、複数 /並列Worker
- report / Gap / snapshot / bufferの永続化、journal、checkpoint、schema migration
- lease / heartbeat、reconciler、idempotency、exactly-once、crash recovery
- content hashing、workspace index、長期監視、PR / CI adapter
- expected-success Predicate / DSL、意味coverage checker、verifier / critic Agent
- Human Task cross-suspend verification、Human / Agent共通aggregate
- 新Pack / toggle / Cargo feature / RPC / process boundary
- actors 4、state machines 3、persistent aggregates 3、external effects 5、process boundaries 3、integrations 4、novel mechanisms 1以上となる変更

STOP-THE-LINE報告:

```text
STOP-THE-LINE

分類: BLOCKER_ORIGINAL_AC / REGRESSION / SAFETY_WITHIN_FAULT_MODEL / NEW_REQUIREMENT / HARDENING / OUT_OF_FAULT_MODEL
発見した要因:
現在scopeへの影響:
Complexity Gateの変化:
MVPからの削除案:
別spec案:
```

禁止:

- nonzero / timeoutを簡単にFailedへまとめない
- malformed schemaをFailedへ再分類しない
- observedへrequired_evidence / global next_objectiveを流用しない
- criterion / safety instructionをtruncationしてfollow-upを続けない
- control JSONをoverflow prefixから再表示しない
- same-size検出のためfile内容をhashしない
- ignored testや`pending=true`を残したまま完了扱いにしない
- `verify.sh`成功前に本書を`docs/done/`へ移さない

## 13. 完了条件と実装順

最初の着手点は`aibe/tests/0071_task_completion_hardening_red.rs::task_completion_hardening_vertical_e2e`を観測可能な実REDへ置換することである。

1. Scope Lock / registry checker確認
2. Phase 1 vertical RED → domain/application最小変更 → GREEN
3. Phase 1 ignore / pending解除
4. Phase 2 terminal / protocol / Gap RED → GREEN
5. Phase 2 ignore / pending解除
6. Phase 3 buffer / observer RED → GREEN
7. Phase 3 ignore / pending解除
8. Phase 4 regressions / docs / manual / mock
9. 全10 AC非pending・非ignored、targeted checker成功
10. `./scripts/verify.sh`、timing summary記録
11. `./scripts/smoke-mock.sh`
12. 成功後だけstatus=`done`、本書を`docs/done/`へ移動、indexを実装済みへ更新

完了には次をすべて満たすこと。

1. 全10 locked ACが同名testでgreen、`pending=false`、`#[ignore]`なし
2. vertical sliceがproduction compositionを通り、query 2 / Worker 2 / follow-up 1 / nonzero Gap /再検証Doneを観測
3. 0068 / 0069 / 0070 / Human Task /通常turn回帰なし
4. protocol / architecture / security / testing / manual同期
5. `check-feature-scope.py` / `check-spec-acceptance.py`成功
6. `verify.sh`成功と`.verify-timing-last` summary転記
7. `smoke-mock.sh`成功
8. 上記後だけdone移動 / status / index更新

## 14. 仕様との差分 / 未確定事項

仕様との差分はない。本書と実装が矛盾する場合は設計書 0071 を優先する。

- **推測:** optional `completion_report`は設計書 §13どおり、既存top-level Error / Cancelled variantへ追加し、legacy`outcome=blocked` + typed terminalで区別する。新response variantは作らない
- **推測:** filesystemがUnix metadata nanosecondsを提供してもその能力を信頼できないとadapterが判定した場合は、changedなしではなく`observation_incomplete=true`を優先する
- **未確定:** result acquisition / cleanup / reap failureを現行runnerのどのtyped errorへ分離できるか。文字列分類が必要になるなら実装を止め、既存process portの最小typed拡張がscope内か`SAFETY_WITHIN_FAULT_MODEL`として再判定する
- **未確定:** manualで利用可能なlocal Worker設定。利用不能ならdeterministic automated fixtureを正本とし、manual未実施を残リスクへ記載する
