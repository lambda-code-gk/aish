# 0066 Human Task Recovery Hardening 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定（実装済み）  
> **関連**: [`0063_human-task-suspend-resume-overview.md`](0063_human-task-suspend-resume-overview.md)、[`0065_human-task-agent-continuation-spec.md`](0065_human-task-agent-continuation-spec.md)、[`docs/feature-development-policy.md`](../feature-development-policy.md)  
> **論理区分**: Issue #6 の 0063-E

## 0. Core outcome

クラッシュや中断後に残った Human Task checkpoint を、ユーザー明示の local CLI で安全に復旧または掃除し、既存 resume / continuation 経路へ戻せる。

## 1. Minimum vertical slice

```text
orphaned Running checkpoint
→ ai human-task status が recover を案内
→ ai human-task recover
→ root flock 取得
→ ユーザー確認
→ Running → Suspended（unexpected_process_termination）を atomic 保存
→ ai human-task resume で既存経路へ復帰
```

同じ手動回復機構で stale `Continuing` は continuation turn ID を保持したまま `ResultPending` へ戻す。`Finished` と通常の破棄は既存 `cancel` を案内する。破損、未対応 version、mode 不正、空 task directory は自動削除せず、診断後の `recover --force-invalid` と明示確認がある場合だけ checkpoint root 内の単一残骸を削除する。

## 2. Fault model

### 2.1 保証対象

単一ホスト・単一ユーザーで、回復操作時に owner が保持していない checkpoint root lock を取得できる場合の手動回復を保証する。PID の死活は推定せず、lock を取得できた `Running` / `Continuing` のみを stale 候補として扱う。

### 2.2 保証対象外

- process 終了検出と自動状態遷移
- PID / lease / heartbeat / background reconciler
- aibe 再起動を跨ぐ exactly-once と durable turn admission
- schema migration、複数 host、複数 active Human Task
- root 自体の ownership 不正や、所有外 directory を再帰削除する cleanup

## 3. Non-goals

- 自動 crash recovery / 自動 continuation retry
- 新しい状態機械、永続 aggregate、agent loop
- session pruning の製品ロジック変更
- bash / zsh の新規自動受け入れ試験（手動確認に限定）

## 4. Complexity inventory

| 項目 | 数・内容 |
|------|----------|
| 実行主体 | 1（既存 local `ai` CLI） |
| 状態機械 | 1（既存 HumanTaskWorkflowState の遷移追加） |
| 永続 aggregate | 1（既存 checkpoint v1） |
| 外部副作用 | 1（既存 secure checkpoint filesystem） |
| プロセス境界 | 0 |
| 新規基盤機構 | 1（manual-stale-checkpoint-recovery） |
| 他機能統合 | 3（status、file store、既存 resume / continuation lifecycle） |

## 5. Complexity Gate

- 判定: **Yellow（Scope Gate 承認済み）**
- 理由: actor / state machine / aggregate / effect は Green 内だが、既存機能統合が 3 で Yellow 閾値に達する。resume / continuation は回復後に既存 lifecycle を選ぶ一つの統合点であり、novel mechanism は手動 stale checkpoint recovery 一つに限定する
- 分割判断: crash recovery、lease、reconciler、migration、exactly-once、secondary agent loop をすべて除外して `false` に固定する
- 承認例外: 不要

## 6. Complexity budget

新規実行主体 +0、状態機械 +0、永続 aggregate +0、external effect / process boundary +0、novel mechanism +0、integration +0。crash / lease / reconciler / migration / exactly-once は +0。

## 7. Split triggers

PID 死活判定、自動遷移、lease / heartbeat、reconciler、journal、schema migration、再起動跨ぎ duplicate 防止、二つ目の aggregate / state machine / agent loop のいずれかが必要になった時点で STOP-THE-LINE とする。

## 8. パック構成の適用

**No** — 0045 §6 の候補条件を満たさない。回復は既存 Human Task lifecycle と secure checkpoint store の安全操作であり、optional 配備、重い依存、専用 RPC、turn hook 群を脱着する機能ではない。Active/Basic Pack、runtime toggle、Cargo feature は追加しない。

## 9. Acceptance Criteria

| ID | 条件 |
|----|------|
| `human_task_recovery_vertical_e2e` | lock 取得済み orphaned Running を確認後 Suspended に保存し、既存 resume 経路で再開できる |
| `human_task_recovery_continuing_to_result_pending` | stale Continuing を確認後、continuation turn ID を保持して ResultPending に戻せる |
| `human_task_recovery_status_guidance` | status が Running / Continuing / Finished / invalid residue ごとに recover・resume retry・cancel・force cleanup の次アクションを明示する |
| `human_task_recovery_force_invalid_cleanup` | corrupt / unsupported / mode 不正 / 空 task directory を自動削除せず、明示確認付き `--force-invalid` だけで単一残骸を掃除する |
| `human_task_recovery_is_confirmed_and_locked` | recover は確認拒否時に無変更で、root lock が busy の間は状態変更・cleanup を行わない |
| `human_task_recovery_preserves_existing_paths` | Suspended / ResultPending / Finished は recover で上書きせず既存 resume / cancel を案内し、通常 cancel を維持する |

## 10. Deferred specs

- 自動 crash recovery と ownership lease（必要性が実証された場合の別 spec）
- session pruning 競合の製品変更
- bash / zsh 自動 E2E

## 11. Scope change log

| Revision | 分類 | 変更 | 理由 |
|----------|------|------|------|
| 1 | INITIAL | 0063-E を正式 spec 0066 として Scope Lock。手動 recovery と明示 force cleanup に限定 | Red 要因を導入せず、既存 resume / continuation へ戻る Vertical Slice を成立させるため |
