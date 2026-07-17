# 0065 Human Task Agent Continuation 実装指示書

> **状態**: 全 AC 完了・全体検証待ち（既存 0054 socket test の sandbox EPERM）

## 0. 目的

[`0065_human-task-agent-continuation-spec.md`](../spec/0065_human-task-agent-continuation-spec.md) に従い、ResultPending の保存済み親コンテキストと final_result から既存 Collaborative Mode turn を開始し、同一 process 内の二重開始を拒否する。

## 0.1 Scope Lock

- Feature scope registry: `scripts/feature-scope.toml`
- Scope revision: 1
- Complexity class: Yellow（review approved。integration 数 3 のため）
- Vertical slice AC ID: `human_task_continuation_vertical_e2e`
- Locked AC IDs: spec §9 の 12 AC
- Novel mechanism: `agent-continuation` 一つ
- `secondary_agent_loop = false` / `exactly_once = false` / `crash_recovery = false` / `lease_or_heartbeat = false` / `schema_migration = false`

## 1. Phase 分割

| Phase | 内容 | ゲート（`scripts/spec-acceptance.toml`） |
|-------|------|------------------------------------------|
| 1 | vertical E2E: continuation message、checkpoint state/ID、既存 execute_turn 接続、成功時 delete | `human_task_continuation_vertical_e2e` が `pending = false` になるまで Phase 2 に進まない |
| 2 | ResultPending resume retry、fail-closed、root lock、aibe 同一 process duplicate 拒否、status/CLI | 全 AC を `pending = false` |
| 3 | 0063/0064 回帰、architecture/testing/security/manual 同期、全体検証 | `human_task_continuation_preserves_resume_regressions` と `./scripts/verify.sh` が成功 |

**Vertical Slice Gate**: Phase 1 成功前に crash recovery、schema migration、lease、汎用 dedup framework、性能最適化を実装しない。

**STOP-THE-LINE**: 新しい実行主体、二つ目の状態機械・永続正本・agent loop、journal、lease、reconciler、crash recovery が必要になった場合は実装を停止し、scope revision と Complexity Gate を再判定する。

## 2. レイヤー別タスク

### Domain

- `HumanTaskCheckpointV1::validate` を ResultPending / Continuing / Finished の仕様へ拡張する
- 継続メッセージを純粋関数で構築し、unverified 契約を固定する

### Application / ports

- root lock 内で ResultPending→Continuing→Finished/delete と失敗復元を行う continuation service を追加する
- continuation turn ID を初回に生成・保存し、再試行で保持する
- `HumanTaskResume` は Suspended の Human Shell 経路と ResultPending の continuation-only 経路を判別できる outcome を返す

### Adapters / composition root

- `ai/src/main.rs` で既存 `execute_turn` に保存済み context と明示 turn ID を渡す
- aibe の既存 in-memory turn admission を同一 ID 再受理拒否へ拡張する
- status / CLI 文言を continuation 利用可能な案内へ更新する

### Tests / docs

- Phase 1 前に全 AC の `#[ignore]` テストと `pending = true` registry を置く
- Phase ごとに対象テストの ignore / pending を外す
- `architecture.md` / `testing.md` / `security.md` / `manual/0065_*` / 0064 manual を同期する

## 3. 受け入れ条件

| ID | テスト関数 | Phase | 初期 pending |
|----|------------|-------|--------------|
| `human_task_continuation_vertical_e2e` | `human_task_continuation_vertical_e2e` | 1 | false |
| `human_task_continuation_message_is_unverified` | `human_task_continuation_message_is_unverified` | 1 | false |
| `human_task_continuation_preserves_parent_context` | `human_task_continuation_preserves_parent_context` | 1 | false |
| `human_task_continuation_turn_id_is_stable` | `human_task_continuation_turn_id_is_stable` | 1 | false |
| `human_task_continuation_state_invariants` | `human_task_continuation_state_invariants` | 1 | false |
| `human_task_result_pending_resume_retries_without_shell` | `human_task_result_pending_resume_retries_without_shell` | 2 | false |
| `human_task_continuation_failure_keeps_result_pending` | `human_task_continuation_failure_keeps_result_pending` | 2 | false |
| `human_task_continuation_holds_root_lock` | `human_task_continuation_holds_root_lock` | 2 | false |
| `aibe_rejects_duplicate_continuation_turn_id` | `aibe_rejects_duplicate_continuation_turn_id` | 2 | false |
| `human_task_continuation_finished_delete_is_fail_closed` | `human_task_continuation_finished_delete_is_fail_closed` | 2 | false |
| `human_task_continuation_status_and_cli_guidance` | `human_task_continuation_status_and_cli_guidance` | 2 | false |
| `human_task_continuation_preserves_resume_regressions` | `human_task_continuation_preserves_resume_regressions` | 3 | false |

## 4. 完了条件

1. 全 AC が `pending = false`
2. `./scripts/verify-targeted.sh --package ai`（aibe 変更後は `--package aibe` も）成功
3. `./scripts/verify.sh` 成功
4. docs 同期後、本ファイルを `docs/done/` へ移し index を実装済みに更新

## 5. 仕様との差分

- なし
