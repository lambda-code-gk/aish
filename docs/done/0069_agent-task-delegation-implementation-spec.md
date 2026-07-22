# 0069 Agent Task Delegation 実装指示書

> **種別**: 実装指示書（`docs/done/`）  
> **状態**: 実装済み（Phase 1–3、全 AC 緑、verify + smoke 成功）  
> **正本**: [`0069_agent-task-delegation-spec.md`](../spec/0069_agent-task-delegation-spec.md)  
> **関連**: [`0068_task-completion-phase1-spec.md`](../spec/0068_task-completion-phase1-spec.md)、[`0062_collab-mode-human-task-tool-spec.md`](../spec/0062_collab-mode-human-task-tool-spec.md)、[`0026_external-commands-spec.md`](../spec/0026_external-commands-spec.md)、[`0045_pack-composition-spec.md`](../spec/0045_pack-composition-spec.md)、[`feature-development-policy.md`](../feature-development-policy.md)

## 0. 目的

設計書 0069 を正本として、親 Query Loop が構造化 `agent_task` を設定済み外部 Worker へ一度だけ同期委譲し、本番の `ExternalCommandWorker` 経路から bounded な `AgentTaskResult` と出所付き Evidence を同じ親へ返す。Worker の正常終了、完了自己申告、ファイル変更のいずれも検証済みとは扱わず、top-level Result と全 Evidence を常に `verified=false` とする。

0069 固有の provider loop、conversation、永続 lifecycle、修正反復、非同期 job は作らない。`human_task` は独立した既存機能として非破壊に保ち、Agent/Human 共通 aggregate へ統合しない。

## 0.1 Scope Lock

- Feature scope registry: `scripts/feature-scope.toml`
- Status: `locked`
- Scope revision: `2`
- Complexity class: Yellow（`scope_review = "approved"`）
- Vertical slice AC ID: `agent_task_vertical_e2e`
- Locked AC IDs:
  - `agent_task_vertical_e2e`
  - `agent_task_request_is_strictly_validated`
  - `agent_task_registry_and_disabled_pack_fail_closed`
  - `agent_task_runs_in_validated_cwd_with_timeout`
  - `agent_task_result_normalizes_worker_outcomes`
  - `agent_task_evidence_is_bounded_and_unverified`
  - `agent_task_recursion_is_rejected`
  - `agent_task_approval_cannot_be_bypassed`
  - `agent_task_integrates_with_task_completion_as_unverified`
  - `agent_task_preserves_human_task_behavior`
  - `agent_task_core_is_product_agnostic_and_mockable`

Scope Lock 後に 0069 をブロックできるのは `BLOCKER_ORIGINAL_AC` / `REGRESSION` / `SAFETY_WITHIN_FAULT_MODEL` だけである。`NEW_REQUIREMENT` / `HARDENING` / `OUT_OF_FAULT_MODEL` は本書へ追加せず、設計書 §10 の Deferred または別 spec へ送る。

## 0.2 実装固定値

request と結果は parse 後ではなく、可能な限り deserialize / 読み取り中から上限を適用する。文字列上限は UTF-8 byte 数とする。

| 項目 | 上限・規則 |
|------|------------|
| Worker ID | 1–64 bytes、ASCII lowercase / digit / `._-`、先頭は lowercase または digit |
| Objective | 1–4096 bytes |
| Instructions | 1–32件、各 1–2048 bytes、合計 16 KiB 以下 |
| Completion Criteria | 1–32件。一意 ID は 1–64 bytes、description は 1–2048 bytes、合計 32 KiB 以下 |
| cwd input | 省略可、指定時 1–4096 bytes。NUL 禁止 |
| timeout | 1–1800秒かつ Worker 設定上限・server 上限以下。0 / 超過は補正せず拒否 |
| stdout / stderr | stream ごとに既存 `MAX_TOOL_OUTPUT_BYTES` 以下へ sanitize / truncate し、`truncated` を保持 |
| summary / Evidence summary | 各 1024 bytes 以下 |
| Evidence | 最大256件。changed path は最大256件、各 path 4096 bytes 以下 |

JSON Schema は `additionalProperties=false` とし、`worker` / `objective` / `instructions` / `completion_criteria` 以外から command 実行能力を与えない。`cwd` と `timeout_secs` は上表の範囲だけを受理する。executable、argv、shell string、environment、permission profile、approval state、delegation depth は LLM schema に置かない。

## 1. Phase 分割

| Phase | 内容 | 対応 AC | ゲート |
|-------|------|---------|--------|
| 1 | Domain / Worker Port / Active・Basic Pack / application service / dedicated approval の最小実装を本番 composition へ接続し、deterministic fixture Worker を一回同期実行する Vertical Slice | `agent_task_vertical_e2e`、`agent_task_request_is_strictly_validated`、`agent_task_registry_and_disabled_pack_fail_closed`、`agent_task_core_is_product_agnostic_and_mockable` | 4件を実 assertion に置換し、`#[ignore]` 解除と同じ変更で `pending=false`。特に Vertical Slice が緑になるまで Phase 2 へ進まない |
| 2 | cwd / timeout / kill-reap、全 Worker outcome、bounded Evidence、再帰拒否、approval 迂回拒否、0068 統合、`human_task` 回帰 | 残る7 AC | 7件を緑にし、0069 の全11件を `pending=false` にする |
| 3 | architecture / security / testing / manual docs 同期、mock・local smoke、全体検証、完了処理 | 新規 AC は追加しない | `./scripts/verify.sh` 成功後のみ `docs/done/` へ移動する |

**Vertical Slice Gate**: Phase 1 は mock service 直呼びでは完了しない。scripted parent LLM → production tool registry → `AgentTaskTool` → production `AgentTaskService` → production `AgentTaskWorkerRegistry` → `ExternalCommandWorker` → fixture subprocess → Result / Evidence → 同じ親 Query Loop、という一本を通す。

**Phase 2 先行禁止**: Vertical Slice 成功前に並列化、非同期化、永続化、独立 verifier、repair loop、汎用 delegation framework、Human/Agent lifecycle 統合、OS sandbox を実装しない。

## 2. レイヤー境界と変更対象

パスは現行構成に沿う推奨配置である。既存 module へ統合する場合も、以下の依存方向と責務は変えない。

### 2.1 `aibe` domain

| パス | 変更 | 責務 |
|------|------|------|
| `aibe/src/domain/agent_task.rs`（新規） | request / validated request / result / evidence / worker ID / status / depth を定義 | 純粋な schema invariant、上限、status 整合、`verified=false` 不変条件。process、filesystem、製品名を参照しない |
| `aibe/src/domain/mod.rs` | domain 型を re-export | application / tests が leaf 型だけを参照できるようにする |
| `aibe/src/domain/task_completion.rs` | Agent Task Evidence を取り込む最小拡張 | `source=agent_task` を未検証 Evidence として表現し、既存 Done 判定を緩めない |
| `aibe/src/domain/tool.rs` / `tool_execution_summary.rs` | `AGENT_TASK` 名、専用 risk / audit metadata を必要最小限追加 | `shell_exec` と別の approval 状態・provenance を保持する |

最低限の型は `AgentTaskRequest`、`ValidatedAgentTaskRequest`、`CompletionCriterion`、`WorkerId`、`DelegationDepth`、`AgentTaskResult`、`AgentTaskStatus`、`AgentTaskEvidence`、`AgentTaskEvidenceKind`、`AgentTaskEvidenceSource` とする。domain constructor / validator は次を fail-closed にする。

- 空、重複 criterion ID、未知 field、件数・byte 上限超過
- `timed_out=true` と `status!=timed_out` などの矛盾
- failure status を success / `reported_complete` へ昇格する入力
- top-level または Evidence に `verified=true` を持ち込む入力
- depth 1 以上からの委譲

製品固有の structured output は domain へ直接 deserialize しない。adapter の parser が untrusted output を読み、application 共通 DTO へ正規化する。

### 2.2 `aibe` ports

| パス | 変更 | 責務 |
|------|------|------|
| `aibe/src/ports/outbound/agent_task_worker.rs`（新規） | `AgentTaskWorker` と worker error/output を定義 | application から opaque Worker を一回同期実行する唯一の新規 effect boundary |
| `aibe/src/ports/outbound/agent_task_registry.rs`（新規） | `AgentTaskWorkerRegistry` lookup interface | `WorkerId` から `Arc<dyn AgentTaskWorker>` を取得。未知 ID は `None`、重複は構築時 error |
| `aibe/src/ports/outbound/config.rs` | Agent Task / Worker 設定 DTO を追加 | enabled、ID、固定 executable / argv template、timeout 上限、permission profile、環境名 allowlist、output format を保持 |
| `aibe/src/ports/outbound/tool_context.rs` | request-local depth / Agent Task eligibility を追加 | `base_dir` / `resolve_path` を cwd 解決の起点にし、depth を process spawn 前に検査可能にする |
| `aibe/src/ports/outbound/tool_approval.rs` | generic approval 境界を Agent Task prompt / decision / audit に拡張 | worker / normalized cwd / effective timeout / permission profile / 残余 risk を表示し、shell approval と別 decision を返す |

`AgentTaskWorker` 以外の汎用 effect framework を新設しない。approval は既存同一 socket 往復を拡張し、新しい socket / RPC service / approval daemon は作らない。

### 2.3 outbound adapters

| パス | 変更 | 責務 |
|------|------|------|
| `aibe/src/adapters/outbound/agent_task/external_command_worker.rs`（新規） | `ExternalCommandWorker` | 設定済み executable / argv、stdin envelope、固定 permission profile / env allowlist、structured-output parser、共通 subprocess policy を所有 |
| `aibe/src/adapters/outbound/agent_task/registry.rs`（新規） | immutable registry builder | 明示設定だけを登録し、empty / duplicate / invalid worker を fail-closed にする |
| `aibe/src/adapters/outbound/agent_task/workspace_observer.rs`（新規） | bounded 前後 metadata snapshot | cwd 配下の path / file type / size / mtime 等を bounded に観測し、内容や symlink target を読まず changed path を正規化する |
| `aibe/src/adapters/outbound/tools/agent_task.rs`（新規） | `ToolExecutor` adapter | JSON tool arguments を parse し、application service を呼び、bounded tool result / execution summary を返す |
| `aibe/src/adapters/outbound/tools/subprocess.rs` | 必要な場合のみ process-group cleanup を共通化 | `shell_exec` と Agent Task が timeout 時の group kill / reap を共有できるようにする。Agent Task 用コピーを作らない |
| `aibe/src/adapters/outbound/toml_config.rs` | Agent Task 設定 parse / validation | executable / argv / env / profile は config からだけ受理し、LLM request と merge しない |

`ExternalCommandWorker` は shell string や `sh -c` を組み立てず、executable と argv を分離して起動する。stdin envelope には `schema_version=1`、objective、instructions、completion criteria、canonical cwd、`delegation_depth=1` を含める。Worker output の `reported_complete` は「構造化応答を正常に parse した」という status に過ぎない。

既存 `[[external_commands]]` は `shell_exec` 用であり Agent Task Worker registry の正本にしない。process 起動・timeout/kill/reap・sanitize の方針だけを再利用する。Agent Task 設定が disabled / empty のとき shell allowlist から Worker を自動生成してはならない。

### 2.4 application

| パス | 変更 | 責務 |
|------|------|------|
| `aibe/src/application/agent_task.rs`（新規） | `AgentTaskService` | validation → depth → registry → cwd → approval → before snapshot → worker → after snapshot → normalize の順序を一回だけ実行 |
| `aibe/src/application/agent_task_pack.rs`（新規） | `AgentTaskPack`、Active / Basic Pack | registry と tool publication policy だけを差し替える薄い Pack 境界 |
| `aibe/src/application/tool_defs.rs` / `tool_round/executor.rs` | conditional publication / execution | Active、request eligibility、depth 0 の積集合でだけ公開。forged call でも service 側で再検査 |
| `aibe/src/application/task_completion.rs` | 0068 Evidence bridge | Agent Task Result を `source=agent_task`, `verified=false` で ledger へ追加。別 query / verifier を起動しない |
| `aibe/src/application/request_service.rs` | protocol context から eligibility / depth を構成 | 通常 Query Loop を維持し、Agent Task 専用 loop を作らない |

処理順序は security invariant として test で固定する。

```text
strict request decode
→ request上限・depth・publication eligibility
→ registry lookup
→ ToolExecutionContext::base_dir 基準のcwd解決・canonicalize・認可root検査
→ agent_task固有approval（deny/unavailable/timeoutはspawn 0）
→ bounded before snapshot
→ Workerを一回同期実行
→ bounded after snapshot
→ status / output / Evidenceをfail-closed正規化
→ top-levelと全Evidenceをverified=falseに固定
→ 同じ親Query Loopへtool resultを返す
```

cwd は既存の認可済み root 群と `ToolExecutionContext::base_dir` を使い、存在、directory、canonical path、root containment を実行直前にも検査する。cwd 検査は Worker の OS-level filesystem sandbox ではないことを approval 表示と docs に残す。

### 2.5 composition / Pack Composition

| パス | 変更 | 責務 |
|------|------|------|
| `aibe/src/application/server.rs` | composition root 1か所で Pack を選択 | enabled + valid non-empty Worker config のときだけ Active、disabled のとき Basic を注入 |
| `aibe/src/adapters/outbound/tools/mod.rs` / registry builder | Pack publication policy を tool registry へ反映 | Basic では `agent_task` definition / executor とも非公開 |

部分適用 Pack の固定事項:

- `AgentTaskPack` が返すのは registry と publication policy だけ
- Active は明示設定 Worker のみ、Basic は empty registry + tool 非公開
- forged allowlist / forged tool call は Basic service 境界でも拒否
- runtime toggle は aibe config の一か所で解決し、`ai` Pack や Cargo feature は増やさない
- core validation / Result normalization は Pack の外に置く

### 2.6 protocol / `ai` CLI

| パス | 変更 | 責務 |
|------|------|------|
| `aibe-protocol/src/request.rs` | delegation depth の後方互換 field、Agent Task approval decision | depth 省略は0。client が true や approval state を自由入力できる schemaにはしない |
| `aibe-protocol/src/response.rs` | Agent Task approval prompt DTO | worker / cwd / timeout / profile / trust-boundary warning を bounded に送る |
| `aibe-client` の callback DTO / validation | Agent Task approval 往復 | prompt ID / turn ID / tool call ID / origin を既存同様に照合 |
| `aibe/src/adapters/inbound/connection_approval.rs` | 同一 connection の専用 approval gate | shell / file write decision と混同せず、cancel / timeout / malformed response を拒否 |
| `ai/src/adapters/outbound/agent_task_approval_ui.rs`（新規または既存 approval UI へ追加） | 対話 approval 表示 | Worker 内部操作は AISH が個別承認しないこと、cwd は sandbox でないことを表示 |
| `ai/src/adapters/outbound/aibe_client.rs` | callback を接続 | `--yes-exec` を Agent Task approval に渡さない |
| `ai/src/domain/request_context.rs` | delegated child の depth を wire へ反映 | `ExternalCommandWorker` が固定した内部環境 / envelope から depth 1 を伝播し、公開 CLI で depth を下げる手段を作らない |

Agent Task 用の自動承認 CLI flag は追加しない。非対話 stdin、UI unavailable、malformed decision は実行拒否とする。`--yes-exec`、`shell_exec` allowlist、Worker の argv / stdout / JSON field は Agent Task approval を満たさない。

`aish` は変更しない。LLM、Worker、approval、delegation depth を shell launcher / logger へ持ち込まない。

## 3. Phase 1 実装手順 — Minimum Vertical Slice

1. `aibe/tests/0069_agent_task_delegation_red.rs` の Phase 1 4件を、`panic!(PENDING)` ではなく観測可能な失敗 assertion を持つ RED test へ置換する。実装が通るまでは `#[ignore]` / `pending=true` を維持する。
2. domain 型と table test を追加し、strict schema、上限、重複 ID、status 整合、`verified=false` constructor を固定する。
3. `AgentTaskWorker` Port、immutable registry、recording `MockWorker` を追加する。Mock は request / context を記録し、success / failure / timeout / malformed-output 相当の決定的結果を process なしで返す。
4. `AgentTaskPack`、Active / Basic を実装し、Basic の registry empty、tool 非公開、forged call 拒否を test する。
5. Agent Task 専用 approval prompt / decision を既存 socket approval 往復へ追加する。Phase 1 の approved happy path でも decision の出所を audit metadata に残す。
6. `AgentTaskService` と `AgentTaskTool` を上記固定順序で接続する。Phase 1 では valid cwd、正常 fixture、bounded な正常出力を一本通す。
7. `aibe/tests/fixtures/0069_agent_task_worker.sh` を追加する。fixture は stdin の version / objective / criteria / cwd / depth を検査し、設定済み argv で選んだ決定的 mode に従って cwd 内へ一つのファイルを書き、schema version 1 の JSON を stdout へ返す。API key、network、実 Agent 製品を使わない。
8. fixture executable / argv / timeout / profile は production config parser と production registry builder を通す。test 専用 facade や `MockWorker` で `agent_task_vertical_e2e` を代替しない。
9. scripted parent LLM の tool call と fixture result を同じ Query Loop で往復させ、`AgentTaskResult.verified=false` と全 Evidence の false、cwd のファイル変更、実行1回を検証する。
10. Phase 1 4 test を緑にして `#[ignore]` を外し、同じ変更で対応する registry 4件だけを `pending=false` にする。

Phase 1 targeted gate:

```bash
cargo test -p aibe --test 0069_agent_task_delegation_red agent_task_request_is_strictly_validated -j 1 -- --exact
cargo test -p aibe --test 0069_agent_task_delegation_red agent_task_registry_and_disabled_pack_fail_closed -j 1 -- --exact
cargo test -p aibe --test 0069_agent_task_delegation_red agent_task_core_is_product_agnostic_and_mockable -j 1 -- --exact
cargo test -p aibe --test 0069_agent_task_delegation_red agent_task_vertical_e2e -j 1 -- --exact
./scripts/check-spec-acceptance.py
./scripts/check-feature-scope.py
```

## 4. Phase 2 実装手順 — Fault handling / integration

1. 残る7 ignored test を、各 AC の拒否位置・spawn 回数・結果 status・audit / Evidence を観測する RED test へ置換する。
2. cwd table test で relative / omitted / root内absolute を許可し、`..` escape、root外absolute、missing、file、canonicalize 後の symlink escape を spawn 前に拒否する。
3. fixture の設定済み mode を別 Worker config として使い、non-zero、timeout + child process、launch failure、malformed JSON、large stdout/stderr を試験する。mode を LLM arguments から選ばせない。
4. 共通 subprocess helper を process-group 単位の有限 kill / reap に揃え、timeout 後に子孫 process が残らないことを PID / sentinel で検証する。cleanup error を success に変換しない。
5. before / after snapshot を bounded にし、作成・変更・削除 path を cwd 相対で列挙する。symlink を辿らず、file content、認証情報、root外 path を Evidence に収集しない。観測上限到達は truncated / incomplete として明示し、完全観測を主張しない。
6. Worker report、workspace observation、process output、exit status を別 provenance に正規化する。exit 0 + `reported_complete` + changed path の組合せでも `verified=false` を維持する。
7. `delegation_depth=1` を child environment と stdin envelope に固定し、delegated context では definition 非公開、forged `agent_task` は registry lookup / approval / spawn より前に拒否する。
8. approval matrix を試験する。explicit UI yes だけが起動可能で、UI no / unavailable / timeout / cancel / malformed、`--yes-exec`、shell allowlist、Worker output の approved claim は spawn 0 とする。audit に bounded worker / cwd / timeout / decision origin を残す。
9. 0068 ledger へ Agent Task Evidence を追加し、Worker 自己申告のみでは criterion satisfied / Done にならないこと、親の後続 read-only verification が別 Evidence になることを検証する。
10. 0062–0066 の `human_task` tests と 0068 tests を回し、公開条件、Human Shell、checkpoint / resume / continuation、Evidence、approval が不変であることを固定する。`human_task` module / protocol の意味を Agent Task 都合で変えない。
11. 残る7 test を緑にして `#[ignore]` を外し、同じ変更で対応 registry を `pending=false` にする。11件すべてが非 pending になるまで完了扱いにしない。

Phase 2 targeted gate:

```bash
cargo test -p aibe --test 0069_agent_task_delegation_red -j 1 -- --test-threads=1
cargo test -p aibe --test 0068_task_completion_phase1_red -j 1 -- --test-threads=1
cargo test -p aibe --test 0062_collab_mode_human_task_tool -j 1 -- --test-threads=1
./scripts/verify-targeted.sh --package aibe --test 0069_agent_task_delegation_red
./scripts/check-spec-acceptance.py
./scripts/check-feature-scope.py
```

## 5. 受け入れ条件レジストリ

| ID | Phase | 現在 | 解除条件（設計書 AC と 1:1） |
|----|-------|------|--------------------------------|
| `agent_task_vertical_e2e` | 1 | green / active | scripted 親 Query Loop から production registry / adapter / subprocess と fixture を一回通し、指定 cwd、Result + Evidence、全 `verified=false`、同じ親への復帰を検証 |
| `agent_task_request_is_strictly_validated` | 1 | green / active | 必須値、固定上限、unknown field、worker、cwd、timeout を spawn 前に検査し、schema に executable / argv / env / profile / approval がないことを検証 |
| `agent_task_registry_and_disabled_pack_fail_closed` | 1 | green / active | allowlisted config だけを登録し、unknown / duplicate / disabled / empty / forged publication を拒否、Basic で definition 非公開を検証 |
| `agent_task_core_is_product_agnostic_and_mockable` | 1 | green / active | core/application の製品名・command 非依存と、recording MockWorker の success / failure / timeout / malformed outcome を process なしで検証 |
| `agent_task_runs_in_validated_cwd_with_timeout` | 2 | green / active | root 内 canonical directory だけで実行し、escape / missing / non-directory / symlink escape を拒否、timeout で process group を有限 kill / reap |
| `agent_task_result_normalizes_worker_outcomes` | 2 | green / active | stdout / stderr / exit / timeout / launch / malformed output の status と truncation を共通 Result へ正規化し、failure を success にしない |
| `agent_task_evidence_is_bounded_and_unverified` | 2 | green / active | changed file / report / output / exit を別 provenance・件数/byte上限付きで返し、top-level と全項目が false。内容・symlink target は収集しない |
| `agent_task_recursion_is_rejected` | 2 | green / active | depth 0 の一回だけを許可し、depth 1 では非公開かつ forged call を registry / approval / spawn 前に拒否 |
| `agent_task_approval_cannot_be_bypassed` | 2 | green / active | 専用 explicit approval だけを許可し、shell allowlist / auto approval / `--yes-exec` / Worker 入出力による迂回を拒否、audit field を検証 |
| `agent_task_integrates_with_task_completion_as_unverified` | 2 | green / active | 0068 ledger が `source=agent_task` / false で取り込み、自己申告だけで satisfied / Done にせず、親の後続観測を別 Evidence にする |
| `agent_task_preserves_human_task_behavior` | 2 | green / active | 0062–0066 の公開、shell、checkpoint / resume / continuation、Evidence、approval 回帰と、共通 lifecycle aggregate 不在を検証 |

`pending=false` は stub が消えた時点ではない。同名 test が上表の本番経路または指定境界を実 assertion で検証し、非 ignored で成功した同じ変更でだけ切り替える。

## 6. Mock / ローカル正常系コマンド（Step 6 用）

実 API / API key / network / 実 Agent 製品を必要条件にしない。実装後、次の順で正常系を再現できること。

```bash
# Port / domain の process なし smoke
cargo test -p aibe --test 0069_agent_task_delegation_red agent_task_core_is_product_agnostic_and_mockable -j 1 -- --exact

# Active / Basic Pack の publication smoke
cargo test -p aibe --test 0069_agent_task_delegation_red agent_task_registry_and_disabled_pack_fail_closed -j 1 -- --exact

# production ExternalCommandWorker + deterministic fixture の正常系
cargo test -p aibe --test 0069_agent_task_delegation_red agent_task_vertical_e2e -j 1 -- --exact --nocapture

# 0068 への未検証 Evidence 取り込み
cargo test -p aibe --test 0069_agent_task_delegation_red agent_task_integrates_with_task_completion_as_unverified -j 1 -- --exact

# human_task 非破壊回帰
cargo test -p aibe --test 0069_agent_task_delegation_red agent_task_preserves_human_task_behavior -j 1 -- --exact
```

fixture の直接起動を受け入れ条件にしない。必須 smoke は production config parser / composition / Worker adapter を通る test command であり、fixture script 単体成功を本番経路成功の代用にしない。

## 7. Security / 不変条件

- `human_task` は非破壊: Human Shell、briefing、checkpoint、resume、continuation、Human Task Evidence、approval の既存型・状態遷移・公開条件を変更しない。共通化は既存 tool publication / request context / approval transport の薄い境界まで。
- 再帰禁止: aibe 管理 depth は最大1。depth 1 以上は definition 非公開かつ forged call を spawn 前拒否。child へ depth 1 を渡すが、悪意ある Worker が環境を破棄するケースは OS sandbox を導入せず保証外として表示する。
- `verified=false`: Worker 自己申告、exit 0、changed path、fixture 成功でも true にしない。true を受理する request/output schema、constructor、adapter shortcut を作らない。
- approval 非迂回: Agent Task 固有 approval を必須とし、`shell_exec` decision / allowlist / `--yes-exec` / Worker flag / output を流用しない。deny / unavailable / timeout / malformed は fail-closed。
- secrets: argv / env 値 / raw structured output / file content を audit、Evidence、error、tracingへ複製しない。環境は設定済み名前 allowlist のみを継承し、値を表示しない。
- cwd: canonical root containment は起動位置と観測範囲の保証であり Worker の filesystem access 上限ではない。この残余 risk を approval prompt と `docs/security.md` に明記する。
- output: stdout / stderr は読みながら bound し、pipe deadlock を避ける。truncate 前の全文を別 buffer / trace に保持しない。

## 8. ドキュメント同期

Phase 1–2 の実装と同じ変更で次を同期する。

- `docs/architecture.md`: Agent Task domain / Worker Port / Active・Basic Pack / composition root、親 Query Loop 内の同期経路、config schema、cwd / depth、0068 Evidence bridge、`ai` / `aibe` / `aish` 境界
- `docs/architecture.md` の stdio JSON schema: Agent Task approval prompt / decision、RequestContext depth の後方互換既定、Result / Evidence schema
- `docs/security.md`: Worker trust boundary、専用 approval、cwd 非 sandbox、env allowlist、bounded output / workspace observation、`verified=false`、再帰保証と保証外
- `docs/testing.md`: MockWorker、scripted parent LLM、fixture subprocess、process-group cleanup test、直列 `-j 1` 方針
- `docs/manual/0069_agent-task-delegation.md`: 実 Worker 製品を任意で確認する場合の設定、approval 表示、cwd、Result / Evidence、cleanup、秘密値非表示のチェックリスト。実施は自動 AC の条件にしない
- `docs/0000_spec-index.md`: 実装中は tasks 行を維持し、全 AC + verify 成功後のみ done へ更新

## 9. STOP-THE-LINE / Complexity budget

0069 の Complexity budget は全項目 `+0` である。実行主体3、状態機械2、永続 aggregate 0、外部副作用3、process boundary2、新規機構1、integration3を増やさない。

次が必要と判明した時点で実装を停止し、0069 へ足さない。

- aibe 管理の secondary agent loop、別 conversation、二つ目の新規状態機械
- 並列 Worker、async queue、background supervisor、fan-out / fan-in
- task / result / session の永続化、checkpoint / resume、schema migration
- depth 2 以上、delegation graph、循環検出
- independent verifier / critic、repair / retry loop
- lease / heartbeat、reconciler、journal、idempotency、exactly-once、crash recovery
- PR / CI / GUI orchestration、Human/Agent lifecycle aggregate
- process boundary 3以上、external effects 5以上、integration 4以上、novel mechanism 2以上
- malicious Worker を閉じ込める OS-level sandbox

報告形式:

```text
STOP-THE-LINE

発見した要因:
現在のscopeへの影響:
Complexity判定の変化:
削除案:
別spec案:
```

継続が必要なら、先に設計書を更新し、`scope_revision` を増やし、Complexity Gate を再判定する。Scope Lock 済み AC を黙って追加・削除・言い換えしない。

## 10. 完了条件

1. 全11 locked AC が同名 test で緑、`pending=false`、`#[ignore]` なし
2. Vertical Slice が production `ExternalCommandWorker` + deterministic fixture subprocess を通り、MockWorker や fixture 直呼びで代替されていない
3. timeout / kill / reap、cwd escape、approval bypass、depth 1 forged call、large/malformed output が fail-closed
4. top-level Result と全 Evidence が常に `verified=false`、0068 が自己申告だけで Done にしない
5. 0062–0066 `human_task` と 0068 Task Completion の regression がない
6. architecture / security / testing / protocol docs が実装と同期し、必要な manual 手順と実施状況を記録
7. `./scripts/verify.sh` 成功。完了報告へ `.verify-timing-last` の timing summary を転記
8. 上記の後だけ `scripts/feature-scope.toml` を `done`、本書を `docs/done/` へ移動し、index を実装済みにする

## 11. 仕様との差分

なし。本書と実装が矛盾する場合は `docs/spec/0069_agent-task-delegation-spec.md` を優先する。簡略化、別 algorithm、mock-only 経路、空 result による成功は認めない。

## 12. 未確定事項

- **未確定**: Agent Task 設定の TOML section / field 名。既存 `[[external_commands]]` と分離し、設計書の Worker config 要件を満たす限り実装時に固定する。固定後は `docs/architecture.md` と config parser tests を同じ変更で更新する。
- **未確定**: generic ToolApproval wire variant の拡張か Agent Task 専用 variant の追加か。いずれも専用 risk / prompt / decision / audit を保持し、`--yes-exec` と shell decision を流用しないことを選択基準とする。新 socket / RPC は不可。
- **推測**: delegated child depth は `ExternalCommandWorker` が設定する予約済み環境値と stdin envelope の両方から `ai` の RequestContext へ伝播する構成を第一候補とする。公開 CLI から depth を下げる option は作らない。
