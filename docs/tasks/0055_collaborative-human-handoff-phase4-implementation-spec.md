# 0055 — Collaborative Human Handoff Phase 4 実装指示書

> **種別**: 実装指示書（`docs/tasks/`）  
> **設計正本**: [0055_collaborative-human-handoff-spec.md](../spec/0055_collaborative-human-handoff-spec.md)  
> **マスター**: [0055_collaborative-human-handoff-implementation-spec.md](0055_collaborative-human-handoff-implementation-spec.md)  
> **状態**: Phase 4 完了
> **前提**: Phase 3 完了

## 0. 目的

クラッシュ・端末切断・再起動後の復旧、`ORPHANED` / `RETURNED` / `CANCELLED`、heartbeat、token rotation、side 実行中の human shell 終了、fault injection を実装する。

## 1. スコープ

### 1.1 対象

| 項目 | 設計参照 |
|------|----------|
| heartbeat による lease 更新（supervisor、prompt 非依存） | spec §15.3, §24.2 |
| lease 失効のみでは自動再実行しない | spec §24.3 |
| owner 消失・異常終了 → `ORPHANED` | spec §14.5, §20.1 |
| `ai resume` / 複数 handoff 一覧（§5.6） | spec §5.6, §20.2 |
| token rotation + shell generation++ | spec §20.2 |
| 二重 resume 拒否 | spec §19, §24.1 |
| `RETURNED` から親のみ再開 | spec §14.7, §20.5 |
| 親 run 意味的再開（§21） | spec §21 |
| `RUNNING` → `UNKNOWN` | spec §15.5, §20.4 |
| `RESUMING_PARENT` 失敗 → `RETURNED` | spec §14.7 |
| checkpoint 後 shell 前失敗 → `CANCELLED` | spec §16, §9 |
| ORPHANED 中 child goal を閉じない | spec §10.3, §22 |
| side RUNNING 中 Ctrl+D → 中断後 RETURNED | spec §14.4 |
| `ReconcileStaleHandoffs` | spec §25.2 |
| 復旧時の非復元警告（§28） | spec §28 |
| fault injection（§28.3） | spec §28.3 |

### 1.2 非対象

| 項目 | Phase |
|------|-------|
| プロンプト hook | 5 |

## 2. 受け入れ条件（`spec = "0055"`）

| id | 条件 | テスト関数 | pending |
|----|------|------------|---------|
| `orphaned_on_abnormal_exit` | 異常終了で ORPHANED、親は自動再開しない | `abnormal_shell_exit_marks_handoff_orphaned` | false |
| `orphaned_on_parent_crash` | 親プロセス消失で ORPHANED | `parent_process_loss_marks_handoff_orphaned` | false |
| `resume_orphaned_shell` | `ai resume` で新 human shell | `resume_orphaned_spawns_new_shell_with_rotated_token` | false |
| `resume_rejects_old_token` | 旧 token / generation を拒否 | `resume_rotates_token_and_rejects_old_generation` | false |
| `resume_exclusive` | 二重 resume 拒否 | `second_resume_rejected_while_lease_active` | false |
| `resume_lists_multiple_handoffs` | 複数時に一覧と ID 要求 | `resume_lists_multiple_recoverable_handoffs` | false |
| `resume_returned_parent_only` | RETURNED は shell なしで親再開 | `resume_returned_restarts_parent_without_shell` | false |
| `parent_semantic_resume` | 新 parent run に保留 ShellExec 文脈 | `resumed_parent_run_carries_pending_shell_exec_context` | false |
| `unknown_tools_not_reexecuted` | UNKNOWN を自動再実行しない | `recovery_does_not_auto_rerun_unknown_tools` | false |
| `side_crash_unknown_tools` | side クラッシュで UNKNOWN | `side_agent_crash_marks_running_tools_unknown` | false |
| `waiting_for_human_survives_reboot` | WAITING 状態を復旧後も維持可 | `resume_preserves_pending_human_request_state` | false |
| `checkpoint_before_shell_crash` | checkpoint 後 shell 前失敗で CANCELLED | `handoff_cancelled_when_shell_never_started_after_checkpoint` | false |
| `parent_resume_failed_returns` | RESUMING_PARENT 失敗で RETURNED | `parent_resume_failure_returns_to_returned_state` | false |
| `child_goal_open_while_orphaned` | ORPHANED では child goal を閉じない | `orphaned_handoff_keeps_child_goal_open` | false |
| `side_running_ctrl_d_interrupts` | side 実行中 Ctrl+D で RETURNED | `ctrl_d_during_side_run_returns_to_parent` | false |
| `lease_lost_no_auto_rerun` | lease 失効だけでは自動再実行しない | `lease_expiry_alone_does_not_auto_resume_parent` | false |

## 3. 変更ファイル（目安）

| 区分 | パス |
|------|------|
| ai app | `ResumeOrphanedHandoff`, `ResumeReturnedParent`, `ReconcileStaleHandoffs`, `CancelHandoff` |
| ai CLI | `ai resume` |
| aish | supervisor + heartbeat |
| tests | `ai/tests/0055_collaborative_recovery.rs` |

## 4. 実装手順

### 4.1 `ai resume`

- 1 件: 自動復旧
- 複数: handoff ID 短縮、親タスク、child goal、状態、cwd、更新時刻、lease 状態を一覧
- ORPHANED → 新 shell + token rotation
- RETURNED → 親 run のみ

### 4.2 fault injection

checkpoint 後 panic、lease 破損、shell log 書き込み失敗、要約失敗（履歴は保持）をテスト。

## 5. 検証

```bash
cargo test -p ai --test 0055_collaborative_recovery -j 1
./scripts/verify-targeted.sh --package ai
./scripts/verify-targeted.sh --package aish
```
