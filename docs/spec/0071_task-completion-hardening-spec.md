# 0071 Task Completion Hardening 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定（実装済み）  
> **起票**: 2026-07-24  
> **関連**: GitHub Issue #29、[`0068_task-completion-phase1-spec.md`](0068_task-completion-phase1-spec.md)、[`0069_agent-task-delegation-spec.md`](0069_agent-task-delegation-spec.md)、[`0070_task-completion-phase3-verification-spec.md`](0070_task-completion-phase3-verification-spec.md)、[`docs/feature-development-policy.md`](../feature-development-policy.md)、[`0045_pack-composition-spec.md`](0045_pack-composition-spec.md)

## 0. Core outcome

Task Completion 対象 turn が取消・検証基盤障害・複数 criterion の未達・大量 streaming・観測精度限界に遭遇しても、実観測に基づく bounded な CompletionReport と安全な終端へ到達できる。

## 1. Minimum vertical slice

```text
Task Completion 対象 turn
→ 既存 Task Contract / Delegated Verification Plan を固定
→ Worker 実行と親 verification を既存経路で実施
→ 実行済み非 zero command は Verification Evidence（verified=false）として記録
→ criterion ごとに実 Evidence / 観測状態から observed、固有の required_work、再検証 plan item を作る
→ bounded Gap を同じ Worker への既存1回 follow-up request へ変換
→ 再検証して Done または既存の理由付き終端へ到達
→ 途中で明示取消なら verification_terminal=Cancelled、検証基盤が実行不能なら Failed を持つ bounded CompletionReport を top-level 応答へ添付
```

同じ経路で `CompletionEventBuffer` は request-local の byte / item 上限を超えず、overflow 時は buffered assistant delta 全体を破棄して control JSON を表示しない。既存 Workspace Observer は file content を読まず、Unix metadata fingerprint を精密化して同一サイズ書き換えを検出し、確証できない対象があれば `observation_incomplete=true` とする。

本 spec は 0068–0070 の本番 composition、Task Completion lifecycle、query budget 2、Agent Task follow-up budget 1 を変更しない。新しい actor、状態機械、永続 aggregate、provider call、Worker、observer service は追加せず、既存の report builder、Gap builder、event sink、workspace snapshot adapter の hardening に限定する。

### 1.1 Failed / Cancelled の CompletionReport 投影

0070 の七終端を本番 `CompletionReport` へすべて接続する。`ClientResponse::Error` / `ClientResponse::Cancelled` は top-level variant と既存 error code / reason を維持し、optional additive な `completion_report` を持てるようにする。field は `#[serde(default, skip_serializing_if = "Option::is_none")]` とし、Task Completion 対象外と Failed ではない既存 Error では省略して従来 JSON を byte shape 上も維持する。旧 payload は新 consumer が `None` として読め、現行 Rust / serde の旧 consumer は top-level variant の未知 field を無視して従来の Error / Cancelled として読めることを protocol regression test で固定する。

Rust enum variant への field 追加は Rust source compatibility を持たないため、workspace 内の全 constructor と `..` のない pattern match は同じ実装変更で更新する。これは新しい response variant や process boundary を増やすものではなく、wire 上の optional additive 変更である。`VerificationTerminal` には `Failed / Cancelled` を追加するが、旧 consumer が未知 enum 値を直接読む経路を作らず、旧 consumer が無視できる新しい top-level `completion_report` 内にだけ出力する。

Task Completion 対象 turn では、明示取消または下記判定表で Failed に該当する検証基盤障害の応答を返す直前に、一つの共通 terminal report builder を通す。provider error、入力 schema error、tool policy 拒否一般を一律に Failed report へ変換しない。

- `Cancelled`: ユーザーまたは既存 approval / turn cancellation の明示取消にだけ用い、`verification_terminal=cancelled` とする
- `Failed`: fixed plan と exact match した verification command の spawn、開始後の結果取得、timeout / signal 後の cleanup / reap など server-owned の検証実行基盤が成立せず、criterion に否定的観測さえ渡せない場合に限定し、`verification_terminal=failed` とする
- Contract 固定後の Cancelled / Failed は全 criterion を既存 Evidence と `unknown` / `unsatisfied` 状態付きで投影する。Contract 固定前にも起こり得る Cancelled は空の criteria と「Contract 固定前」を示す bounded `unverified_items` を持つ。Failed は fixed Verification Plan に対する基盤障害なので Contract 固定後にだけ成立する
- 既存 `CompletionOutcome` に `Failed / Cancelled` を混入させず、legacy `outcome=blocked` と additive `verification_terminal` で機械的に区別する。新 consumer は必ず `verification_terminal` を優先する
- reason、criteria、Evidence、Gap、unverified item は既存 sanitize と item / field / total byte 上限を通し、raw provider output や control envelope を複製しない

Failed と既存終端の境界は次で固定する。

| 事象 | 分類 | 理由 |
|------|------|------|
| assistant envelope / schema / Evidence 参照不正、plan mismatch、cwd / allowlist 検査拒否 | 既存 `InvalidRequest` / `ToolNotAllowed` 等 | 0068 / 0070 の入力 fail-closed 契約を維持し、検証基盤障害と偽らない |
| approval の明示取消、turn cancellation | `Cancelled` report | ユーザーの明示取消である |
| exact-match command の spawn が OS / I/O error で成立しない | `Failed` report | command の成否を観測する基盤自体が実行不能 |
| 起動済み command が exit nonzero | `Verification(verified=false)` → `unsatisfied / unknown` → Gap | 検証基盤が得た有効な否定的観測 |
| 起動済み command が timeout / signal となり cleanup / reap と終了状態取得に成功 | `Verification(verified=false)` → `unknown` → Gap | 完了成功ではないが、実行済みの bounded な観測を得ている |
| 起動後の result acquisition または cleanup / reap が失敗し、構造化終了状態を得られない | `Failed` report | 結果不明を通常の Gap として偽装しない |
| Workspace Observer が bounded / permission / race 制約で完全観測できない | `unknown` + `observation_incomplete` → Gap | observer は fail-closed な typed 観測を返せている |

したがって、provider が返した malformed schema や Gap 入力の criterion 不整合は既存どおり `InvalidRequest` であり `Failed` へ再分類しない。valid な server-owned verification execution state を criterion 評価へ正規化できない内部不変条件違反だけは、raw state を表示せず `InternalError` + `verification_terminal=Failed` の最小 report へ変換する。

### 1.2 Criterion 単位の Gap

delegated evaluation の actionable な `unsatisfied / unknown` criterion は、bounded な非空 `required_work` を criterion ごとに持つ。0068 の `required_evidence` は「達成判定に追加で必要な Evidence」の記述として残し、Gap の `observed` や作業指示へ転用しない。非委譲 turn の schema 互換のため criterion-level `required_work` は optional additive とするが、0070 Active で actionable な criterion には必須とする。

Gap entry は次の決定的な入力から構築する。

| field | 正本 |
|-------|------|
| `criterion_id` | 固定済み親 Contract の criterion ID |
| `observed` | 当該 criterion が参照した実在 Evidence の sanitized summary、verification exit / observation failure の構造化状態、または `observation_incomplete` / plan 未実行など server が記録した観測状態 |
| `expected` | 固定済み親 Contract の criterion description |
| `required_work` | 当該 criterion evaluation の bounded `required_work` |
| `verification_plan_item_ids` | 固定済み plan で当該 criterion を覆う item ID |

`observed` は LLM の `required_evidence` 文言から生成しない。Gap builder は `contract / evaluation` だけでなく、検査済み Evidence ledger と verification / observation の構造化実行状態を受け取る。参照 Evidence は `evidence_id` の安定順に選び、当該 criterion に関連付いた non-stale Evidence だけを bounded summary へ投影する。参照 Evidence がない `unknown` では、コードが確認した `observation_incomplete`、plan 未実行、矛盾、観測なしのいずれかを型付き理由から表示する。理由も Evidence もない入力、未知・stale・別 criterion の Evidence 参照は既存の invalid evaluation として拒否し、provider 入力不正を `Failed` と偽装しない。複数 criterion で同じ global `next_objective` を `required_work` として複製せず、各 `required_work` の非空・byte 上限と criterion ID 集合一致を検査する。

Gap follow-up request は既存 0070 変換を維持し、各 entry について `criterion_id / observed / required_work / verification_plan_item_ids` を一つの bounded instruction item にする。completion criteria も同じ criterion ごとに構築する。entry / instruction / request の item・field・合計 byte 上限を超えた場合は途中 truncation や criterion 欠落を行わず `Blocked` とし、再委譲 budget は1回のままとする。

### 1.3 CompletionEventBuffer の bounded memory

`CompletionEventBuffer` は request ごとに `buffered_bytes`、`delta_count`、`overflowed` を保持し、UTF-8 byte の合計上限と item 上限を両方適用する。上限内では既存どおり最終 envelope 判定まで delta を保持する。次の delta でどちらかの上限を超える場合は、partial delta を保存せず既存 buffer 全体を clear し、`overflowed=true` として以後の assistant delta を破棄する。

overflow 後は `AgentTurnResult`、`Suspended`、`Error`、`Cancelled` のいずれでも buffered delta を replay しない。成功時に表示できるのは構造検査後の bounded / sanitized `assistant_message.content` だけであり、control marker / JSON を含む場合は表示しない。progress と final response 通知は既存どおり転送する。空 delta の大量投入でも item 上限により `Vec` が無制限に増えない。buffered assistant delta は既存 `ENVELOPE_MAX_BYTES` と同じ **64 KiB**、delta item は **1024件**を上限として一か所の定数で固定し、設定や query budget は増やさない。

### 1.4 Workspace Observer の同一サイズ書き換え

既存の前後 snapshot と `MAX_OBSERVED_PATHS / MAX_CHANGED_PATHS` を維持し、Unix の `symlink_metadata` から file kind、length、device / inode、mtime seconds / nanoseconds、ctime seconds / nanoseconds を持つ metadata fingerprint を記録する。regular file、symlink target を含む file content は読まず、hash / digest も取得しない。これにより 0069 の「changed-file observation は bounded metadata、file content を暗黙収集しない」という信頼境界を維持する。

size と mtime が同じでも ctime nanoseconds、inode、file kind のいずれかが変われば changed path とする。metadata field の取得不能、snapshot 中の entry 消失 / read race、対象 filesystem / platform で nanosecond change time を信頼できない場合、または path 数上限超過により同一サイズ rewrite を否定できなければ `observation_incomplete=true` とする。path 数上限超過時の truncate + incomplete、秘密値を含む path の除外、`verified=false` は 0069 の契約を維持する。Unix 以外の fallback や content hashing は導入しない。

## 2. Fault model

### 2.1 保証対象

標準 Fault Model に従う。単一ホスト・単一ユーザー・正常な `ai` / `aibe` process 生存中に、次を保証する。

- Task Completion 対象 turn の明示取消と検証基盤実行不能を、bounded CompletionReport の typed `verification_terminal` で区別する
- verification command の実行後非 zero を実 Evidence として Gap へ回し、実行基盤障害と混同しない
- Gap の `observed`、`required_work`、再検証 plan item を固定 criterion ごとに対応させる
- streaming delta と workspace fingerprint のメモリ / I/O / item 数を有限にし、上限到達時は情報漏洩や偽陰性より fail-closed な非表示 / incomplete を選ぶ
- Phase 1–3、Human Task、Task Completion 対象外の通常 turn の既存外部挙動を維持する

### 2.2 保証対象外

- `ai` / `aibe` crash、socket 切断、OS 再起動後の report、buffer、snapshot、Gap の復元
- 複数 process / host、並列 observer、長期 watcher、exactly-once
- metadata capability / path budget 外の file が変更されていないことの保証（`observation_incomplete` で表す）
- verification command や Completion Criteria が意味的に十分な coverage を持つこと
- LLM が生成する criterion ごとの `required_work` が常に最善であること（構造、対応、上限、Evidence provenance はコードで検査する）

## 3. Non-goals

- 意味的 coverage の自動保証
- `expected_success` の一般 Predicate / DSL 化
- 0068 の query budget 増加、0070 の tool / command item 上限緩和
- 複数 Worker、並列検証、Worker 切替、2回以上の再委譲
- 新しい actor、状態機械、agent loop、observer service、汎用 retry controller
- 永続化、crash recovery、journal、lease / heartbeat、reconciler、schema migration
- filesystem watcher、長期監視、外部 CI / PR adapter
- Human Task cross-suspend 独立検証または Human / Agent lifecycle の統合
- file 内容、content digest、provider control JSON、秘密値の report / log 出力

## 4. Complexity inventory

| 項目 | 数・内容 |
|------|----------|
| 実行主体 | 3（既存 `ai` client、既存 `aibe` Task Completion / Query Loop、既存 0069 Worker process） |
| 状態機械 | 2（既存 Task Completion lifecycle、既存 Query Loop。0071 固有は 0） |
| 永続 aggregate | 0（report / Gap / buffer / snapshot は request-local） |
| 外部副作用 | 4（既存 provider query、Worker process、workspace / Git 観測、verification command） |
| プロセス境界 | 2（既存 `ai` ↔ `aibe` socket、既存 child process 境界） |
| 新規基盤機構 | 0（0070 production path の hardening のみ） |
| 他機能統合 | 3（0068 Task Completion、0069 Agent Task、0070 Independent Verification） |

数値は `scripts/feature-scope.toml` の 0071 entry と一致する。Workspace Observer の metadata fingerprint 精密化は既存 snapshot adapter 内の観測精度改善であり、新しい watcher / actor / external effect には数えない。

## 5. Complexity Gate

- 判定: **Yellow**
- 理由: 0070 と同じ既存経路を扱うため、実行主体 3、状態機械 2、外部副作用 4、process boundary 2、integration 3 が Yellow 閾値に達する。一方、0071 固有の novel mechanism は 0、永続 aggregate は 0 で、Red flag はすべて false とする
- 分割判断: Issue #29 の4項目は 0070 統合後に露出した同一 production completion report / Evidence 観測境界の不変条件を閉じる hardening である。終端だけを分離すると Gap / buffer overflow 時に同じ bounded report 契約を二重化するため一 spec に保ち、意味 coverage、budget 拡張、複数 Worker、永続化、長期監視は Deferred とする
- scope review: **approved** — 既存 0068–0070 の actor / lifecycle / effect / boundary 数を増やさず、report builder、Gap builder、event buffer、workspace snapshot adapter の補正だけに限定する
- 承認例外: 不要（Red ではない）

One Novelty Rule 上の新規機構はない。bounded byte accounting と metadata fingerprint はそれぞれ既存 collector / observer の有限化・精度改善であり、独立 service、watcher、store、状態機械へ昇格させない。

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

wire 変更は既存 `Error / Cancelled` variant への optional `completion_report`、`VerificationTerminal` の Failed / Cancelled、criterion evaluation の optional `required_work` に限定する。新 response variant、RPC、CLI、設定、Cargo dependency を追加しない。

## 7. Split triggers

次のいずれかが必要になった時点で STOP-THE-LINE とし、0071 に追加しない。

- 新しい actor、状態機械、agent loop、background observer / watcher
- report / buffer / snapshot / Gap の永続化、crash recovery、migration、journal
- query budget 増、command plan item 増、再委譲2回以上、複数 / 並列 Worker
- content hashing、workspace 全内容 index、継続監視、外部 service
- expected success の一般 Predicate / DSL、意味的 coverage checker、verifier Agent
- Human Task cross-suspend verification または共通 lifecycle aggregate
- integration 4 以上、external effect 5 以上、process boundary 3 以上、novel mechanism 1 以上になる変更

発生時は実装を停止し、`scope_revision`、Complexity inventory、Gate を更新して別 spec への分割を判断する。

## 8. パック構成の適用

**No** — 0045 §6 を検討したが、0071 は optional な機能束、専用 RPC / CLI / turn hook、重い依存、別配備単位を追加せず、既存 Task Completion 対象 turn の安全不変条件を補正する hardening である。無効化した Basic Pack が unbounded buffer、曖昧な Gap、偽陰性の workspace 観測、report のない取消・失敗を残す構成は安全契約を二重化するため、新しい Pack 境界、Active / Basic Pack、toggle、composition root は設けない。0069 `AgentTaskPack` が disabled なら委譲経路自体が非公開となる既存挙動を維持し、本変更は既存 core / adapter 境界に置く。

## 9. Acceptance Criteria

| ID | 条件 |
|----|------|
| `task_completion_cancel_report_vertical_e2e` | deterministic mock provider / tool の production composition 縦断試験で Task Completion 対象 turn を Contract 固定前 / 後に取消すと、top-level Cancelled を維持しつつ、serialized 128 KiB 上限内の CompletionReport が `verification_terminal=Cancelled` を持ち、control JSON や raw output を表示しない（AC1） |
| `verification_infrastructure_failure_reports_failed` | 固定 verification plan の exact-match command が spawn 不能、または起動後の result acquisition / cleanup / reap 不能で構造化終了状態を得られないとき、top-level Error と bounded CompletionReport の `verification_terminal=Failed` へ到達する。一方、schema / Evidence 参照不正は既存 InvalidRequest、観測可能な非 zero / timeout / signal は Gap となり混同しない（AC2） |
| `task_completion_hardening_vertical_e2e` | deterministic mock provider / Worker / tool の production composition 縦断試験で、exact-match verification command の非 zero を `Verification(verified=false)` として取り込み、複数 criterion の実 observed・異なる required_work・固定 plan item を持つ bounded Gap を同一 Worker への1回 follow-up request に変換し、同じ plan の再検証後に Done へ到達して Failed / InvalidRequest に短絡しない（AC3） |
| `gap_observed_uses_actual_evidence` | 各 Gap entry の `observed` は当該 criterion の実 Evidence summary または server の構造化観測状態から作られ、`required_evidence` 文言を観測済み事実として誤用しない（AC4） |
| `gap_required_work_is_per_criterion` | 複数の未達 criterion がそれぞれ非空・bounded・異なる `required_work` を保持でき、global `next_objective` の複製や criterion 間取り違えがない（AC5） |
| `gap_follow_up_request_is_per_criterion_and_bounded` | 再委譲 request が各 criterion の ID、observed、required_work、再検証 plan item を対応する bounded instruction / completion criterion として持ち、上限超過時は欠落・partial truncation・2回目再委譲を行わず Blocked になる（AC6） |
| `completion_event_buffer_has_total_byte_limit` | CompletionEventBuffer が delta 合計 64 KiB / 1024 item の上限を超えず、overflow 時は全 assistant delta を破棄してどの終端でも control marker / JSON を表示せず、progress / final 通知を維持する（AC7） |
| `workspace_observer_detects_same_size_rewrite_or_marks_incomplete` | 同一サイズ・同一 mtime 相当の regular file 書き換えを ctime nanoseconds / inode 等の Unix metadata fingerprint で changed path として検出し、metadata capability / permission / race / path budget により確証不能なら `observation_incomplete=true` とする。file content / digest は読まない（AC8） |
| `task_completion_hardening_preserves_regressions` | 0068 Phase 1、0069 Phase 2、0070 Phase 3、Human Task の Suspended / resume / continuation、Task Completion 対象外の通常 turn、既存 query 2 / follow-up 1 budget が不変である。Error / Cancelled の `completion_report=None` は field を serialize せず旧 JSON と一致し、旧 payload は新 consumer が読め、新 field を含む payload は現行 serde の旧 shape consumer が従来 top-level variant として読める（AC9） |
| `task_completion_hardening_verify_and_smoke` | 完了時に `./scripts/verify.sh` と `./scripts/smoke-mock.sh` が成功し、timing summary と smoke 結果が実装完了報告に記録される（AC10） |

`vertical_slice_ac_id` は **`task_completion_hardening_vertical_e2e`** とする。Step 3 で `scripts/feature-scope.toml` を `status="locked"` / revision 3へ更新して Scope Lock を成立させ、`scripts/spec-acceptance.toml` の `pending=true` 同名 ignored test 10件と一致させた。以後は各 AC を本番経路のテストで順次 green にする。

### 9.1 終端と Gap の判定順序

1. 明示取消なら `Cancelled` report を構築する。
2. Contract / evaluation / Evidence の bounded schema と参照整合をコードで検査し、不正入力は既存 `InvalidRequest` とする。
3. valid な fixed plan に対する verification command の spawn / result acquisition / cleanup / reap が成立せず終了状態を得られない場合は `Failed` report を構築する。
4. 実行済み verification command の非 zero / timeout / signal は否定的 Evidence として criterion 評価へ渡す。
5. 全 applicable criterion が satisfied なら Done、既存の NeedsUser / Blocked / Stagnated / BudgetExhausted 順序を維持する。
6. follow-up 可能なら、全 actionable criterion の実 observed と固有 required_work を持つ一つの bounded Gap を作る。

取消と基盤障害は Gap を捏造しない。非 zero と観測された未達は Failed を捏造しない。provider が返した Gap 入力の構造・参照不正は InvalidRequest とし、valid な入力から report を構築できない server internal invariant failure だけを Failed とする。

### 9.2 Bounded 契約

次の上限を一か所の定数として固定し、境界値・超過値をテストする。

- CompletionReport: serialized total **128 KiB**。terminal / outcome / queries、全 criterion ID / status を先に保持し、Evidence は criterion ごと8件・全体64件、Gap は既存最大32件、unverified item は32件、可変文字列は各1024 bytes 以下とする
- CompletionEventBuffer: buffered UTF-8 **64 KiB**、delta **1024件**
- Gap follow-up: 既存 `AgentTaskRequest` の item / field / aggregate 上限をすべて満たし、serialized request も **128 KiB** 以下とする
- Workspace snapshot: observed path count、changed path count、metadata error / race count（file content read budget は設けず、content を読まない）

sanitize 後の UTF-8 byte で数え、切り詰める場合は文字境界を守る。CompletionReport は terminal と全 criterion row を落とさず、Evidence / Gap summary / unverified item を安定 ID 順に採用し、省略があれば bounded な `unverified_items` の omission marker を1件予約する。既存 Contract 上限内でも terminal + 全 criterion row の最小 projection が 128 KiB に収まらない入力は tool 実行前に拒否する。ただし control envelope、criterion entry、safety instruction は partial truncation せず、Gap request では Blocked を選ぶ。

### 9.3 AC のテスト可能性自己点検

| AC | 主なテストレベル | 観測可能な判定点 |
|----|------------------|------------------|
| `task_completion_cancel_report_vertical_e2e` | application / protocol E2E | production composition、Cancelled variant、terminal、report total bytes、非表示 delta |
| `verification_infrastructure_failure_reports_failed` | application integration | spawn / internal failure、Error code、Failed terminal、bounded reason |
| `task_completion_hardening_vertical_e2e` | production composition E2E | process started、exit nonzero、criterion 別 Gap / follow-up、再検証、Done、非Error |
| `gap_observed_uses_actual_evidence` | domain table test | Evidence ID / summary、typed observation reason、required_evidence 非流用 |
| `gap_required_work_is_per_criterion` | schema / domain table test | 2 criterion、異なる required_work、ID 対応、blank / overflow 拒否 |
| `gap_follow_up_request_is_per_criterion_and_bounded` | application integration | instruction / criterion / plan 対応、request bytes、Blocked、follow-up count |
| `completion_event_buffer_has_total_byte_limit` | event sink async unit | byte / item 境界、overflow、Suspended / Error / Cancelled、marker 分割 |
| `workspace_observer_detects_same_size_rewrite_or_marks_incomplete` | Unix filesystem adapter | 同サイズ・同 mtime rewrite、ctime / inode 差、metadata capability / permission / race incomplete、content 非read |
| `task_completion_hardening_preserves_regressions` | regression / protocol / architecture | 0068–0070、Human Task、通常 turn、budget、optional field の omission / default、禁止依存 |
| `task_completion_hardening_verify_and_smoke` | repository acceptance | 同名 Rust test は smoke-mock が依存する mock production composition の通常 turn 回帰を process 内で検証する。AC の最終判定は、その test を含む verify timing summary と別プロセスの smoke-mock exit 0 の両方。test 内から `verify.sh` / `smoke-mock.sh` を再帰起動しない |

## 10. Deferred specs

| 候補 | 0071 から分離する理由 |
|------|------------------------|
| Semantic Verification Coverage | expected success の一般 Predicate、coverage 推論、意味保証という新機構が必要になるため |
| Adaptive / Parallel Repair | query budget 増、2回以上の follow-up、複数 Worker / 並列調停が必要になるため |
| Durable Completion Recovery | 永続 aggregate、schema migration、lease、reconciler、crash recovery が必要になるため |
| Long-running Workspace / CI Observation | watcher、background actor、外部 service、長期結果不明状態が必要になるため |
| Human Task Cross-suspend Verification | checkpoint / continuation と親 Contract ownership の新統合が必要になるため |

## 11. Scope change log

| Revision | 分類 | 変更 | 理由 |
|----------|------|------|------|
| 1 | INITIAL / `HARDENING` | Issue #29 の Failed / Cancelled report、criterion 単位 Gap、event buffer bounded memory、workspace 同一サイズ観測を、既存 0068–0070 production path の補正として設計固定 | 統合後に露出した終端・Gap・観測境界を actor / state machine / persistence / budget の追加なしで閉じるため |
| 2 | `BLOCKER_ORIGINAL_AC` / `REGRESSION` / `SAFETY_WITHIN_FAULT_MODEL` | Failed 判定表、Gap 本流の vertical AC、optional wire field 互換試験を固定し、Workspace Observer を content digest から metadata-only fingerprint へ修正 | Step 2 review: 0068 InvalidRequest、0069 content 非収集、0070 nonzero Gap、既存 consumer の契約を壊さず Issue #29 を実装可能にするため |
| 3 | SCOPE_LOCK | 設計書 §9 の全10 AC、vertical slice、具体的 bounded 定数、Complexity inventoryを実装開始前に固定 | Step 3でpending acceptance testと1:1対応させ、以後のscope driftを機械検査するため |

## 12. 実装時のドキュメント同期

実装時に `docs/architecture.md` の Task Completion wire projection / Gap / streaming 境界と Agent Task Workspace Observer 節を更新し、`docs/security.md` に control JSON 非表示、report total bound、metadata-only fingerprint / content 非read を追記する。`aibe-protocol` の additive field と `ai` presenter / JSON 表示を同じ変更で同期する。Step 3 は Scope Lock / 実装指示までのため、正本の現行挙動説明はまだ変更しない。

## 13. 未確定事項

- **推測:** Issue #29 の「本番 CompletionReport へ接続」は、既存 top-level `Error / Cancelled` variant を `AgentTurnResult` へ置換せず、optional additive `completion_report` を添付して既存 consumer と typed terminal の両方を維持する意味と解釈した
- **推測:** Workspace Observer の同一サイズ書き換えは 0069 の content 非収集を優先し、Unix ctime nanoseconds / inode 等で検出し、metadata 能力で確証できない場合に `observation_incomplete` を立てる選択を許すと解釈した
- CompletionReport / event buffer / Gap request の具体的 byte budget は §9.2 と実装指示書で `128 KiB / 64 KiB + 1024 delta / 128 KiB` に固定した。設定可能にしたり query budget を増やしたりしない
