# 0064 Collaborative Mode Human Task Resume 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **関連**: [`0063_human-task-suspend-resume-overview.md`](0063_human-task-suspend-resume-overview.md)、[`0063_human-task-suspend-checkpoint-spec.md`](0063_human-task-suspend-checkpoint-spec.md)、[`docs/feature-development-policy.md`](../feature-development-policy.md)  
> **論理区分**: Issue #6 の 0063-C

## 0. Core outcome

ユーザーが Suspended の Human Task を `ai human-task resume [TASK_ID]` で再開し、最後の作業 cwd と既存 briefing から新しい Human Shell を始め、再度 `human-task suspend` して進捗（segment / Observation）を順序どおり checkpoint に残せる。

## 1. Minimum vertical slice

```text
既存 Suspended checkpoint（0063 経路）
→ ai human-task resume
→ root lock / Suspended 検証 / cwd 存在確認
→ Running 保存（prior segments 保持）
→ 新しい runtime dir と HumanShellLauncher + HumanTaskBriefing
→ ユーザーが human-task suspend [reason]
→ segment index N を追記し Suspended を atomic 保存
→ ai human-task status が task ID・cwd・最新 reason・Resume/Cancel 案内を表示
```

本 spec は resume・複数回中断（segment 追記）に加え、Done 後の **`ResultPending` 永続化**（agent continuation なし）を含む。親 Collaborative Mode turn の開始は後送する。

### 1.1 Checkpoint invariant 拡張

0063 の version 1 envelope を維持し、同一 aggregate の状態不変条件だけを拡張する。

- `Running`: `suspended_at_ms` / `suspend_reason` / `final_result` / continuation turn ID がなく、segment は空（初回 create）またはすべて `end_reason=Suspended` の連番 index（resume 中）
- `Suspended`: `suspended_at_ms` があり、`final_result` がなく、1 件以上の完全な `Suspended` segment があり、index は `0..n-1` の連番、`current_cwd == segments.last().final_cwd`
- `ResultPending`: `final_result` が Done の `HumanTaskResult`、prior segments はすべて Suspended、最後の segment は `end_reason=Done`、suspend fields なし、continuation turn ID なし
- `Continuing` / `Finished`: 引き続き予約。0064 は生成・遷移させない

遷移:

```text
Suspended → Running → Suspended              # resume 後の再中断
Suspended → Running → ResultPending          # resume 後の Done（continuation は後送）
Running → terminal Done（checkpoint 削除）   # 初回 create の既存 0063 経路のみ
```

### 1.2 Resume application

`ai` の local CLI として `HumanTaskResume` application を置く。aibe socket へ接続しない。

1. root の排他 file lock を取得する
2. `load_active` で Suspended checkpoint を読む（なければ `human_task_not_found`）
3. 任意の TASK_ID 指定時は一致確認（不一致は `human_task_not_found`、既存 file 非変更）
4. state が Suspended でない場合は拒否（Running / ResultPending は `human_task_not_suspended`）
5. `current_cwd` の存在を確認し、なければ shell を起動せず Suspended を維持（`human_task_resume_cwd_unavailable`）
6. state を Running へ更新し、suspend fields をクリアして save（prior segments は保持）
7. 新しい runtime handoff directory を割り当て、`ParentTermiosGuard` 付きで既存 `HumanShellLauncher` を起動する
8. shell 終端後、0061 の bounded Observation を収集する
9. `Suspended` なら新 segment を追記し Suspended を atomic 保存。最終 save 失敗時は resume 直前の Suspended へ復元する
10. `Done` なら Done segment と `final_result` を持つ `ResultPending` を atomic 保存（Suspended へ巻き戻さない）。最終 save 失敗時も Suspended へ戻さず、既存の Running（orphaned / resume 不可・cancel のみ）を維持する
11. launch 失敗時: Human Shell 未開始（MissingCwd / PreLaunchFailed）だけ Suspended へ復元する。起動後または不明な Cancelled / Interrupted / Failed では Running を維持する
12. lock は終端処理まで保持する

### 1.3 CLI と案内

```text
ai human-task resume
ai human-task resume ht-20260714-7f31c2
```

`ai human-task status` と aibe の SuspendTurn 固定応答は、動作する Resume 案内と Cancel 案内の両方を表示する。ResultPending の status は Resume を案内せず Cancel のみを案内する。

Done after resume の標準出力例:

```text
Human Task completed and saved.

Task:
  <task-id>
State: result pending

Agent continuation is not available yet.
Cancel to discard:
  ai human-task cancel --yes
```

### 1.4 Evidence

全 segment の Observation を順序どおり checkpoint に保持する。Done 時は当該 session の Observation を `final_result` に保存する。親 Collaborative Mode turn への返却は 0063-D。

## 2. Fault model

### 2.1 保証対象

標準 Fault Model を基礎とし、単一ホスト・単一ユーザー上で Suspended checkpoint から resume → 再 suspend できる。create/status/cancel/resume は同じ root flock で直列化する。

### 2.2 保証対象外

- Done 後の agent continuation / continuation turn ID
- `ai` / `aish` crash 途中からの自動復旧
- cwd 再作成、`--cwd` 上書き
- crash recovery、lease、schema migration、exactly-once

## 3. Non-goals

- continuation turn ID、新しい Collaborative Mode turn（ResultPending の永続化自体は本 spec）
- Evidence flatten の親 turn への返却
- 0055 旧 `shell_exec` handoff への resume
- 複数 active task、task 一覧 UI、crash recovery、lease

## 4. Complexity inventory

| 項目 | 数・内容 |
|------|----------|
| 実行主体 | 1（既存 ai 同期 Human Shell callback。resume は local CLI） |
| 状態機械 | 1（既存 Human Task checkpoint の Suspended↔Running 拡張） |
| 永続 aggregate | 1（既存 version 1 checkpoint。新 aggregate なし） |
| 外部副作用 | 2（既存 Human Shell、secure checkpoint filesystem） |
| プロセス境界 | 1（既存 ai→aish Human Shell） |
| 新規基盤機構 | 1（human-task-resume） |
| 他機能統合 | 3（0063 checkpoint、Human Shell/briefing、0061 Evidence 保持） |

## 5. Complexity Gate

- 判定: **Yellow（Scope Gate承認済み）**
- 理由: actors / aggregate は Green 内だが、既存 SM 拡張と機能統合が Yellow 閾値に達する
- 分割判断: Done / continuation を 0063-D へ送り novel mechanism を resume 一つに固定する
- 承認例外: 不要

## 6. Complexity budget

| 項目 | 上限 |
|------|------|
| 新規実行主体 | +0 |
| 状態機械 | +0（既存 1 個の遷移拡張のみ） |
| 永続 aggregate | +0 |
| external effect | +0 |
| process boundary | +0 |
| novel mechanism | +0 |
| integrations | +0 |
| agent loop | 新設 +0 |

## 7. Split triggers

- agent continuation / continuation turn ID / Continuing→Finished
- 二つ目の永続正本・実行主体・agent loop
- lease / heartbeat / reconciler / crash recovery / schema migration
- cwd 上書き UI、複数 active task

## 8. パック構成の適用

**No** — 0045 §6 の候補条件を満たさない。resume は明示 Human Task lifecycle の core 契約であり、optional Pack 脱着を目的としない。

## 9. Acceptance Criteria

| ID | 条件 |
|----|------|
| `human_task_resume_vertical_e2e` | Suspended fixture から resume、Running 保存後の shell 起動、再 suspend、segment 追記、status 表示までが一貫する |
| `human_task_resume_restores_cwd_and_briefing` | 保存 cwd と `HumanTaskBriefing::from(task)` で launcher が呼ばれる |
| `human_task_resume_appends_suspended_segment` | 再 suspend で index N の Suspended segment が追記され prior segment が保持される |
| `human_task_resume_holds_root_lock` | resume が root lock を load 前から最終 save まで保持する |
| `human_task_status_shows_resume_command` | status と SuspendTurn 固定応答が `ai human-task resume` を案内する |
| `human_task_resume_supports_multiple_suspends` | Suspended→Running→Suspended を複数回繰り返し、全 segment の Observation が順序保持される |
| `human_task_resume_rejects_missing_or_mismatched_id` | task なし・ID 不一致を安定 code で拒否し file を変更しない |
| `human_task_resume_rejects_non_suspended` | Running / ResultPending / invalid を Suspended として扱わず shell を起動しない |
| `human_task_resume_cwd_unavailable_fails_closed` | cwd 不在時は shell 未起動・Suspended 維持・安定 code |
| `human_task_resume_done_persists_result_pending` | Done 後は Done segment と final_result を ResultPending として永続化し Suspended へ巻き戻さない |
| `human_task_resume_final_save_failure_restores_suspended` | 再 suspend の最終 save 失敗時は Running を残さず resume 前の Suspended へ復元する |
| `human_task_resume_done_save_failure_keeps_running` | Done 後の ResultPending save 失敗時は Suspended へ戻さず Running を維持し resume を拒否する |
| `human_task_resume_post_launch_error_keeps_running` | Human Shell 起動後の Cancelled / Interrupted / Failed では Suspended へ戻さず Running を維持する |
| `human_task_resume_pre_launch_error_restores_suspended` | Human Shell 未開始の pre-launch 失敗だけ Suspended へ復元する |
| `human_task_create_post_launch_error_keeps_running` | 初回 create で起動後 Cancelled / Interrupted / Failed 時は checkpoint を削除せず Running を維持する |
| `human_task_create_pre_launch_error_removes_checkpoint` | 初回 create の pre-launch 失敗だけ Running checkpoint を削除する |
| `human_task_resume_cli_installs_parent_termios_guard` | resume CLI が ParentTermiosGuard で親 TTY を復元する |
| `human_task_resume_preserves_single_segment_regression` | 単一 segment の 0063 Suspended 経路と create Done 削除が回帰しない |

## 10. Deferred specs

- **0063-D（別4桁番号）**: ResultPending から親 context の新 Collaborative Mode turn、continuation turn ID、Evidence flatten の親への返却
- **0063-E（別4桁番号）**: stale ownership、crash hardening

## 11. Scope change log

| Revision | 分類 | 変更 | 理由 |
|----------|------|------|------|
| 1 | INITIAL | 0063-C を 0064 として Scope Lock。Done/continuation は後送 | overview 分割と One Novelty Rule に従い resume だけを実装単位にする |
| 2 | SAFETY_WITHIN_FAULT_MODEL / BLOCKER_ORIGINAL_AC | Done を ResultPending 永続化へ変更。最終 save 失敗時の Suspended 復元と ParentTermiosGuard を追加 | PR #8 レビュー: Suspended 巻き戻しは外部副作用に対して fail-closed でない |
| 3 | SAFETY_WITHIN_FAULT_MODEL | Done/ResultPending の terminal save 失敗では Suspended へ戻さず Running を維持 | PR #8 再レビュー: rename後fsync失敗でも完了済み作業を再resume可能にしてはならない |
| 4 | SAFETY_WITHIN_FAULT_MODEL | 起動後 Cancelled/Interrupted/Failed では Suspended 復元せず Running 維持。PreLaunchFailed を分離 | PR #8 再レビュー: spawn後失敗で副作用済み作業を再resume可能にしてはならない |
| 5 | SAFETY_WITHIN_FAULT_MODEL / REGRESSION | HumanTaskCoordinator 初回 create も起動後エラーで Running を維持 | PR #8 再レビュー: resume と同じ重複実行リスクが初回経路に残っていた |
