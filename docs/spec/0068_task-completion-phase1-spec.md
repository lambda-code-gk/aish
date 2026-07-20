# 0068 Task Completion Phase 1 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **起票**: 2026-07-20  
> **関連**: GitHub Issue #18（Parent: #17）、[`0001_aibe-tool-agent-loop-spec.md`](../done/0001_aibe-tool-agent-loop-spec.md)、[`0006_max-tool-rounds-terminator-spec.md`](../done/0006_max-tool-rounds-terminator-spec.md)、[`docs/feature-development-policy.md`](../feature-development-policy.md)、[`0045_pack-composition-spec.md`](0045_pack-composition-spec.md)

## 0. Core outcome

一つの `ai` コマンド内で、LLM の終了文ではなく完了条件ごとの Evidence を根拠に未達 gap を埋め、達成状態または理由付きの非達成状態まで既存 Agent loop を継続できる。

## 1. Minimum vertical slice

```text
ユーザー要求
→ 既存 Query Loop を1回実行（最初の assistant 応答で Task Contract を確定）
→ Task Contract（Goal / Completion Criteria / Constraints / Deliverables / Verification）を固定
→ tool result と副作用後の再観測を Evidence ledger へ追加（初期 verified=false）
→ 同じ Query Loop の最終 assistant 応答を Completion Evaluator envelope として検査し、criteria ごとの satisfied / unsatisfied と不足 Evidence を得る
→ 未達なら同一質問を再送せず next_objective による gap-driven continuation
→ 既存 Query Loop をもう1回実行
→ 再評価
→ Done / NeedsUser / Blocked / BudgetExhausted と Evidence・未検証事項を最終表示
```

Phase 1 は一つの `ai` process と一つの request の生存中だけ Task Contract、Evidence ledger、評価履歴を保持する。外側の Task Completion Loop は既存 Query Loop を直列に呼び出し、Query Loop 内の provider 呼び出し、tool round、approval、timeout、streaming、`max_tool_rounds` の責務を変更しない。

Phase 1 の query budget は **固定値 2** とする。ここで 1 query は Task Completion Loop が既存 Query Loop を開始する1回を指し、Query Loop 内で tool result を provider へ返す provider round 数とは別である。公開設定・環境変数による変更は Phase 1 に含めない。Task Contract は初回 Query Loop の最初の assistant 応答に含む構造化 envelope で、最初の tool 実行を受理する前に生成・確定する。各 Query Loop の最終 assistant 応答は評価 envelope を兼ね、Contract 生成または Completion 評価だけを目的とする Query Loop / provider 呼び出しを追加しない。

## 2. Fault model

### 2.1 保証対象

標準 Fault Model に従う。単一ホスト・単一ユーザー・単一の正常な `ai` / `aibe` process 生存中に、次を保証する。

- Task Contract と criterion ID は command 内で安定し、Evaluator は Contract にない criterion を達成扱いしない
- assistant の終了文、計画、自己申告、tool 呼び出し要求だけでは Evidence を `verified=true` にしない
- assistant が生成した Deliverable 本文は `source=deliverable` の候補 Evidence にできるが、「完了した」「検証済み」といった自己申告とは分離し、Contract が本文そのものを成果物に指定した criterion にだけ関連付ける
- 変更系 tool の成功は「副作用を要求した事実」であり、変更後に得た read-only tool result、検査結果、または同等の再観測がなければ当該 criterion を満たさない
- Evaluator の構造化出力はコードで schema、criterion ID、Evidence 参照、終端不変条件を検査し、意味判定だけを LLM に委ねる
- query budget、同一失敗の反復、Evidence 集合の不変を process-local に検出し、無制限に継続しない
- すべての終端で、利用した Evidence と未検証事項をユーザーへ表示する

### 2.2 保証対象外

- `ai` / `aibe` crash、socket 切断、OS 再起動後の Task Contract / Evidence / query budget の復元
- 複数 `ai` command、複数 process、複数 host をまたぐ completion coordination
- 外部副作用の exactly-once、結果不明状態の自動解消、transaction / rollback
- 任意の shell command が行った変更対象を完全に推論すること
- LLM の意味判定が常に正しいこと（コードで検査できる構造・集合・上限・証拠参照は LLM に委ねない）
- provider 障害、tool timeout、ユーザー拒否を command 外で自動 retry / resume すること

## 3. Non-goals

- Task Contract / Evidence の永続 store、resume、履歴検索
- query budget の公開設定、環境変数、適応的な増減
- Agent 委譲、side agent、secondary agent loop、並列 query / tool 実行
- Human Task の新規統合、0062–0066 checkpoint / continuation lifecycle の変更
- 開発工程全体の状態機械、Planner DSL、汎用 workflow engine
- filesystem watcher、reconciler、lease / heartbeat、journal、schema migration、exactly-once
- 既存 Query Loop の tool round、approval、provider adapter、streaming protocol の再実装
- すべての completion criterion をコードだけで判定すること
- Phase 1 で query budget を適応的に増減すること

### 3.1 既存 Query Loop との責務境界

| 境界 | Task Completion Loop（0068） | 既存 Query Loop（0001 / 0006 / 0007） |
|------|------------------------------|---------------------------------------|
| 入力 | 初回はユーザー要求、2回目は固定済み Task Contract、前回までの Evidence、`next_objective` | 1 query の messages / tools / context |
| 制御 | query を開始するか、完了評価するか、終端するか | provider 応答と tool call を tool round 上限まで進める |
| 証拠 | tool execution record と観測結果を criterion に関連付ける | tool を実行し raw result / status を返す |
| 上限 | query budget（初期値 2）、停滞判定 | `max_tool_rounds`、tool timeout、approval |
| 終端 | Done / NeedsUser / Blocked / BudgetExhausted | `AgentTurnStatus::Ok` / `MaxToolRounds` / error |

`AgentTurnStatus::Ok` は「1 query が正常終了した」ことだけを表し、Task Completion の Done を意味しない。`MaxToolRounds` や query error は Evaluator / 停滞判定への入力であり、直ちに Done へ写像しない。Task Completion Loop は `aibe` application 層に置き、LLM・tool 実行を `ai` へ移さない。`ai` は既存どおり request context の送信、ユーザー承認、最終表示を担当する。

Completion Evaluator は独立した agent loop、実行主体、provider adapter ではない。既存 Query Loop の assistant 応答 envelope に含まれる意味判定を `aibe` application / domain 層が fail-closed に検査する境界である。初回 envelope は固定する Contract と tool call を、各 query の最終 envelope は評価結果を含む。既存 Query Loop が一つの query 内で行う provider round と tool round は既存責務のままとし、0068 は別の provider 呼び出し経路を作らない。

## 4. Complexity inventory

| 項目 | 数・内容 |
|------|----------|
| 実行主体 | 2（`ai` client、既存 `aibe` request / Query Loop 実行主体） |
| 状態機械 | 2（新規 Task Completion lifecycle、既存 Query Loop） |
| 永続 aggregate | 0（Task Contract / Evidence ledger / 評価履歴は command-local） |
| 外部副作用 | 2（既存 provider query、既存 tool execution。0068 固有の新規 effect は追加しない） |
| プロセス境界 | 1（既存 `ai` ↔ `aibe` Unix socket） |
| 新規基盤機構 | 1（task-completion-loop） |
| 他機能統合 | 1（既存 Query Loop） |

## 5. Complexity Gate

- 判定: **Yellow**
- 理由: 既存 Query Loop の外側に Task Completion lifecycle を一つ追加するため、実行経路全体の状態機械数が 2 となり Yellow 閾値に達する。永続 aggregate、新規 process boundary、二次 agent、並列性、crash recovery は追加せず、novel mechanism は Task Completion Loop 一つに限定する
- 分割判断: Contract、Evidence、Evaluator、gap-driven continuation は一つの vertical slice を成立させる同一機構なので Phase 1 に含める。永続化、resume、委譲、Human Task 統合、並列、watcher は後続 spec へ分離する
- scope review: **approved** — Issue #18 の最小 vertical slice を query budget 2、command-local 状態、既存 Query Loop の直列再利用に限定する
- 承認例外: 不要（Red ではない）

`secondary_agent_loop=false` とする。新設するのは同じ agent / request が既存 Query Loop を直列再利用する completion orchestration であり、独立した会話・tool set・実行主体を持つ二次 agent ではない。ただし実装中に独立 agent、二つ目の新規状態機械、または別 conversation が必要になった場合は STOP-THE-LINE とする。

## 6. Complexity budget

| 項目 | 追加可能な上限 |
|------|----------------|
| 実行主体 | +0 |
| 状態機械 | +0（新規は Task Completion lifecycle の1つだけ） |
| 永続 aggregate | +0 |
| 外部副作用 | +0（既存 provider / tool port を再利用） |
| プロセス境界 | +0 |
| 新規基盤機構 | +0 |
| 他機能統合 | +0（既存 Query Loop のみ） |

wire DTO の追加が必要な場合も既存 `ai` ↔ `aibe` request / response 境界内に限定し、新しい RPC、socket、process は追加しない。

## 7. Split triggers

次のいずれかが必要になった時点で STOP-THE-LINE とし、0068 に追加しない。

- Task Completion lifecycle 以外の新しい状態機械、独立 agent loop、conversation、実行主体
- Contract / Evidence / evaluation の disk 保存、resume、migration、GC
- query または tool の並列化、Agent 委譲、Human Task lifecycle 統合
- lease / heartbeat、reconciler、watcher、journal、idempotency key、exactly-once
- 外部副作用の結果不明状態を command 外で自動照合する仕組み
- Planner DSL、汎用 workflow engine、開発工程 state machine
- 既存 Query Loop や provider adapter の置換
- Contract 生成または Completion 評価専用の Query Loop / provider 呼び出し
- integrations が 2 以上、external effects が 3 以上、process boundaries が 2 以上になる変更

## 8. パック構成の適用

**No** — 0045 §6 の適用候補を検討したが、Task Completion は全 provider / 全通常 Agent turn に共通する core の完了意味論であり、無効化した別 runtime profile、専用 RPC / CLI / turn hook の束、重い optional dependency、optional 配備を目的としない。Active / Basic Pack で無効時に従来の「LLM 終了文を完了とみなす」経路を残すと安全契約が二重化するため、Pack 境界にはしない。通常の application port / domain 境界で Evaluator を差し替え可能にし、composition root は既存のものを使う。

## 9. Acceptance Criteria

| ID | 条件 |
|----|------|
| `task_completion_vertical_e2e` | deterministic mock provider / tools を使う一つの `ai` command の縦断試験で、初回 query の副作用後に criterion が未達となり、同一質問ではなく `next_objective` と不足 Evidence を含む2回目 query が再観測を行い、全 criterion の verified Evidence を伴う Done を返す |
| `task_contract_is_stable_and_complete` | Goal / Completion Criteria（安定 ID）/ Constraints / Deliverables / Verification を持つ Contract が初回 Query Loop の最初の assistant envelope で最初の tool 実行前に生成・固定され、後続 envelope で criterion の追加・削除・ID 変更を拒否する。Contract 生成専用の Query Loop / provider 呼び出しは発生しない |
| `assistant_claim_is_not_verified_evidence` | tool result のない assistant 終了文、完了自己申告、tool 未実行の計画を入力しても Evidence は `verified=false` のままで Done にならない |
| `side_effect_requires_post_observation` | 変更系 tool の成功だけでは対応 criterion を satisfied にせず、その副作用より後の read-only 観測または verification result を参照した場合だけ verified Evidence として満たせる |
| `completion_evaluator_is_structured_and_fail_closed` | 各 Query Loop の最終 assistant envelope が criterion ごとの `satisfied` / `unsatisfied`、`required_evidence`、`next_objective`、Evidence 参照を返し、未知 ID、欠落 ID、存在しない Evidence 参照、矛盾する終端をコードが拒否する。評価専用の Query Loop / provider 呼び出しを開始せず、機械判定可能な query 上限、集合整合、tool status は LLM 判定で上書きできない |
| `continuation_is_gap_driven_and_detects_plan_only` | 未達時の次 query は元要求の再送ではなく unsatisfied criteria・required evidence・単一の `next_objective` を含み、変更または検証を要求する Contract に対する plan-only 応答は未達として継続する。一方、Deliverable 自体が計画文書である Contract は内容 Evidence があれば plan-only と誤判定しない |
| `terminal_outcomes_are_distinct` | 全 criterion に verified Evidence がある場合だけ Done、ユーザー入力・承認・手動操作が必要なら NeedsUser、command 内で解消不能または同一失敗反復なら Blocked、それ以外の未達で query budget 到達なら BudgetExhausted となり、相互に混同しない |
| `progress_and_stall_are_bounded` | 初期 query budget 2 を超えて Query Loop を開始せず、各評価の unsatisfied 集合・Evidence fingerprint・正規化 failure を比較して、新規 Evidence / criterion 改善を進展とし、Evidence 不変または同一失敗の2回目を停滞として記録する |
| `final_report_lists_evidence_and_unverified_items` | Done を含む全 outcome の最終 human / structured 表示に outcome、criterion 別 Evidence の観測元と verified 状態、unsatisfied criteria、未検証事項、query 使用数を含め、assistant 自己申告を観測済みとして表示しない |

### 9.1 判定順序と不変条件

1. Contract schema、criterion 集合、Evidence 参照、tool status、query budget をコードで検査する。
2. Contract の各 criterion に対する Evidence の意味的充足だけを Completion Evaluator（LLM）へ委ねる。
3. 全 criterion が valid な verified Evidence を持つ場合だけ `Done` とする。
4. 未達かつユーザーにしか解消できない入力・承認・操作が具体化されていれば `NeedsUser` とする。
5. 未達かつ command 内で解消不能な failure、または同一 failure の2回目なら `Blocked` とする。
6. 上記以外で query budget に達したら `BudgetExhausted` とする。
7. いずれにも達せず進展可能なら、Evaluator の単一 `next_objective` で次 query を開始する。

Evidence は最低限 `evidence_id`、`criterion_ids`、`source`（tool / observation / verification / deliverable）、`observed_after_effect`、`summary`、`verified` を持つ。`verified=true` は、コードが provenance・tool status・effect 後の順序を検査でき、かつ Evaluator が当該 criterion との意味的充足を認めたことを表す。外界の真実を LLM の自己申告だけで保証する印ではない。`source=deliverable` は Contract が assistant 生成本文そのものを Deliverable とする場合に限り、本文の存在・内容を Evidence にできるが、その本文中の完了自己申告や未実行の検証結果を Evidence に昇格させない。tool output 全文や秘密値を無条件に最終表示へ複製せず、既存の sanitize / bounded output 契約を維持する。

### 9.2 AC のテスト可能性自己点検

| AC | 主なテストレベル | 観測可能な判定点 |
|----|------------------|------------------|
| `task_completion_vertical_e2e` | mock socket / provider / tool の command E2E | query 数、2回目 prompt、tool 順序、Done、Evidence |
| `task_contract_is_stable_and_complete` | domain / application 単体 | field、ID集合、Contract 固定と最初の tool 実行の順序、provider 呼び出し回数、改変拒否 |
| `assistant_claim_is_not_verified_evidence` | Evaluator 境界単体 | verified=false、非Done |
| `side_effect_requires_post_observation` | application 統合 | effect / observation の順序、criterion 状態 |
| `completion_evaluator_is_structured_and_fail_closed` | schema / domain table test | invalid output の各拒否理由、機械判定優先 |
| `continuation_is_gap_driven_and_detects_plan_only` | prompt builder / application 統合 | 再送禁止、gap 内容、計画 deliverable の対照例 |
| `terminal_outcomes_are_distinct` | domain table test | 同一入力から一意の4 outcome |
| `progress_and_stall_are_bounded` | fake Query Loop / clock 不要の単体 | 最大2 query、fingerprint、failure counter |
| `final_report_lists_evidence_and_unverified_items` | presenter snapshot / structured schema | 全 outcome の必須 field、sanitize |

各 AC は単一の同名 Rust test function（内部の labeled subcase は可）へ 1:1 で登録でき、外部 API、実ユーザー入力、wall-clock sleep、永続 store に依存しない。Step 2 で `scripts/spec-acceptance.toml` へ `pending=true` で登録し、`#[ignore]` の RED test を追加して Scope Lock 済みである。実装時は AC 単位で本番経路を通るテスト本文へ置換し、達成した AC の `pending` と `#[ignore]` を同時に外す。

## 10. Deferred specs

- Task Contract / Evidence の永続 store と別 command からの resume
- adaptive query budget、長期停滞分析、cross-command progress history
- Agent 委譲、並列 completion、Human Task との新しい統合
- watcher / reconciler による外部状態の継続監視
- 副作用の exactly-once、transaction、結果不明状態の durable recovery
- Planner DSL、開発工程 state machine、汎用 workflow engine

## 11. Scope change log

| Revision | 分類 | 変更 | 理由 |
|----------|------|------|------|
| 1 | INITIAL | Issue #18 を command-local Task Contract / Evidence / Evaluator、query budget 2、既存 Query Loop の直列再利用に限定して仮 Scope Lock | Phase 1 の gap-driven completion vertical slice を成立させつつ、永続化・委譲・並列・Human Task 統合を後続へ分離するため |
| 2 | BLOCKER_ORIGINAL_AC / NEW_REQUIREMENT 除外 | Contract 固定を初回 tool 実行前へ明確化し、Evaluator を各 Query Loop の assistant envelope に限定。query budget は固定値 2 とし公開設定を Deferred へ移動 | Contract より先に副作用が起きる曖昧さと、評価専用呼び出し・設定追加が hidden scope になる余地をなくすため |

## 12. `docs/architecture.md` への影響

実装時に `docs/architecture.md` の Agent loop 節へ、既存 Query Loop の外側にある Task Completion Loop、Task Contract / Evidence ledger / Completion Evaluator の責務、4終端、query budget と `max_tool_rounds` の違いを追記する。wire DTO を追加する場合は同書の stdio JSON schema と `aibe-protocol` の正本を同じ変更で更新する。Evidence の表示・保持範囲が既存 security 契約へ影響する場合は `docs/security.md` も同期する。本 Step 1 では実装前のため architecture 正本は変更しない。

## 13. 未確定事項

- **未確定**: 汎用 shell command の副作用対象と再観測対象を関連付ける最小 metadata。Phase 1 では tool 種別、呼び出し順、criterion / Evidence ID の明示関連付けを下限とする
- **推測**: Issue #18 の「AI_MAX_QUERIES=2 相当」は公開環境変数の追加要求ではなく、Task Completion Loop が既存 Query Loop を開始できる回数を固定値 2 で制限する要求と解釈した。既存 Query Loop 内の provider / tool round と `max_tool_rounds` とは分離する
