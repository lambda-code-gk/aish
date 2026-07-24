# 0070 Task Completion Phase 3 Independent Verification 設計書

> **種別**: 設計書（`docs/spec/`）
> **状態**: 設計確定（実装済み）
> **起票**: 2026-07-23
> **関連**: GitHub Issue #27（Parent: #17 / Depends on: #18, #24）、[`0068_task-completion-phase1-spec.md`](0068_task-completion-phase1-spec.md)、[`0069_agent-task-delegation-spec.md`](0069_agent-task-delegation-spec.md)、[`docs/feature-development-policy.md`](../feature-development-policy.md)、[`0045_pack-composition-spec.md`](0045_pack-composition-spec.md)

## 0. Core outcome

親 AISH が委譲先の完了報告ではなく成果物と外部状態の独立再観測を根拠に Task Contract を criterion 単位で判定し、未達なら一度だけ Gap 付きで再作業を委譲して理由付き終端へ到達できる。

## 1. Minimum vertical slice

```text
親 Task Completion Loop が元 Task Contract を所有
→ 一つの Agent Task を作成
→ Worker が Result + Artifact references を返す
→ 親が Result / Evidence を verified=false で取り込む
→ 親が changed files / Git diff / 指定 verification command を自分の tool 経路で再観測
→ Completion Evaluator が元 Contract の全 criterion を satisfied / unsatisfied / unknown / not_applicable で評価
→ 全 applicable criterion が satisfied なら検証成功
→ 未達なら一つの Gap を生成
→ 同じ Worker へ Gap 付きで一度だけ同期再委譲
→ 親が同じ検証計画を再実行して再評価
→ 成功なら親 Task を継続または Done、未達なら Stagnated または NeedsUser
```

本 spec の「独立」は、Worker の Result、`reported_complete`、exit code、Artifact references を判定根拠としてそのまま信用せず、元 Task Contract を所有する親が既存の read-only tool と verification command 実行経路から新しい Evidence を得ることを指す。独立 verifier Agent、critic / judge Agent、別 provider conversation は作らない。

MVP の再委譲 budget は一つの元 Agent Task につき **1回**である。再委譲は 0069 の同じ設定済み Worker を通常の `agent_task` 経路でもう一度同期実行する。Gap は元 Contract の criterion ID、観測された差分、必要な成果、再検証方法を持つ bounded な追加入力であり、Worker executable、permission profile、cwd、approval、timeout、delegation depth の 0069 契約を迂回しない。再委譲も新しい `agent_task` approval を必要とする。0070 は 0069 の wire `AgentTaskRequest` に `gap` field を追加せず、§1.3 の変換で既存 field だけを構築する。

### 1.1 Evidence の優先順位

同じ criterion の同じ検証対象について Evidence が矛盾する場合は、次の順に外部状態へ近い親の再観測を優先する。同順位の矛盾は `unknown` とし、都合のよい一方だけで `satisfied` にしない。異なる検証対象（例: 成果物内容と test 実行結果）は順位で片方を捨てず、Contract が要求する両方を満たす。

1. 親の read-only tool による成果物内容・メタデータの直接観測、および親が取得した Git diff
2. 親が §1.3 の固定済み Verification Plan と完全一致する形で実行した verification command の構造化された終了状態と bounded output
3. 親が取得した changed-file observation
4. Worker が返した Artifact references、changed-file report、process output、完了自己申告

Artifact reference は再観測先を示す locator であり、存在・内容・criterion との対応を親が確認するまで `verified=false` の候補 Evidence に留める。verification command が成功しても、その command が委譲前に親 Contract へ固定された plan item と完全一致し、対象 criterion と実行後状態をコードで関連付けられない限り、自動的に全 criterion を満たさない。

### 1.2 Criterion と Gap

Completion Evaluator は元 Task Contract の criterion 集合を変更せず、各 criterion を次のいずれか一つに評価する。

| 状態 | 意味 |
|------|------|
| `satisfied` | 親が独立再観測した valid Evidence が criterion を満たす |
| `unsatisfied` | 親の観測が具体的な未達または検証失敗を示す |
| `unknown` | 観測不能、不十分、矛盾、または検証基盤の失敗で判定できない |
| `not_applicable` | 元 Contract の条件付き criterion が今回の入力では適用されず、その理由を示せる |

`not_applicable` は Evaluator の宣言だけでは認めない。`CompletionCriterion` に optional additive な `applicability`（bounded な条件記述）を追加し、Contract 固定時から非空であること、evaluation が valid / verified な `applicability_evidence_ids` を参照することをコードで検査し、その Evidence と条件の意味的対応だけを既存 Completion Evaluator に委ねる。`applicability` のない criterion を `not_applicable` にすること、未検証・stale Evidence で非適用にすることを拒否する。

**一つの Gap** は `entries` 配列を持ち、すべての actionable な `unsatisfied` と解消可能な `unknown` criterion を criterion ごとの entry としてまとめる。各 entry は元 criterion ID、観測事実、期待状態、必要な再作業、再検証 plan item ID を含む。元 Contract の Goal / Constraints / criterion 集合を再委譲用に書き換えない。actionable な未達が件数・byte 上限を超えて一つの Gap に安全に収まらない場合は一部を黙って落とさず `Blocked` とする。

### 1.3 親固定 Verification Plan と再委譲変換

0070 Active 時は、最初の `agent_task` を実行する前に、固定済み Task Contract から request-local の `DelegatedVerificationPlan` を構築して固定する。Task Contract には optional additive field として構造化 `delegated_verification` を追加し、各 plan item は stable ID、対象 criterion ID 集合、`observation` または `command`、期待する成功条件を持つ。非委譲 turn では field を省略できるが、`agent_task` を呼ぶ assistant step では tool 実行前の ContractGate が non-empty plan、重複のない plan item ID、Agent Task completion criterion ID の親 Contract 部分集合、各委譲 criterion を覆う1件以上の plan item を検査し、欠落・不整合なら Worker 起動前に fail-closed で拒否する。

- `observation`: 0068 の server-trusted read-only tool（`read_file` / `list_dir` / `grep` / `git_status` / `git_diff`）と対象を指定する
- `command`: `shell_exec` の exact `command`、分離済み `args`、親 `context.cwd` 基準で解決して初回 Agent Task の canonical な検証済み実効 cwd と一致する cwd、対象 criterion ID 集合を指定する。shell string、環境変数、Worker output からの command 追加は許可しない
- plan item の criterion ID は元 Contract の部分集合、command は server の `allowed_commands` と既存 approval policy の範囲内でなければならない
- Worker Result / Artifact reference / Gap から plan item を追加・変更できない。再委譲後も同一 plan ID と内容を再利用する

0068 の通常 `shell_exec` は引き続き `UnknownShellEffect` であり、検証 Evidence にはならない。0070 Active で、委譲後に実行された call が固定済み command plan item の command / args / cwd と完全一致し、既存 allowlist・approval・timeout を通過して **実プロセスが起動された**場合、その plan item が指定した criterion に限定して `Verification` 候補 Evidence とする。`status=Ok` のときだけ `verified=true` とし、非 zero exit など実行後失敗は `verified=false` の観測結果として Gap / 四状態評価へ回す（それ自体を `Failed` / `InvalidRequest` にしない）。handoff・事前拒否などプロセス未起動は plan 未実行のままとする。MVP では成功 shell の stale 化と照合 `.find()` の制約から **command plan item は1件まで**とし、Contract 固定時に拒否する。command は従来どおり過去の observation / verification を stale 化するため（成功実行時）、Verification Plan は command item を observation item より先に実行し、成果物内容を要求する criterion は command 後の直接観測も満たさなければ `satisfied` にしない。これにより 0068 の任意 shell 非信頼契約を一般化または緩和しない。

Gap follow-up は新 wire field を作らず、次の決定的変換で既存 0069 `AgentTaskRequest` を構築する。

| 0069 field | follow-up 値 |
|------------|---------------|
| `worker` | 初回 request と同一 `WorkerId` |
| `objective` | 元 objective を維持し、bounded な「未達 Gap を解消する」目的を付記 |
| `instructions` | 元 instructions に、Gap entry の観測事実・必要作業・plan item ID を bounded item として追加 |
| `completion_criteria` | Gap entry の元 Task Contract criterion ID と description。初回 Agent Task の criterion ID は親 Contract ID の部分集合であることを初回実行前に検査する |
| `cwd` / `timeout_secs` | 初回の検証済み実効値と同一 |

変換後 request は初回と同じ 0069 shape / registry / cwd / timeout / depth / approval 検査を再度通す。既存上限に収まらない場合は truncation で criterion や安全指示を失わせず `Blocked` とする。

この cycle は 0068 の固定 query budget **2** の内側で完結する。query 1 が初回 `agent_task` と Verification Plan 実行・評価、query 2 が Gap follow-up `agent_task` と同じ plan の再実行・再評価を担う。評価専用または再委譲専用の3回目 query / provider call は追加せず、query 2 で tool round budget が尽きれば既存規則に従い `BudgetExhausted` とする。

## 2. Fault model

### 2.1 保証対象

標準 Fault Model に従う。単一ホスト・単一ユーザー・正常な `ai` / `aibe` process 生存中に、次を保証する。

- 親 Task Completion Loop が元 Task Contract と criterion ID の唯一の所有者であり、Worker Result から条件を追加・削除・達成済みへ変更しない
- `agent_task` と `human_task` の Result / Evidence は親へ戻った時点で `verified=false` とし、委譲先の完了自己申告だけでは親 Task を Done にしない
- 親が Artifact references、changed files、Git diff、固定 Verification Plan の verification command を既存 tool 経路から再取得し、provenance と観測順を持つ新しい Evidence として記録する
- 全元 criterion を四状態のいずれかにちょうど一つ評価し、`unknown` を `satisfied` に fail-open しない
- 全 applicable criterion が独立再観測 Evidence により `satisfied` の場合だけ検証成功とする
- 未達時は actionable な criterion entry をまとめた bounded な Gap を一つ生成し、Agent Task に限り同じ Worker へ最大1回だけ再委譲して、同じ検証計画を再実行する
- 0069 の registry、cwd、timeout、depth、approval、process cleanup、redaction、bounded output の契約を初回と再委譲の両方で維持する
- 終端は `Done / NeedsUser / Blocked / Stagnated / BudgetExhausted / Failed / Cancelled` のいずれか一つとし、criterion 評価、利用 Evidence、Gap、再委譲回数、未検証事項を bounded に報告する

### 2.2 保証対象外

- `ai` / `aibe` crash、socket 切断、OS 再起動後の Contract、Evidence、Gap、再委譲 budget の復元
- 複数 process / host、並列委譲、非同期 Worker、複数 Worker の比較・多数決
- 外部副作用の exactly-once、rollback、結果不明状態の自動解消
- 任意の shell command が行った全副作用や Git 管理外・workspace 外状態の完全な発見
- verification command 自体の仕様が正しいこと、または LLM の意味評価が常に正しいこと
- 悪意ある Worker、verification command、成果物を OS-level sandbox で隔離すること
- 長期監視、process 再起動後の自動再検証、外部 CI 完了待ち

## 3. Non-goals

- verifier / critic / judge Agent、secondary agent loop、別 provider conversation
- 並列実行、複数 Worker、多数決、Agent 間通信、再帰委譲、深度 2 以上
- Agent Task の自動 Worker 選択、Worker 切替、adaptive retry、2回以上の再委譲
- Task Graph、Planner DSL、汎用 workflow engine、開発・PR・CI 専用の固定 workflow や状態機械
- Human Task の UI、checkpoint / resume / continuation、approval、外部挙動の変更
- Human Task を自動で再開・再委譲すること（未達時は既存のユーザー導線へ `NeedsUser` として返せる）
- 永続 store、lease / heartbeat、reconciler、journal、schema migration、crash recovery
- filesystem watcher、長期監視、PR / CI status adapter、GUI adapter
- 委譲開始後の verification command 自動生成、Worker / Gap による command 追加、任意 shell 実行権限の拡張

### 3.1 0068 / 0069 / human_task との責務境界

| 境界 | 本 spec（0070） | 既存契約 |
|------|-----------------|----------|
| Task Contract | 元 criterion の保持、委譲前 Verification Plan の固定、委譲結果の独立検証、Gap と bounded follow-up | 0068 が Contract / Evidence ledger / Task Completion lifecycle を所有。optional additive Contract field 以外の意味は不変 |
| Agent Task | 未検証 Result の受理、Gap から既存 request field への変換、同一 Worker への最大1回の再呼び出し | 0069 が request validation、Worker 実行、approval、Result 正規化を所有。wire request / result schema は不変 |
| 観測・検証 | 検証計画に従い既存 tool を親として呼び、結果を criterion へ関連付ける | 既存 Query Loop / tool execution が read / Git / command effect を実行 |
| Human Task | 完了自己申告を親 Task の検証済み根拠へ昇格させない回帰保証のみ。0070 の新規 Verification Plan / Gap 経路は接続しない | 既存公開条件、通常 Done の親継続、Suspended 時評価 skip、shell handoff、checkpoint / resume / continuation、approval は不変 |

0068 の Evidence 不変条件を置換せず、`source=agent_task` Evidence を候補 Evidence として取り込んだ後に、親観測の Evidence を追加する。通常の shell を非信頼とする規則も維持し、§1.3 の委譲前固定・exact match・criterion 限定を満たす command だけを 0070 の Verification 候補へ昇格する。0069 の `verified=false` 出力 schema は変更せず、検証済みという意味は親 Task Completion 側の criterion evaluation にだけ保持する。Worker Result 自体を `verified=true` に書き換えない。

MVP の新規実行経路は Agent Task vertical slice に限定する。Human Task Evidence と完了自己申告を `verified=false` のまま扱う既存安全不変条件は回帰保証するが、0068 が `AgentTurnStatus::Suspended` で評価をスキップする挙動、0063–0066 の cross-suspend context / continuation、Human Task Result に対する自動 Verification Plan 実行は変更しない。このため Human Task は新規 integration 数へ含めない。cross-suspend で元 Task Contract を所有し続けて独立検証する機能は Deferred とする。

## 4. Complexity inventory

| 項目 | 数・内容 |
|------|----------|
| 実行主体 | 3（`ai` client、既存 `aibe` 親 Task Completion / Query Loop、0069 外部 Worker process） |
| 状態機械 | 2（既存 Task Completion lifecycle を拡張、既存 Query Loop。0070 固有の二つ目は追加しない） |
| 永続 aggregate | 0（criterion evaluation / Gap / follow-up count は request-local） |
| 外部副作用 | 4（既存 provider query、Worker process 実行、workspace / Git 再観測、verification command 実行） |
| プロセス境界 | 2（既存 `ai` ↔ `aibe` socket、`aibe` ↔ Worker / command child process 境界） |
| 新規基盤機構 | 1（`delegated-result-verification-cycle`） |
| 他機能統合 | 3（0068 Task Completion、0069 Agent Task、既存 tool execution / Evidence） |

数値は `scripts/feature-scope.toml` の 0070 entry と一致する。Human Task は Result を信用しない既存安全契約と回帰試験の対象であり、外部挙動や lifecycle を統合しないため integration 数へ追加しない。

## 5. Complexity Gate

- 判定: **Yellow**
- 理由: 実行主体 3、状態機械 2、外部副作用 4、process boundary 2、integration 3 が Yellow 閾値に達する。一方、永続 aggregate は 0、新規機構は1つで、Red flag（crash recovery、schema migration、secondary agent loop、lease / heartbeat、reconciler、exactly-once）はすべて false とする
- 分割判断: 独立検証と Gap follow-up は「未検証の委譲結果を親が完了判定へ変換する」一本の vertical slice を構成するため、`delegated-result-verification-cycle` 一つにまとめる。検証だけで再作業を含まなければ Issue #27 の未達解消経路が閉じず、再委譲だけでは信頼できる終了判定が成立しない。並列、複数 retry、永続化、専用 verifier Agent、Human Task 自動再開、PR / CI adapter は分割する
- scope review: **approved** — Issue #27 の Minimum Vertical Slice を単一 Agent Task、親による既存 tool 再観測、同一 Worker への再委譲1回、request-local 状態に限定する
- 承認例外: 不要（Red ではない）

One Novelty Rule 上、独立検証と Gap follow-up は別々の基盤機構ではない。既存 Task Completion lifecycle 内の `unverified delegated result → parent observation → criterion evaluation → optional one follow-up → re-evaluation` という一つの bounded cycle である。Gap は新しい job aggregate ではなく既存 `agent_task` request に渡す request-local DTO、Completion Evaluator は 0068 と同じ evaluator 境界の拡張である。

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

verification plan、criterion evaluation、Gap、follow-up count は既存 Task Completion request 内の型として保持する。独立した scheduler、queue、conversation、verifier service、汎用 retry controller へ昇格させない。

## 7. Split triggers

次のいずれかが必要になった時点で STOP-THE-LINE とし、0070 に追加しない。

- 親 Task Completion / Query Loop / 外部 Worker 以外の新しい実行主体、独立 verifier Agent、secondary agent loop
- 既存 Task Completion lifecycle とは別の状態機械、汎用 retry / workflow controller
- 再委譲2回以上、Worker 切替、複数 Worker、並列 fan-out / fan-in、Agent 間通信、再帰委譲
- Contract / Evidence / Gap / follow-up の永続 aggregate、checkpoint / resume、migration、GC
- lease / heartbeat、reconciler、watcher、journal、idempotency key、exactly-once、crash recovery
- Human Task lifecycle / Agent Task lifecycle の共通 aggregate 化、Human Task の自動 resume / re-dispatch
- PR / CI / GUI 固有 adapter、長期監視、Task Graph / DSL、開発工程状態機械
- process boundary 3 以上、external effects 5 以上、integration 4 以上、novel mechanism 2 以上になる変更

上記が発生した場合は Complexity inventory と `scripts/feature-scope.toml` の `scope_revision` を更新して Gate を再判定し、Red なら通常 feature として実装せず分割 spec を作る。

## 8. パック構成の適用

**No** — 0045 §6 を検討したが、0070 は optional な Worker adapter を追加する機能ではなく、Task Completion 対象 turn が委譲 Result を受け取った場合に自己申告を信用しないための core 完了意味論である。専用 RPC / CLI / turn hook の束、重い依存、compile-time 除外、別配備単位を追加せず、無効化した Basic Pack が委譲報告をそのまま Done にできると安全契約が二重化するため、新しい Pack 境界を設けない。0069 の `AgentTaskPack` が disabled なら Agent Task 経路自体が非公開となり本 cycle は起動しない。検証・Gap 型と evaluator policy は既存 0068 application / domain 境界に置き、composition root は増やさない。

## 9. Acceptance Criteria

| ID | 条件 |
|----|------|
| `delegated_verification_vertical_e2e` | deterministic Worker と fake tools を使う一つの親 Task Completion 縦断試験で、初回 Agent Task Result が `verified=false` で戻り、親が changed files / Git diff / 指定 verification command を再観測し、未達 criterion から一つの Gap を作って同じ Worker へ一度だけ再委譲し、同じ検証を再実行して全 applicable criterion の `satisfied` と独立 Evidence を伴う Done または親 Task 継続へ到達する |
| `parent_contract_owns_delegated_completion` | 親が元 Task Contract と安定 criterion ID を保持し、Agent Task / Human Task の完了自己申告、exit code 0、Artifact references、changed-file report だけでは criterion を satisfied または親 Task を Done にせず、委譲 Result / Evidence を常に `verified=false` から扱う |
| `parent_reobserves_artifacts_and_external_state` | 委譲前に親 Contract の non-empty structured Verification Plan を固定し、stable plan ID、委譲 criterion coverage、初回 Agent Task の canonical cwd 一致を Worker 起動前に検査する。Artifact references を bounded / sanitized に解決し、親の既存 tool 経路で changed files、Git diff、成果物内容、plan と exact match する verification command を委譲後に再取得する。観測元・順序・対象 criterion を持つ別 Evidence とし、通常 shell、Worker / Gap 由来 command、mismatch、missing / escape / unreadable / command failure を成功扱いしない |
| `evidence_precedence_and_conflicts_fail_closed` | 指定 verification command、直接観測 / Git diff、changed-file observation、Worker report の優先順位を適用し、同順位の矛盾、stale observation、criterion に関連しない成功 command、存在しない Evidence 参照を `unknown` または invalid として Done に使わない |
| `criterion_evaluation_is_exhaustive_and_structured` | 元 Contract の全 criterion を `satisfied / unsatisfied / unknown / not_applicable` のちょうど一つへ評価し、未知・欠落・重複 ID を拒否する。`not_applicable` は Contract 固定時からの bounded `applicability` と valid / verified / non-stale な `applicability_evidence_ids` がある場合だけ許し、全 applicable criterion が独立 Evidence 付き `satisfied` の場合だけ検証成功となる |
| `gap_follow_up_is_single_bounded_and_same_worker` | すべての actionable な未達 criterion entry（元 ID、観測事実、期待状態、必要作業、plan item ID）を持つ bounded な Gap を一つ生成し、既存 `AgentTaskRequest` field へ決定的に変換して、0069 の同じ Worker / cwd / depth / timeout / approval 契約で最大1回だけ再委譲する。Contract / Verification Plan を変更せず、上限超過、2回目の follow-up、Worker 切替、再帰委譲を拒否する |
| `follow_up_repeats_verification_and_detects_stagnation` | 再委譲後に同じ verification plan を親が再実行し、criterion 状態、Evidence fingerprint、正規化 failure を比較する。全 applicable criterion が満たされなければ追加再委譲せず、新規 Evidence も criterion 改善もない場合は Stagnated、ユーザーにしか解消できない事項が具体化された場合は NeedsUser とする |
| `verification_terminal_outcomes_are_distinct` | `Done / NeedsUser / Blocked / Stagnated / BudgetExhausted / Failed / Cancelled` を相互排他的に決定し、§9.1 の既存 wire 互換 projection で機械的に区別できる。Done は全 applicable criterion satisfied、NeedsUser は具体的なユーザー入力・承認・手動作業待ち、Blocked は現在の権限・制約内で解消不能、Stagnated は follow-up 後も進展なし、BudgetExhausted は既存 query / tool budget 到達、Failed は検証経路自体の非回復エラー、Cancelled は明示取消にだけ用いる |
| `verification_preserves_existing_boundaries_and_human_task` | 0068 の query budget 2（初回委譲＋検証 / Gap follow-up＋再検証）と Evidence 不変条件、0069 の Worker registry / approval / cwd / depth / timeout / redaction、既存 `human_task` の Suspended 時評価 skip・公開条件・shell・checkpoint / resume / continuation・approval が不変で、独立 verifier Agent、3回目 query、開発専用 workflow、Human/Agent 共通 lifecycle aggregate を導入しない |
| `verification_report_is_bounded_and_auditable` | `AgentTurnResult` で終わる5終端の human / structured 表示に元 criterion ごとの四状態、採用 Evidence の provenance、未検証事項、Gap、Worker ID、follow-up 使用数、verification command の bounded result、終端理由を含める。Failed / Cancelled は既存 `Error` / `Cancelled` response の bounded reason を表示し、いずれも秘密値・非採用 raw output・workspace 外 path を無制限に表示しない |

`vertical_slice_ac_id` は **`delegated_verification_vertical_e2e`** とする。Step 2 では Scope Lock 前のため、`scripts/spec-acceptance.toml` 登録と pending / ignored test は作成しない。Step 3 で各 AC を単一の同名 Rust test function と 1:1 で登録し、同じ変更で `status="locked"`、`locked_ac_ids` 全10件へ遷移する。

現行 `check-feature-scope.py` は `status="draft"` にも `vertical_slice_ac_id` の acceptance registry 存在検査を無条件適用するため、この Step 2 の正規状態では既知の1件（0070 vertical AC 未登録）で失敗する。Step 1 / 2 の「acceptance pending test を作らない」という工程契約を優先し、checker や registry へ例外・仮 case を追加しない。Step 3 の Scope Lock で解消し、それ以前は feature-scope の数値・Gate・必須節をレビューで照合する。この staged-state 不整合自体の恒久修正は 0056 governance の別変更であり、0070 の機能 scope に含めない。

### 9.1 終端判定順序

1. 明示取消を受けた場合は `Cancelled`。
2. schema、Evidence 参照、tool status、criterion 集合をコードで検査する。検証経路自体が再試行不能に壊れた場合は `Failed`。
3. 全 applicable criterion が valid な独立 Evidence 付き `satisfied` なら `Done`（親 Contract に残作業があれば親 Task を継続）。
4. 未達で、具体的なユーザー入力・承認・手動操作がなければ解消できない場合は `NeedsUser`。
5. 未達で、現在の権限・Constraint・利用可能 tool 内では解消不能な場合は `Blocked`。
6. follow-up 後も criterion 改善または新規 valid Evidence がない場合は `Stagnated`。
7. 上記以外で既存 query / tool budget に達した場合は `BudgetExhausted`。
8. 初回評価で解消可能な未達があり follow-up 未使用なら Gap を一つ生成して再委譲する。

検証 command の非 zero、Artifact の missing、Evidence 矛盾は通常 `unsatisfied` / `unknown` の観測結果であり、それ自体を `Failed` にしない。`Failed` は command launch や内部 schema 処理など、criterion を評価する経路自体が成立せず、Fault Model 内で再試行不能な場合に限定する。

### 9.2 既存 wire 互換 projection

0070 domain は七つの `VerificationTerminal` を持つが、既存 wire `CompletionOutcome` の4 variant と top-level response variant は削除・改名しない。`CompletionReport` には optional additive な `verification_terminal` を、`CompletionCriterionReport` には optional additive な `evaluation_status`（四状態）を追加する。従来の `satisfied: bool` は `evaluation_status=satisfied` の場合だけ true として残す。0070 非適用 report では新 field を省略できるため、0068 の既存 JSON を維持する。

| domain terminal | 既存 wire projection | additive field / reason |
|-----------------|----------------------|-------------------------|
| `Done` | `AgentTurnResult.completion_report.outcome=done` | `verification_terminal=done` |
| `NeedsUser` | `outcome=needs_user` | `verification_terminal=needs_user` |
| `Blocked` | `outcome=blocked` | `verification_terminal=blocked` |
| `Stagnated` | `outcome=blocked` | `verification_terminal=stagnated`。自由文 prefix に依存しない |
| `BudgetExhausted` | `outcome=budget_exhausted` | `verification_terminal=budget_exhausted` |
| `Failed` | 既存 top-level `ClientResponse::Error`（原因に応じた既存 `ErrorCode`） | bounded / sanitized `message` |
| `Cancelled` | 既存 top-level `ClientResponse::Cancelled` | bounded / sanitized `reason` |

したがって既存 wire consumer は従来4 outcome または top-level error / cancelled のまま処理でき、新 consumer は `verification_terminal` で Blocked と Stagnated を型付きで区別できる。Failed / Cancelled に completion report を後付けせず、既存 top-level cancellation / error lifecycle を変更しない。`unknown` / `not_applicable` は `unsatisfied_criteria` へ混ぜず、追加 `evaluation_status` と `unverified_items` で表現する。

### 9.3 AC のテスト可能性自己点検

| AC | 主なテストレベル | 観測可能な判定点 |
|----|------------------|------------------|
| `delegated_verification_vertical_e2e` | fake provider / Worker / tool の application E2E | query 2回、Worker 2回、親観測2回、Gap、criterion、Done |
| `parent_contract_owns_delegated_completion` | domain / application 単体 | Contract ID 不変、verified=false、自己申告の非Done |
| `parent_reobserves_artifacts_and_external_state` | tool adapter 統合 | plan 固定、exact match、観測順、path rejection、command status、provenance |
| `evidence_precedence_and_conflicts_fail_closed` | domain table test | 優先順位、矛盾、stale / unrelated Evidence |
| `criterion_evaluation_is_exhaustive_and_structured` | schema / domain table test | 四状態、集合一致、not_applicable 条件、成功不変条件 |
| `gap_follow_up_is_single_bounded_and_same_worker` | application 統合 | Gap fields、Worker identity、approval 回数、retry 上限 |
| `follow_up_repeats_verification_and_detects_stagnation` | fake Worker / tools 単体 | verification plan 再使用、fingerprint、Stagnated / NeedsUser |
| `verification_terminal_outcomes_are_distinct` | domain / wire table test | 一つの入力に一つの7 outcome、既存4 outcome / top-level response への projection |
| `verification_preserves_existing_boundaries_and_human_task` | regression / architecture test | 0068 / 0069 契約、human_task 公開挙動、禁止依存 |
| `verification_report_is_bounded_and_auditable` | presenter snapshot / schema test | 必須 field、上限、sanitize / redaction |

## 10. Deferred specs

| 候補 | 0070 から分離する理由 |
|------|------------------------|
| Multi-worker / Parallel Verification | 複数実行主体、fan-out / fan-in、多数決、結果統合が必要になるため |
| Adaptive Repair Iteration | 2回以上の retry、Worker 選択、budget policy、より大きな停滞状態機械が必要になるため |
| Durable Verification Resume | 永続 aggregate、schema migration、lease、reconciler、crash recovery が必要になるため |
| Human Task Cross-suspend Independent Verification / Follow-up | 0068 の Suspended evaluation skip と 0063–0066 の context / resume / continuation を統合し、元 Contract の保持と Human Task 再開意味論を設計する必要があるため |
| PR / CI Verification Adapters | 外部 service、長期監視、結果不明状態、専用 workflow を伴うため |
| Nested Delegation / Task Graph | 再帰深度、循環検出、Graph / DSL が必要になるため |
| OS-level Verification Sandbox | 任意 Worker / command の filesystem、network、credential 隔離は独立 security feature のため |

## 11. Scope change log

| Revision | 分類 | 変更 | 理由 |
|----------|------|------|------|
| 1 | INITIAL | Issue #27 を親による独立再観測、criterion 四状態評価、一つの Gap、同一 Worker への最大1回の再委譲、request-local 状態に限定 | 0069 が Deferred した独立検証・自動修正反復を一つの bounded vertical slice で受け取り、並列・永続化・専用 verifier・開発 workflow を分離するため |
| 2 | `BLOCKER_ORIGINAL_AC` / `REGRESSION` / `SAFETY_WITHIN_FAULT_MODEL` | 委譲前に固定する structured Verification Plan、通常 shell と exact-match command の信頼境界、四状態 applicability、Gap→既存 AgentTaskRequest 変換、query budget 2、7終端の additive wire projection、Human Task 回帰境界、draft checker staged-state を固定 | Step 2 review: 0068 の任意 shell 非信頼・Suspended / query budget、0069 wire schema、既存4 outcome / top-level cancel/error を壊さず Issue #27 の Agent Task vertical slice を実装可能にするため |
| 3 | SCOPE_LOCK | 設計書 §9 の全10 AC、vertical slice、Complexity inventory を実装前に固定 | Step 3 で pending acceptance tests と 1:1 対応させ、以後の scope drift を機械検査するため |
| 4 | COMPLETION | 全10 AC 緑・verify + smoke 成功後に feature status=`done`、実装指示書を `docs/done/` へ移動 | Step 8 コミット準備。scope 数値と locked AC は不変 |
| 5 | `BLOCKER_ORIGINAL_AC` / `SAFETY_WITHIN_FAULT_MODEL` | 検証 command の実行後失敗を plan 未実行から分離し `Verification(verified=false)` へ。MVP で command item を1件に制限 | PR #28 review: 非 zero を InvalidRequest にしない §9.1 と、複数 command の stale / 照合衝突を固定するため |

## 12. `docs/architecture.md` への影響

実装時に `docs/architecture.md` の Task Completion / Agent Task 節へ、親 Contract ownership、structured Verification Plan、exact-match command 境界、委譲 Result の `verified=false`、Evidence 優先順位、criterion 四状態、Gap follow-up budget 1、7終端と既存 wire projection を追記する。Task Contract / CompletionReport の optional additive DTO は同書の stdio JSON schema と `aibe-protocol` を同期する。verification command output と Artifact references の保持・表示に合わせて `docs/security.md` も同じ変更で更新する。本 Step 2 では実装前のため architecture 正本は変更しない。

## 13. 未確定事項

- **推測:** Issue #27 の「指定検証コマンド」は、委譲前に親 Task Contract の structured Verification Plan へ固定し、既存 `shell_exec` allowlist / approval 経路で実行する command であり、Worker が任意 command を追加して親権限で実行させる要求ではないと解釈した
- **推測:** Issue #27 の Phase 3 全体では `human_task` も独立再観測の対象だが、Minimum Vertical Slice と Depends on #24 に従い、0070 MVP の新規経路は `agent_task` の同一 Workerだけを対象とする。Human Task cross-suspend 独立検証は既存外部挙動を変え得るため Deferred と解釈した
- Git 管理外 workspace では `git_status` / `git_diff` failure を `unknown` Evidence とし、Verification Plan に固定した Artifact reference の直接観測と command item を実行する。criterion に必要な独立 Evidence が得られなければ fail-closed に `NeedsUser` / `Blocked` とするため、Git fallback の追加機構は設けない
