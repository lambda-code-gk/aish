# 0066 Human Task Recovery Hardening 実装指示書

## 0. 目的

[`0066_human-task-recovery-hardening-spec.md`](../spec/0066_human-task-recovery-hardening-spec.md) に従い、local `ai human-task recover` で stale checkpoint を既存 resume / continuation 経路へ戻し、invalid residue を明示操作で安全に掃除する。

## 0.1 Scope Lock

- Feature scope registry: `scripts/feature-scope.toml`
- Scope revision: 1
- Complexity class: Yellow（approved）
- Vertical slice AC ID: `human_task_recovery_vertical_e2e`
- Locked AC IDs: 設計書 §9 の6件

## 1. Phase 分割

| Phase | 内容 | ゲート（`scripts/spec-acceptance.toml`） |
|-------|------|------------------------------------------|
| 1 | recover CLI、Running / Continuing 遷移、状態別 status、invalid force cleanup の Vertical Slice | 6 AC を `pending = false` にする |
| 2 | bash / zsh 自動 E2E、session pruning 競合の追加 hardening | 本 spec では Deferred |

**Vertical Slice Gate**: Phase 1 成功前に PID / lease / reconciler / schema migration / 自動 recovery / 汎用 framework 化を実装しない。

## 2. 受け入れ条件

| ID | 条件 | テスト関数 | pending |
|----|------|------------|---------|
| `human_task_recovery_vertical_e2e` | Running→Suspended→既存 resume | 同名 | false（実装完了時） |
| `human_task_recovery_continuing_to_result_pending` | Continuing→ResultPending、ID保持 | 同名 | false（実装完了時） |
| `human_task_recovery_status_guidance` | 状態別の次アクション | 同名 | false（実装完了時） |
| `human_task_recovery_force_invalid_cleanup` | invalid residue の明示 cleanup | 同名 | false（実装完了時） |
| `human_task_recovery_is_confirmed_and_locked` | confirmation と flock | 同名 | false（実装完了時） |
| `human_task_recovery_preserves_existing_paths` | 通常経路の非上書き | 同名 | false（実装完了時） |

## 3. 完了条件

1. Phase 1 の全 AC が `pending = false`
2. `./scripts/verify.sh` 成功
3. architecture / testing / security / manual 同期
4. Phase 2 は Deferred のため、本指示書は Phase 1 完了後も `docs/tasks/` に保持する

## 4. 仕様との差分

なし。
