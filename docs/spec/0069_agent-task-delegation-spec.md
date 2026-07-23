# 0069 Agent Task Delegation 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定（実装済み）  
> **起票**: 2026-07-23  
> **関連**: GitHub Issue #24（Parent: #17 / Depends on: #18）、[`0068_task-completion-phase1-spec.md`](0068_task-completion-phase1-spec.md)、[`0062_collab-mode-human-task-tool-spec.md`](0062_collab-mode-human-task-tool-spec.md)、[`0026_external-commands-spec.md`](0026_external-commands-spec.md)、[`docs/feature-development-policy.md`](../feature-development-policy.md)、[`0045_pack-composition-spec.md`](0045_pack-composition-spec.md)

## 0. Core outcome

親 Agent が Task Completion の部分作業を構造化された `agent_task` として設定済み外部 Agent へ同期委譲し、実行結果と出所付き Evidence を `verified=false` で受け取れる。

## 1. Minimum vertical slice

```text
親 Query Loop
→ agent_task tool call
→ AgentTaskRequest の schema・worker・cwd・depth・timeout・approval を検査
→ AgentTaskWorkerRegistry から設定済み Worker を選択
→ 検査済み cwd で ExternalCommandWorker を同期実行
→ Objective / Instructions / Completion Criteria を渡す
→ stdout / stderr / exit code / timeout と変更ファイル観測を取得
→ AgentTaskResult + Evidence に正規化（常に verified=false）
→ tool result として同じ親 Query Loop へ返す
→ 0068 Evidence ledger が未検証 Evidence として取り込む
```

初期対象は単一 Worker、同期実行、委譲深度 1、設定済み external command、構造化結果、cwd 検証、timeout に限定する。試験 adapter には `MockWorker` を用いる。`agent_task` は親 Query Loop の通常の tool call であり、aibe 内に別の provider loop、会話履歴、planner、修正反復 loop を作らない。

親 Agent の選択肢は次の三つである。

| 選択 | 経路 | 意味 |
|------|------|------|
| Do | 既存 tools | 親 Agent がその場で作業する |
| Delegate to Human | 既存 `human_task` | 人間の shell handoff と既存 lifecycle を使う |
| Delegate to Agent | 新規 `agent_task` | 設定済み外部 Worker を一回同期実行する |

## 2. Fault model

### 2.1 保証対象

標準 Fault Model に従う。単一ホスト・単一ユーザー・正常な `ai` / `aibe` process 生存中に、次を保証する。

- request は実行前に schema、非空文字列、件数・文字数上限、worker ID、cwd、timeout、delegation depth を検査する
- LLM は executable、raw argv、shell string、環境変数、approval 状態を request から指定できない
- Worker は設定済み registry からだけ選び、製品固有の引数・出力変換は adapter に閉じる
- cwd は親 request の認可済み `context.cwd` を基準に解決し、その許可 root 外、存在しない path、非 directory、symlink escape を実行前に拒否する
- timeout 時は既存 process cleanup 契約を再利用して子 process group を停止・回収し、timeout を成功へ変換しない
- stdout / stderr / exit code / timeout / launch failure / bounded changed-file observation を出所付きで正規化する
- Worker の完了自己申告、exit code 0、changed files の存在だけでは検証済みにせず、全結果・Evidence を `verified=false` で返す
- `agent_task` 自体の approval を、`shell_exec` の allowlist、`--yes-exec`、Worker の自己申告、または製品固有 flag で迂回できない
- aibe が管理する委譲深度は 1 までとし、委譲された context からの `agent_task` を fail-closed に拒否する

### 2.2 保証対象外

- `ai` / `aibe` crash、socket 切断、OS 再起動後の Worker 再接続・結果復元
- Worker process が強制終了直前に行った外部副作用の完全な rollback または結果確定
- Worker 内部の LLM、tool、sandbox、network、login、独自 approval の安全性
- 検査済み cwd を Worker の filesystem access 上限にすること（cwd は起動位置であり OS-level sandbox ではない）
- 悪意ある Worker が環境変数を破棄して別の `ai` process を起動することまで防ぐ OS-level sandbox
- changed-file observation がファイル内容の正しさや Completion Criteria 達成を証明すること
- 外部 Agent 製品の出力 format 変更に対する自動 recovery
- 複数 process / host、exactly-once、再実行 deduplication

## 3. Non-goals

- 複数 Worker の並列 fan-out / fan-in、非同期 job、queue
- Worker からの再委譲、深度 2 以上、aibe 内の secondary agent loop
- Worker session / thread の永続化、checkpoint / resume、lease / heartbeat、reconciler、crash recovery
- Worker 結果の独立検証、親による自動修正反復、critic / judge Agent
- PR / CI 専用 orchestration、GUI 操作、開発工程専用状態機械、汎用 workflow DSL
- 巨大な `DelegatedTaskFramework`、共通 Human/Agent lifecycle aggregate、`human_task` の挙動変更
- 外部 Agent の内部コマンドを AISH safe tools と同等に承認・監査したと主張すること
- 未設定 Worker を成功扱いする fallback、空の mock/stub 結果による成功

### 3.1 0068 Task Completion との関係

`agent_task` は 0068 の外側に新しい Task Completion loop を作らず、既存 Query Loop 内の一つの tool として動く。`AgentTaskResult` は 0068 の Evidence ledger に `source=agent_task`、`verified=false` で追加できるが、Worker の `reported_complete` や exit code 0 だけで criterion を satisfied または Done にしない。親が後続の read-only tool や verification command で再観測した場合は、0068 の既存規則が別 Evidence として検証可能性を判断する。

本段階では Worker 結果を受けて自動で再委譲・修正する loop を設けない。親 Query Loop が結果を読んで次の通常 tool call を選ぶことは既存挙動であり、専用修正反復ではない。

### 3.2 `human_task` との関係

`human_task` と `agent_task` は親 Agent に提示される独立した選択肢である。`agent_task` は Human Shell、briefing、checkpoint、resume、continuation、Human Task Evidence を再利用・変更しない。共通化するのは tool publication、request context、approval 表示など既存の薄い基盤境界までとし、Human/Agent を統合する新 aggregate や状態機械は作らない。

### 3.3 信頼境界

| 境界 | AISH が保証すること | AISH が保証しないこと |
|------|----------------------|------------------------|
| 親 LLM → `agent_task` | strict schema、上限、registry lookup、cwd/depth/approval 検査 | LLM が選んだ委譲内容の有用性 |
| aibe → Worker process | argv 分離、設定済み executable、検査済み cwd、bounded timeout/kill/reap、出力上限 | Worker 内部の tool・network・認証・sandbox |
| Worker output → 親 | status と Evidence の fail-closed 正規化、provenance、`verified=false` | Worker の自己申告の真実性 |
| Worker filesystem effect → Evidence | bounded な前後メタデータ観測と changed path の列挙 | 内容の正しさ、全外部副作用の把握 |

`agent_task` approval は「指定 Worker に、表示した objective / cwd / timeout / permission profile で一回実行を許す」判断である。Worker 内部操作ごとの承認を代行するものではない。この差を approval UI と監査記録に明示する。

ここで cwd 検査が保証するのは、Worker の起動位置と changed-file observation の基準が親の認可 root 内であることまでであり、Worker 自体の filesystem access を root 内へ閉じ込めることではない。`permission profile` は adapter が設定から選ぶ Worker 起動 profile の識別子であって OS 強制境界を意味しない。LLM request から profile、環境変数、credential を指定・拡張させず、adapter は製品固有環境を設定の allowlist からだけ構成する。Worker が profile 外へアクセスし得る残余リスクを approval UI に表示し、root 内への強制が必要になった場合は Deferred の OS-level Worker Sandbox Hardening として STOP-THE-LINE する。

## 4. Complexity inventory

| 項目 | 数・内容 |
|------|----------|
| 実行主体 | 3（`ai` client、既存 `aibe` 親 Query Loop、外部 Worker process） |
| 状態機械 | 2（既存 Task Completion lifecycle、既存 Query Loop。0069 固有の状態機械は 0） |
| 永続 aggregate | 0（request / result / snapshot は request-local） |
| 外部副作用 | 3（既存 provider query、Worker process 実行、workspace メタデータ観測） |
| プロセス境界 | 2（既存 `ai` ↔ `aibe` socket、`aibe` ↔ Worker child process） |
| 新規基盤機構 | 1（`agent-task-worker-port`） |
| 他機能統合 | 3（既存 Query Loop / tool registry、0068 Task Completion Evidence、既存 external-command process policy） |

数値は `scripts/feature-scope.toml` の 0069 entry と一致する。`human_task` は挙動を変えず regression 対象に留めるため integration 数へ含めない。

## 5. Complexity Gate

- 判定: **Yellow**
- 理由: 実行主体 3、状態機械 2、外部副作用 3、process boundary 2、integration 3 が Yellow 閾値に達する。一方、永続 aggregate は 0、新規機構は Worker Port 一つで、Red flag（crash recovery、schema migration、secondary agent loop、lease / heartbeat、reconciler、exactly-once）はすべて false とする
- 分割判断: 同期 Worker 呼び出しから未検証 Result + Evidence を親へ返すまでが一つの vertical slice である。独立検証、修正反復、並列、再委譲、永続化、recovery は Deferred spec に分割する
- scope review: **approved** — Issue #24 の Minimum Vertical Slice を単一 Worker・同期・深度 1・request-local 状態に限定する
- 承認例外: 不要（Red ではない）

外部 Worker が内部に Agent loop を持つことは adapter の外にある opaque な製品挙動であり、aibe が管理する `secondary_agent_loop` ではない。aibe が Worker の provider round、会話履歴、tool catalog、再試行を管理する必要が判明した場合は STOP-THE-LINE とし、本 spec へ追加しない。

## 6. Complexity budget

| 項目 | 追加可能な上限 |
|------|----------------|
| 実行主体 | +0 |
| 状態機械 | +0 |
| 永続 aggregate | +0 |
| 外部副作用 | +0 |
| プロセス境界 | +0 |
| 新規基盤機構 | +0 |
| 他機能統合 | +0 |

実装は `AgentTaskWorker` Port、Registry、同期 application service、外部 command adapter、bounded Evidence collector に留める。これらを包括する汎用委譲 framework や新 lifecycle controller は予算外である。

## 7. Split triggers

次のいずれかが必要になった時点で STOP-THE-LINE とし、0069 に追加しない。

- aibe が管理する secondary agent loop、二つ目の新規状態機械、別 conversation
- Worker の並列実行、非同期 queue、background supervisor
- task / result / Worker session の永続 aggregate、checkpoint / resume、schema migration
- Worker からの再委譲、深度 2 以上、delegation graph
- 独立 verifier / critic Agent、自動修正・再実行 loop
- lease / heartbeat、reconciler、journal、idempotency key、exactly-once、crash recovery
- PR / CI / GUI 固有 orchestration または開発工程 state machine
- process boundary 3 以上、external effects 5 以上、integration 4 以上、novel mechanism 2 以上になる変更
- `human_task` lifecycle と Agent Task lifecycle の共通 aggregate 化

## 8. パック構成の適用

**部分適用** — 0045 §6 のうち、無効化した basic runtime を維持したい、既存 Query Loop の tool registry へ横断的に公開する、将来 optional 配備し得る、の 3 条件に該当する。server-side の `AgentTaskPack` trait を Pack 境界とし、その返す `AgentTaskWorkerRegistry` と tool publication policy だけを差し替える。Active 側は設定済み Worker だけを登録、Basic 側は registry empty かつ `agent_task` 非公開で fail-closed、選択は aibe の composition root 1 か所に限定する。runtime toggle / disabled test は設けるが、初期段階では重い in-process dependency がなく Worker は外部 process なので Cargo feature による compile-time 除外は行わず、`ai` client pack も新設しない。request validation、application service、Result / Evidence 正規化は core に残す。

### 8.1 Port / Registry / composition

core / application 層は製品名や command line を知らず、次の Port のみに依存する。

```rust
trait AgentTaskWorker: Send + Sync {
    fn execute(
        &self,
        request: ValidatedAgentTaskRequest,
        context: AgentTaskExecutionContext,
    ) -> Result<WorkerExecutionOutput, AgentTaskWorkerError>;
}
```

`AgentTaskPack` は registry と tool publication policy を供給する薄い server-side trait とする。`ActiveAgentTaskPack` と `BasicAgentTaskPack` がこの trait を実装し、core / application は具体 Pack を参照しない。

- `AgentTaskWorkerRegistry`: opaque な `WorkerId` から `Arc<dyn AgentTaskWorker>` を引く。未知 ID、disabled、重複 ID は fail-closed
- `ActiveAgentTaskPack`: 明示的に Agent Task 用として許可された external command 設定だけを adapter 化して登録する
- `BasicAgentTaskPack`: registry empty、tool 非公開。forged allowlist で `agent_task` を呼ばれても拒否する
- composition root: aibe server 起動時の 1 か所だけで enabled 設定と Worker 構成を解決する
- `ExternalCommandWorker`: executable / argv template / structured-output parser / product-specific environment を所有する outbound adapter
- `MockWorker`: process を起動せず、受け取った request/context と決定的 output/error を記録・返却するテスト adapter

Codex / Cursor / その他製品名、CLI flag、JSON 方言は `ExternalCommandWorker` の具体 adapter または設定 parser に閉じる。domain DTO、application service、tool schema に製品固有 field を置かない。

## 9. Acceptance Criteria

| ID | 条件 |
|----|------|
| `agent_task_vertical_e2e` | deterministic な fixture external command を設定済み Worker として使う親 Query Loop の縦断試験で production の registry / adapter / subprocess 経路から `agent_task` を1回呼び、検査済み request が指定 cwd で同期実行され、Result + Evidence が `verified=false` で同じ親へ戻る（実 API・実 Agent 製品は使わない） |
| `agent_task_request_is_strictly_validated` | Objective / Instructions / Completion Criteria の必須・上限、Worker ID、cwd、timeout、未知 field を実行前に検査し、LLM から executable / argv / env / approval を指定できない |
| `agent_task_registry_and_disabled_pack_fail_closed` | registry は明示許可された設定済み Worker だけを返し、未知・重複・disabled・forged tool allowlist を拒否し、Basic Pack では `agent_task` を公開しない |
| `agent_task_runs_in_validated_cwd_with_timeout` | cwd を親の認可 root 基準で解決して escape / missing / non-directory を拒否し、timeout 時は process group を有限時間で kill / reap して timed_out を返す |
| `agent_task_result_normalizes_worker_outcomes` | stdout / stderr / exit code / timeout / launch failure / malformed structured output を bounded な共通 `AgentTaskResult` に正規化し、非 zero・timeout・parse error を成功扱いしない |
| `agent_task_evidence_is_bounded_and_unverified` | changed files、stdout/stderr、exit/timeout を provenance と上限付き Evidence にし、Worker の報告、exit code 0、変更 path があっても全項目の `verified` は false のままである |
| `agent_task_recursion_is_rejected` | parent depth 0 から一度だけ委譲でき、delegated depth 1 の context では tool 非公開かつ forged request も拒否する |
| `agent_task_approval_cannot_be_bypassed` | agent_task 固有 approval が実行前に必要で、shell_exec allowlist / auto approval、`--yes-exec`、Worker 引数・出力から承認済みへ昇格できず、監査に worker / cwd / timeout / decision が残る |
| `agent_task_integrates_with_task_completion_as_unverified` | 0068 Evidence ledger が AgentTaskResult を未検証 Evidence として取り込み、Worker 完了自己申告だけでは criterion satisfied / Done にせず、後続の親による独立観測を別 Evidence として扱う |
| `agent_task_preserves_human_task_behavior` | `human_task` の公開条件、Human Shell、checkpoint / resume / continuation、Evidence、approval の既存回帰試験が不変で、Agent Task と共通 lifecycle aggregate を持たない |
| `agent_task_core_is_product_agnostic_and_mockable` | core / application は `AgentTaskWorker` Port と共通 DTO のみ参照し、製品固有 command / parser は adapter 内にあり、`MockWorker` で success / failure / timeout / malformed output を process 起動なしに試験できる |

### 9.1 Request schema

LLM が呼ぶ `agent_task` tool schema は次を基本形とする。JSON Schema は `additionalProperties=false` とし、文字列・配列には実装指示書で固定する bounded limit を設定する。

```json
{
  "worker": "configured-worker-id",
  "objective": "委譲で達成する一つの目的",
  "instructions": ["守るべき作業指示"],
  "completion_criteria": [
    {"id": "criterion-1", "description": "完了判定可能な条件"}
  ],
  "cwd": "親 context.cwd 以下の相対パス",
  "timeout_secs": 600
}
```

`AgentTaskRequest` は raw 入力型、`ValidatedAgentTaskRequest` は検査後の domain 型とする。`worker` は registry key であり executable 名ではない。`cwd` の省略時は親 `context.cwd`、絶対 path は同一の認可 root 内であることを canonicalize 後に要求する。`timeout_secs` は request 値、Worker 設定上限、server 全体上限の最小値を実効値とし、0 や上限超過を暗黙補正せず拒否する。

Worker へ渡す prompt / stdin envelope は少なくとも schema version、objective、instructions、completion criteria、validated cwd、`delegation_depth=1` を含む。shell interpolation は使わず、argv と stdin を分離する。

### 9.2 Result / Evidence schema

```json
{
  "schema_version": 1,
  "worker": "configured-worker-id",
  "status": "completed | blocked | cancelled | failed | timed_out | launch_failed | invalid_output",
  "summary": "bounded worker summary or normalization message",
  "reported_complete": false,
  "blockers": ["optional bounded blocker messages when status=blocked"],
  "exit_code": 0,
  "timed_out": false,
  "stdout": {"text": "bounded and sanitized", "truncated": false},
  "stderr": {"text": "bounded and sanitized", "truncated": false},
  "evidence": [
    {
      "kind": "changed_file | worker_report | process_output | exit_status",
      "source": "workspace_observation | worker_report | process_runner",
      "path": "optional relative path",
      "summary": "bounded description",
      "verified": false
    }
  ],
  "verified": false
}
```

- `status=completed` かつ `reported_complete=true` は Worker が正常終了して構造化 `done` を返したことだけを意味し、Task Completion の `Done` ではない
- Worker 構造化 report の `status` は `done | blocked | cancelled | failed`。`blocked` は非空の `blockers` を必須とし、親が未完了と実行障害を区別できるようにする
- timeout / signal termination では `exit_code=null` を許容し、`timed_out=true` と status を一致させる
- stdout / stderr / Evidence / `changed_paths` は、`env_allowlist` で継承した値の exact-value 置換（path に含まれる場合は当該 path を除外して `observation_incomplete`）と既存 sanitize / redaction（`aish_replay::sanitize_log_text`）、および byte/item 上限を適用し、完全な process log や秘密値を親 prompt へ無制限に複製しない
- changed-file Evidence は worker report と filesystem 前後観測を別 provenance にし、path は cwd 相対・正規化済みとする。symlink target や file content を暗黙収集しない
- top-level と各 Evidence の `verified` は本 spec では常に false。true を受理・生成する schema は後続の独立検証 spec まで導入しない

### 9.3 権限・再帰・approval 不変条件

1. `agent_task` の利用可否は親 request の execution mode、server 設定、Active registry の積集合で決める。Worker 設定だけで tool を全 turn に公開しない。
2. Worker permission profile と製品固有環境は設定の allowlist からだけ選び、LLM request から指定・拡張できない。adapter は設定より強い product flag を足さない。cwd / profile は OS sandbox ではないため、親 root 外への filesystem access 不可能性は保証しない。
3. `agent_task` は shell command の別名ではない。専用 risk class と approval record を持ち、`shell_exec` の allowlist / approval decision を流用して自動承認しない。
4. `delegation_depth=1` を Worker 環境と構造化 envelope に渡し、aibe inbound context でも検査する。depth 1 以上の `agent_task` は registry lookup や process spawn より前に拒否する。
5. 外部 Worker が AISH の外で行う内部操作は別の信頼境界である。初期実装は悪意ある executable を sandbox 化したとは主張せず、管理者が明示設定した Worker だけを対象にする。

## 10. Deferred specs

| 候補 | 0069 から分離する理由 |
|------|------------------------|
| Agent Task Independent Verification | verifier / critic という新規実行主体と判定 loop が必要になるため |
| Agent Task Repair Iteration | Worker 再実行、budget、停滞判定を持つ新状態機械になるため |
| Parallel / Async Delegation | queue、fan-out/fan-in、cancellation、複数 Worker coordination が必要になるため |
| Durable Agent Task Resume / Recovery | 永続 aggregate、lease、reconciler、schema migration、crash recovery が必要になるため |
| Nested Delegation | delegation graph、深度 budget、循環検出が必要になるため |
| PR / CI / GUI Adapters | 製品・実行環境固有の追加 process boundary と権限設計が必要になるため |
| OS-level Worker Sandbox Hardening | malicious Worker に対する namespace / seccomp / network / credential isolation は独立した security feature のため |

## 11. Scope change log

| Revision | 分類 | 変更 | 理由 |
|----------|------|------|------|
| 1 | INITIAL | 単一 Worker・同期・深度 1・設定済み external command・未検証 Result/Evidence に Scope Lock 前の draft を限定 | Issue #24 の Minimum Vertical Slice を Yellow 以下で設計し、独立検証・反復・永続化を後続へ分離するため |
| 2 | `SAFETY_WITHIN_FAULT_MODEL` / `BLOCKER_ORIGINAL_AC` | cwd / permission profile の非 sandbox 境界を明記し、production external-command adapter の deterministic smoke を vertical AC に固定。部分適用の Pack trait と core 残置範囲を明記 | cwd の保証を過大評価させず、本番経路と 0045 最低要件を検証可能にするため |
| 3 | `SAFETY_WITHIN_FAULT_MODEL` / `BLOCKER_ORIGINAL_AC` | 絶対 executable、timeout の drain 包含、承認監査の型化、`blocked`+`blockers`、stdout/stderr redaction を固定（PR #26 review） | 信頼境界・有限時間・監査正しさ・親継続判断・秘密流出防止を Fault Model 内で満たすため |
