# 0052 — `ai work` 作業文脈管理 実装指示書

> **種別**: 実装指示書（`docs/tasks/`）
> **設計の正本**: [0052_ai_work.md](../spec/0052_ai_work.md)
> **状態**: 実装指示書
> **起票**: 2026-06-28
> **関連**: [architecture.md](../architecture.md)、[security.md](../security.md)、[testing.md](../testing.md)、[docs/manual/](../manual/)、[`scripts/spec-acceptance.toml`](../../scripts/spec-acceptance.toml)、[`docs/0000_spec-index.md`](../0000_spec-index.md)

## 0. 目的

`docs/spec/0052_ai_work.md` を満たすため、`ai work ...` を `ai` の高レベル UX として追加し、作業文脈の保存・状態遷移・通常 turn への注入を `aibe` の Contextual Memory Pack に実装する。

`ai` は CLI、表示、Unix socket client に限定する。Work の状態、ID 採番、stack、整合性、永続化、prompt block 構築は `aibe` が所有し、`ai` に永続状態を置かない。既存の `ai goal` / `ai now` / `ai idea` / `ai mem` / `ai context` は削除せず、同じ memory space に対する低レベル API として維持する。

## 1. 実装前提として固定する設計

### 1.1 所有権と wire protocol

Work は複数クライアントから同じ状態を参照できる必要があるため、`aibe` を source of truth とする。既存 `MemoryOperationDto` だけでは `Paused / Deferred / Done`、`parent_id`、stack、複合状態遷移を原子的に表現できないため、汎用 Memory RPC を連続実行して代用しない。

`aibe-protocol` に次の専用 RPC を追加する。

- `ClientRequest::WorkApply(WorkApplyRequestBody)`
- `ClientRequest::WorkQuery(WorkQueryRequestBody)`
- `ClientResponse::WorkApplyResult { id, snapshot, outcome }`
- `ClientResponse::WorkQueryResult { id, snapshot }`

`WorkOperationDto` は次を表す。

- `Start { goal }`
- `Focus { text }`
- `AddEntry { kind: Idea | Note | Decision, text }`
- `Defer { text }`
- `Switch { work_id }`
- `Push { goal }`
- `Pop`
- `Finish`

dashboard / status / list は `WorkQuery` の同一 snapshot を表示用途別に整形する。CLI 表示用文字列を protocol や `aibe` domain に持ち込まない。

### 1.2 永続化

Work state は memory space 単位で次へ保存する。

```text
$AIBE_ROOT/memory/spaces/<memory_space_id>/work-state.json
```

保存対象は `schema_version`、`revision`、`next_work_id`、`active_work_id`、`stack`、`works`、`entries` とする。

- directory は `0700`、state file は `0600` とする。
- 既存 space の `.lock` と同じ排他境界で read-modify-write する。
- mutation は lock 取得後の最新 state に対して適用し、同一 directory の一意な temp file への完全書き込み、`0600` 設定、`sync_all`、rename、親 directory の同期の順で原子的かつ crash-safe に置換する。
- `next_work_id` は lock 内で単調増加させ、削除・完了後も再利用しない。
- file 不在は空 state として扱う。
- 未知の `schema_version`、壊れた JSON、state invariant 違反は明示エラーとし、既存 file を上書きしない。
- explicit Work RPC は保存エラーを返す。通常 turn の injection は既存 Contextual Memory と同様に best-effort とし、Work 読み込み失敗だけで turn 全体を落とさない。

### 1.3 ドメイン不変条件と状態遷移

`WorkStatus` は `Active / Paused / Deferred / Done / Abandoned` を持つ。ただし今回 `Abandoned` へ遷移する CLI は実装しない。

常に次を満たす。

1. `active_work_id` は 0 または 1 件である。
2. `active_work_id = Some(id)` なら対象 work の status は `Active` である。
3. `Active` status の work は最大 1 件である。
4. stack に重複 ID、存在しない ID、`Done / Deferred / Abandoned` は入らない。
5. stack 上の work は `Paused` であり、`parent_id` と stack 順が親子 chain を形成する。
6. `WorkEntry.work_id` は存在する WorkItem を参照する。

各操作の契約を次に固定する。

| 操作 | 前提 | 原子的な結果 |
|------|------|--------------|
| `start` | stack が空 | 旧 active を `Paused`、新 work を `Active` にする。active がなければ新規作成だけ行う |
| `focus` | active あり | active work の focus を置換する |
| `idea/note/decide` | active あり | active work へ対応 kind の entry を追加する |
| `defer` | 前提なし | 新しい `Deferred` work を作る。active と stack は変更しない |
| `switch` | stack が空、対象が `Paused` または `Deferred` | 旧 active を `Paused`、対象を `Active` にする |
| `push` | active あり | 旧 active を `Paused` にして stack へ積み、`parent_id` 付き child を `Active` にする |
| `pop` | stack が空でない | 現 child を `Done`、stack top を `Active` にして復帰する |
| `finish` | active あり、stack が空 | active を `Done`、`active_work_id` を unset する |

stack が残る状態で `start / switch / finish` は拒否し、先に `pop` するよう表示する。`defer` は active work がなくても成功する。`push` は active work がなければ失敗する。`pop` は child の entries を表示するが、親へ自動 merge しない。

### 1.4 既存 Contextual Memory との関係

仕様書 §8 の対応表は意味上の対応として実装する。Work state を複数の汎用 `MemoryApply` に二重書きしない。二重書きは途中失敗と switch 時の decision 再活性化を安全に扱えないためである。

- Work と generic memory は同じ `memory_space_id` 解決、filesystem root、permission、lock、Pack composition を再利用する。
- `ai work` の goal / focus / entries は WorkStore が正本である。
- 既存 `goal / now / idea / mem` の entries は引き続き generic memory として保持する。
- 通常 turn では active work block と generic memory block の両方を `aibe` が解決する。
- `ai work` 実装のために低レベル CLI の意味や既存データを変更しない。

この物理モデルは `docs/spec/0052_ai_work.md` §7–§9 と同期済みである。Phase 0 では実装詳細とのずれがないことを再確認し、`docs/architecture.md` に反映する。

### 1.5 通常 turn への注入

注入責務は `aibe` の `ContextualMemoryPack::prepare_turn_messages` に一本化する。`ai` の `RequestContext.system_instruction` へ Work 文脈を追加しない。

active work がある場合のみ、次を含む `[active work]` block を作る。

- goal
- focus（存在する場合）
- recent decisions 最大 3 件

Work block と既存 generic memory block の合計を既存 `MEMORY_PROMPT_BUDGET_BYTES` 内に収める。Work block を先に確保し、残量を generic memory に渡す。deferred、ideas、notes、done work、stack 上の paused work は通常注入しない。

`ask / chat / retry / rerun` はすべて同じ AgentTurn + TurnHook 経路を通し、個別の client-side 注入分岐を追加しない。

## 2. パック構成の適用

**部分適用**。新しい独立 Pack は作らず、既存 Contextual Memory Pack の client-side / server-side 境界を拡張する。

- memory enabled: `ContextualMemoryPack` が Work RPC、WorkStore、Work injection を提供する。
- runtime disabled: `BasicPack` が Work RPC を既存 memory-disabled error で拒否し、injection は no-op とする。
- `ai --no-default-features`: Work CLI は既存 memory stub 経由で fail-closed にする。
- composition root は既存 memory Pack 選択箇所の 1 か所を維持する。

disabled RPC、runtime disabled CLI、feature-off build / CLI の回帰を受け入れ条件に含める。

## 3. Phase 分割

| Phase | 内容 | ゲート |
|-------|------|--------|
| 0 | **実装・AC 完了（2026-06-28、全体 verify 未完了）**。全 AC の ignored test scaffold と registry 登録、wire DTO、Work domain / store、Pack disabled 経路、CLI parse / empty state | Phase 0 AC はすべて `pending = false` |
| 1 | `start / focus / idea / note / decide / defer`、dashboard / status / list | Phase 1 AC がすべて `pending = false` になるまで Phase 2 に進まない |
| 2 | `switch / finish`、stack 保護、エラー契約 | Phase 2 AC がすべて `pending = false` になるまで Phase 3 に進まない |
| 3 | `push / pop`、親子 chain、child entry 表示 | Phase 3 AC がすべて `pending = false` になるまで Phase 4 に進まない |
| 4 | TurnHook injection、低レベル memory 回帰、docs / manual / security、全体検証 | 全 AC が `pending = false` かつ `./scripts/verify.sh` が通る |

各 Phase 開始時は targeted 検証、Phase 完了時は当該 AC の `#[ignore]` と `pending` を同時に解除する。全 Phase 完了前に task を `docs/done/` へ移動しない。

## 4. 変更ファイル

### 4.1 protocol / client

| パス | 役割 |
|------|------|
| `aibe-protocol/src/work.rs`（新規） | Work DTO、operation、snapshot、outcome、validation limit |
| `aibe-protocol/src/request.rs` | `WorkApply / WorkQuery` request variant |
| `aibe-protocol/src/response.rs` | Work result variant |
| `aibe-protocol/src/lib.rs` | Work DTO export |
| `ai/src/ports/outbound/work_client.rs`（新規） | `ai` application が使う Work client port |
| `ai/src/adapters/outbound/aibe_client.rs` | Unix socket Work RPC adapter |

### 4.2 `aibe`

| パス | 役割 |
|------|------|
| `aibe/src/domain/work.rs`（新規） | state invariant、遷移、prompt projection |
| `aibe/src/ports/outbound/work_store.rs`（新規） | WorkStore port と error |
| `aibe/src/adapters/outbound/work_store.rs`（新規） | `work-state.json`、space lock、atomic replace、permission |
| `aibe/src/plugin_memory/work_service.rs`（新規） | Work RPC use case、space 解決、domain/store orchestration |
| `aibe/src/plugin_memory/contextual_memory_pack.rs` | Work service 配線と bounded Work block 注入 |
| `aibe/src/ports/outbound/rpc_extension.rs` | Work RPC を既存 Pack 境界へ追加 |
| `aibe/src/application/basic_memory_pack.rs` | disabled Work RPC 拒否、注入 no-op 回帰 |
| `aibe/src/application/request_service.rs` | `ClientRequest::WorkApply / WorkQuery` dispatch |
| `aibe/src/application/server.rs` と composition root 関連 | WorkStore を ContextualMemoryPack へ 1 か所で注入 |
| 各 `mod.rs` | 新規 module export |

既存 filesystem space layout / lock helper の共有に抽出が必要なら、`aibe/src/adapters/outbound/memory_space_fs.rs` を追加し、ContextualMemoryStore と WorkStore の双方から利用する。lock 実装を別々に持たせない。

### 4.3 `ai`

| パス | 役割 |
|------|------|
| `ai/src/clap_cli.rs` | `AiCommand::Work` と optional nested `WorkCommand`、全 subcommand、help |
| `ai/src/main.rs` | `run_work` composition、MemoryCliOptions と同じ socket/context 解決 |
| `ai/src/domain/work.rs`（新規） | dashboard / status / list の表示 model。永続 domain state は置かない |
| `ai/src/application/work_cli.rs`（新規 facade） | feature enabled implementation / feature-off stub の公開 |
| `ai/src/plugin_memory/work_cli.rs`（新規） | WorkClient 呼び出しと表示整形 |
| `ai/src/application/memory_stub.rs` | feature-off Work CLI の fail-closed 応答 |
| `ai/tests/work_cli.rs`（新規） | CLI E2E、mock socket、表示・状態遷移・エラー |
| `ai/tests/memory_disabled_cli.rs` | runtime disabled 回帰 |
| `ai/tests/phase_a_cli.rs` | 既存低レベル memory CLI 回帰 |

### 4.4 tests / docs

| パス | 役割 |
|------|------|
| `aibe-protocol/src/work.rs` | DTO roundtrip / unknown field rejection |
| `aibe/tests/work_rpc.rs`（新規） | 実 store を通る Work RPC integration |
| `aibe/tests/memory_pack_turn_hook.rs` | Work injection、budget、disabled/no-active 回帰 |
| `scripts/spec-acceptance.toml` | §7 の AC を test function と 1:1 登録 |
| `docs/spec/0052_ai_work.md` | §7–§9 の物理モデル、Pack 部分適用、注入責務を同期 |
| `docs/architecture.md` | Work RPC、ownership、store layout、TurnHook ordering |
| `docs/security.md` | permission、破損時 fail-closed、prompt budget、入力上限 |
| `docs/testing.md` | unit / integration / E2E / feature-off / manual |
| `docs/manual/ai-work.md`（新規） | 操作とエラーの手動確認 |
| `docs/manual/README.md` | manual 一覧 |
| `docs/0000_spec-index.md` | Phase 中は実装中、全完了後のみ実装済みへ更新 |

## 5. 実装手順

### 5.1 Phase 0: acceptance scaffold と基盤

1. §7 の全 AC を `scripts/spec-acceptance.toml` へ `pending = true` で登録する。
2. 各 `file_glob` に同名の `#[test] #[ignore]` test scaffold を作る。未実装型を参照せず `panic!("pending 0052")` だけで compile できる形から始める。
3. `./scripts/check-spec-acceptance.py` を実行し、missing test がないことを確認する。
4. `aibe-protocol/src/work.rs` と request / response variant を追加し、deny-unknown-fields、text byte limit、WorkId validation、serde roundtrip を固定する。
5. Work domain state と全 invariant validator を追加する。
6. WorkStore port と filesystem adapter を追加し、empty load、atomic replace、0600/0700、monotonic ID、破損 file 非上書き、複数 store instance の直列化をテストする。
7. `RpcExtension`、`ContextualMemoryPack`、`BasicPack`、request dispatch、composition root を配線する。
8. `ai` に WorkClient port / adapter と clap skeleton を追加する。`AiCommand::Work` の `command` は `Option<WorkCommand>` とし、引数なしを dashboard として扱う。
9. empty dashboard / status / list と全 subcommand parse を通す。
10. runtime disabled と feature-off の拒否メッセージを既存 memory CLI と揃える。
11. Phase 0 AC の ignore / pending を解除し、protocol / aibe / ai の targeted test と `cargo check -p ai --no-default-features` を直列実行する。

### 5.2 Phase 1: 基本操作と表示

1. `start` を単一 WorkStore mutation として実装し、旧 active pause と新 active 作成を同時 commit する。
2. stack 非空の `start` は state を変更せず拒否する。
3. `focus` は active 必須で、空文字・上限超過を拒否する。
4. `idea / note / decide` は active 必須で、entry kind と時刻を保存する。
5. `defer` は active の有無に依存せず、新しい `Deferred` WorkItem を作る。active / stack が byte-for-byte 同じであることをテストする。
6. dashboard / status は active、focus、stack、recent decisions、ideas、deferred、Suggested next の順を固定する。
7. list は Active / Paused / Deferred / Done の順で分類し、stack 上の work に marker を付ける。
8. stdout は人間向け表示、protocol error は stderr + non-zero exit とする。秘密情報を debug dump しない。
9. Phase 1 AC の ignore / pending を解除し、targeted test を通す。

### 5.3 Phase 2: switch / finish / error contract

1. `switch` は `Paused / Deferred` のみ受け付け、旧 active pause と対象 active 化を単一 mutation にする。
2. missing / Done / Abandoned work、stack 非空をそれぞれ state 非変更で拒否する。
3. `finish` は stack 空の場合だけ active を Done にして unset する。
4. active なしの `focus / idea / note / decide / push / pop / finish` を統一 error にする。
5. active ありの `start` が旧 active を Paused にする出力を固定する。
6. operation error 前後で `revision` と state が変わらないことを domain / RPC test で確認する。
7. Phase 2 AC の ignore / pending を解除し、targeted test を通す。

### 5.4 Phase 3: push / pop

1. `push` は active pause、stack push、child 作成、active 切替を単一 mutation にする。
2. nested push で `stack = [root, child1, ...]` と parent chain が一致することを保証する。
3. `pop` は current child を Done、stack top を Active、stack pop とする。
4. pop 出力には child の decisions / notes / ideas を kind 別に表示するが、親 entries は変更しない。
5. empty stack、壊れた parent chain は fail-closed とし state を変更しない。
6. status / list / push / pop の work ID、title、stack marker を固定する。
7. Phase 3 AC の ignore / pending を解除し、targeted test を通す。

### 5.5 Phase 4: TurnHook injection と完了作業

1. WorkStore に active work prompt projection を追加する。
2. `ContextualMemoryPack::prepare_turn_messages` で Work block を解決し、generic memory block と合計 `MEMORY_PROMPT_BUDGET_BYTES` 内に clamp する。
3. no active / disabled / corrupt Work state では Work block を注入しない。通常 turn 自体は継続する。
4. goal / focus / recent decisions 最大 3 件だけが入り、deferred / idea / note / done / paused が入らないことを固定する。
5. client-side `system_instruction` に Work block が追加されていないことを回帰テストする。
6. 既存 `goal / now / idea / mem / context` の CLI と generic memory injection が壊れていないことを確認する。
7. spec / architecture / security / testing / manual / index を §4.4 の内容で同期する。
8. manual に start、二重 start、defer without active、switch、nested push/pop、finish、全 error、通常 turn 注入の確認手順を書く。
9. Phase 4 AC の ignore / pending を解除し、`./scripts/verify.sh` を実行する。
10. 全 AC が `pending = false` であることを確認した後だけ、本 task を `docs/done/` へ移し index を実装済みにする。

## 6. エラー・安全性契約

- user text は protocol/domain の共通 byte limit で、空文字、NUL、上限超過を拒否する。
- Work ID は正の整数だけを受理し、path component として使用しない。
- `memory_space_id` は既存 resolver / validator だけで解決し、client 文字列を直接 filesystem path に連結しない。
- explicit Work RPC の破損・permission・I/O error は詳細 path や state 本文を返さず、分類済み error にする。
- state / prompt / debug log に API key、設定内容、shell log を自動収集しない。
- Work 内容はユーザーが明示保存する contextual memory であり、自動 redaction はしない。この点を manual と security docs に明記する。
- Work block は user-controlled untrusted context として既存 memory block と同等に扱い、system instruction として注入しない。

## 7. 受け入れ条件と登録案

以下を `spec = "0052"` で test function と 1:1 登録する。実装開始前は全件 `pending = true` かつ `#[ignore]` とする。

| Phase | id | 条件 | test | file_glob |
|------|----|------|------|-----------|
| 0 | `work_protocol_roundtrip` | 全 Work request / response DTO が roundtrip し unknown field を拒否する | `work_protocol_dto_roundtrip_and_rejects_unknown_fields` | `aibe-protocol/src/work.rs` |
| 0 | `work_store_atomic_state` | state の保存・再読込、単調 ID、permission、破損非上書きが成立する | `work_store_persists_atomic_state_and_preserves_corrupt_file` | `aibe/src/adapters/outbound/work_store.rs` |
| 0 | `work_store_concurrency` | 複数 store instance の同時 mutation が lost update を起こさない | `work_store_serializes_concurrent_mutations_without_lost_updates` | `aibe/src/adapters/outbound/work_store.rs` |
| 0 | `work_pack_disabled` | BasicPack が Work RPC を拒否し注入しない | `basic_pack_rejects_work_rpc_and_does_not_inject_work` | `aibe/src/application/basic_memory_pack.rs` |
| 0 | `work_cli_parse` | 引数なしを含む全 `ai work` command が parse できる | `work_subcommands_parse_successfully` | `ai/src/clap_cli.rs` |
| 0 | `work_empty_views` | dashboard / status / list の空状態が区別される | `work_dashboard_status_and_list_render_empty_state` | `ai/tests/work_cli.rs` |
| 0 | `work_cli_disabled` | runtime disabled で Work CLI が fail-closed になる | `work_cli_rejects_when_memory_is_disabled` | `ai/tests/memory_disabled_cli.rs` |
| 0 | `work_cli_feature_off` | feature-off stub が Work CLI を fail-closed にする | `work_cli_stub_rejects_when_memory_feature_is_disabled` | `ai/src/application/memory_stub.rs` |
| 1 | `work_start_active` | start で active work が作られる | `work_start_creates_active_work` | `aibe/tests/work_rpc.rs` |
| 1 | `work_start_pauses_previous` | active 中の start が旧 work を Paused にする | `work_start_pauses_previous_active_work` | `aibe/tests/work_rpc.rs` |
| 1 | `work_focus` | focus が active work だけを更新する | `work_focus_updates_active_work` | `aibe/tests/work_rpc.rs` |
| 1 | `work_idea` | idea が active work に記録される | `work_idea_adds_entry_to_active_work` | `aibe/tests/work_rpc.rs` |
| 1 | `work_note` | note が active work に記録される | `work_note_adds_entry_to_active_work` | `aibe/tests/work_rpc.rs` |
| 1 | `work_decision` | decision が active work に記録される | `work_decision_adds_entry_to_active_work` | `aibe/tests/work_rpc.rs` |
| 1 | `work_defer_without_active` | active なしでも defer が Deferred work を作る | `work_defer_succeeds_without_active_work` | `aibe/tests/work_rpc.rs` |
| 1 | `work_defer_keeps_active` | defer が active と stack を変更しない | `work_defer_keeps_active_work_and_stack_unchanged` | `aibe/tests/work_rpc.rs` |
| 1 | `work_status_sections` | populated status に active/focus/stack/decisions/ideas/deferred が出る | `work_status_renders_all_required_sections` | `ai/tests/work_cli.rs` |
| 1 | `work_list_groups` | list が Active/Paused/Deferred/Done に分類される | `work_list_groups_works_by_status` | `ai/tests/work_cli.rs` |
| 2 | `work_switch` | switch が旧 active を Paused、対象を Active にする | `work_switch_changes_active_work_atomically` | `aibe/tests/work_rpc.rs` |
| 2 | `work_finish` | finish が active を Done にして unset する | `work_finish_marks_active_done_and_unsets_active` | `aibe/tests/work_rpc.rs` |
| 2 | `work_requires_active` | active 必須操作が state 非変更で失敗する | `work_mutations_requiring_active_fail_without_state_change` | `aibe/tests/work_rpc.rs` |
| 2 | `work_switch_missing` | missing work への switch が失敗する | `work_switch_rejects_missing_work` | `aibe/tests/work_rpc.rs` |
| 2 | `work_switch_done` | Done work への switch が失敗する | `work_switch_rejects_done_work` | `aibe/tests/work_rpc.rs` |
| 2 | `work_stack_guard` | stack 非空の start/switch/finish が失敗する | `work_root_transitions_reject_non_empty_stack` | `aibe/tests/work_rpc.rs` |
| 3 | `work_push` | push が旧 active を stack へ積み child を Active にする | `work_push_stacks_parent_and_activates_child` | `aibe/tests/work_rpc.rs` |
| 3 | `work_nested_push` | nested push の stack と parent chain が一致する | `work_nested_push_preserves_parent_chain` | `aibe/tests/work_rpc.rs` |
| 3 | `work_pop` | pop が child を Done にして直前の親へ戻る | `work_pop_finishes_child_and_restores_parent` | `aibe/tests/work_rpc.rs` |
| 3 | `work_pop_empty` | empty stack の pop が state 非変更で失敗する | `work_pop_rejects_empty_stack_without_state_change` | `aibe/tests/work_rpc.rs` |
| 3 | `work_no_auto_merge` | child entries が親へ自動 merge されない | `work_pop_does_not_merge_child_entries_into_parent` | `aibe/tests/work_rpc.rs` |
| 3 | `work_stack_display` | status/list が stack と child marker を表示する | `work_views_render_stack_and_child_marker` | `ai/tests/work_cli.rs` |
| 4 | `work_injection_fields` | active goal/focus/recent decisions だけが注入される | `work_turn_hook_injects_active_goal_focus_and_recent_decisions` | `aibe/tests/memory_pack_turn_hook.rs` |
| 4 | `work_injection_budget` | Work + generic memory が既存 budget を超えない | `work_and_memory_injection_share_existing_budget` | `aibe/tests/memory_pack_turn_hook.rs` |
| 4 | `work_injection_exclusions` | deferred/idea/note/done/paused は通常注入されない | `work_turn_hook_excludes_non_active_and_non_required_fields` | `aibe/tests/memory_pack_turn_hook.rs` |
| 4 | `work_injection_best_effort` | no-active/破損 state でも通常 turn が継続する | `work_turn_hook_is_best_effort_for_missing_or_corrupt_state` | `aibe/tests/memory_pack_turn_hook.rs` |
| 4 | `work_no_client_injection` | ai が Work block を system_instruction へ重複注入しない | `work_context_is_not_added_to_client_system_instruction` | `ai/src/main.rs` |
| 4 | `work_low_level_regression` | 既存 goal/now/idea/mem/context CLI が維持される | `work_feature_keeps_low_level_memory_cli_behavior` | `ai/tests/phase_a_cli.rs` |

## 8. 検証コマンド

Phase ごとの変更範囲に応じて、複数クレートを同時実行せず次を使う。

```bash
./scripts/check-spec-acceptance.py
cargo test -p aibe-protocol -j 1 work
cargo test -p aibe -j 1 work
cargo test -p ai -j 1 work
cargo check -p ai --no-default-features
```

完了直前は必ず次を実行する。

```bash
./scripts/verify.sh
```

## 9. 実装しないもの

1. 完全なタスク管理、カンバン、期限、担当者、優先度
2. P2P aibe 共有
3. ブラウザ拡張 / エディタ拡張
4. 汎用 client-side tool protocol
5. LLM による Work / memory の自動更新
6. DAG 型 work graph
7. top-level `start / status / finish`
8. `resume / abandon / rename / show / remove / reopen`
9. child work の LLM 要約と親への自動 merge

## 10. 完了条件

1. 全 command と §1.3 の状態遷移が本番 Unix socket 経路で動く。
2. Work state が `aibe` 所有で原子的に永続化され、複数 CLI process の更新で invariant を壊さない。
3. active work は最大 1 件で、stack / parent chain / Deferred / Done が再起動後も復元される。
4. 通常 turn への Work 注入が `aibe` TurnHook の 1 経路だけで行われ、既存 budget を超えない。
5. runtime disabled、feature-off、破損 state が fail-closed / best-effort の契約どおり動く。
6. 既存 Contextual Memory CLI と injection の回帰がない。
7. §7 の全 AC が `pending = false` かつ非 ignore で成功する。
8. spec / architecture / security / testing / manual / index が実装と同期する。
9. 手動検証結果または未実施事項が報告される。
10. `./scripts/verify.sh` が成功する。
