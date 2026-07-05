# 0055 — Collaborative Human Handoff Phase 6 実装指示書（縦切り統合）

> **種別**: 実装指示書（`docs/done/`）  
> **設計正本**: [0055_collaborative-human-handoff-spec.md](../spec/0055_collaborative-human-handoff-spec.md)  
> **状態**: 実装済み（Phase 6）  
> **前提**: Phase 1–5 完了

## 0. 目的

port モックだけでは検出しにくい **コンポーネント境界の接続バグ** を、巨大 full E2E 1 本の代わりに **縦切り統合テスト** で押さえる。

各縦切りは **実 `FilesystemHandoffStore` + 必要最小限の実 process / 実 shell** を接続する。full PTY 通しは nightly / 手動。

## 1. 実行 tier

| tier | 対象 | 実行 |
|------|------|------|
| CI | lease / work / candidate / side-run / parent resume 縦切り | `./scripts/verify.sh` |
| nightly / 手動 | structured human action、full PTY 通し | `./scripts/run-collaborative-nightly.sh` |

`#[ignore]` は **`pending = true` の AC のみ**。無期限 ignore で完了扱いしない。

## 1.1 Durable workflow 再設計（本ブランチ）

設計書 §33 参照。`handoff.json` と `checkpoint.json` を個別更新する旧経路を廃止し、`CollaborativeWorkflow` aggregate を `workflow.json` へ原子的に保存する。状態変更は domain reducer、外部副作用は durable `pending_effects`、クラッシュ復旧は `CollaborativeWorkflowReconciler` の単一入口へ集約する。

実装順序:

1. aggregate / reducer / invariant の RED test
2. workflow store（revision CAS + atomic replace）
3. pending effect の claim / complete / fail
4. unified reconciler
5. application の `save_handoff` / `save_checkpoint` 直接呼び出しを workflow mutation へ移行
6. static check と縦切り E2E

既存 `handoff.json` / `checkpoint.json` は読み取り移行のみ許可する。新規の本番書き込みは `workflow.json` を正本とし、token plaintext を aggregate、effect、監査ログへ保存しない。

## 2. 受け入れ条件（`spec = "0055"` phase = 6）

| id | 条件 | テスト関数 | pending |
|----|------|------------|---------|
| `slice_lease_transfer_real_shell` | 実 aish human-shell で lease 所有者が parent→shell→release→parent resume→completed | `slice_lease_transfer_real_shell` | false |
| `slice_work_push_pop_real_store_shell` | 実 store + mock work client + 実 shell で Push/Pop | `slice_work_push_pop_real_store_shell` | false |
| `slice_candidate_cache_store_to_real_shell` | handoff store → suggestion cache → 実 aish recall | `slice_candidate_cache_store_to_real_shell` | false |
| `slice_side_run_exclusive_real_store` | 実 store 上で side-run lock 排他（並行 dispatch） | `slice_side_run_exclusive_real_store` | false |
| `slice_parent_resume_real_store_work` | Returned checkpoint → resume + child Work 完了 | `slice_parent_resume_real_store_work` | false |
| `slice_structured_human_action_real_shell_ai` | WAITING → human shell 内 `ai` → side 状態を安全に観測 | `slice_structured_human_action_real_shell_ai` | false |
| `slice_full_collaborative_pty_nightly` | 上記縦切りの最小連結（full PTY 通し） | `slice_full_collaborative_pty_nightly` | false |
| `workflow_atomic_aggregate_round_trip` | handoff/checkpoint/effect を単一 revision として原子的に保存 | `workflow_atomic_aggregate_round_trip` | false |
| `workflow_reducer_rejects_invariant_violation` | reducer が ID/state 不整合を拒否し永続状態を変更しない | `workflow_reducer_rejects_invariant_violation` | false |
| `workflow_reconciler_retries_pending_effect_once` | crash 後の pending effect を統一 reconciler が冪等に収束 | `workflow_reconciler_retries_pending_effect_once` | false |
| `workflow_never_persists_plaintext_token` | workflow/effect に token plaintext を永続化しない | `workflow_never_persists_plaintext_token` | false |

## 3. 変更ファイル

| 区分 | パス |
|------|------|
| tests | `ai/tests/0055_collaborative_vertical_slice.rs` |
| workflow tests | `ai/tests/0055_collaborative_workflow.rs` |
| domain | `ai/src/domain/collaborative_workflow.rs` |
| store | `ai/src/adapters/outbound/collaborative_workflow_store.rs` |
| reconciler | `ai/src/application/collaborative_workflow_reconciler.rs` |
| nightly | `scripts/run-collaborative-nightly.sh` |
| static check | `scripts/check-collaborative-workflow.sh` |
| registry | `scripts/spec-acceptance.toml` |
| docs | `docs/testing.md` |

## 4. 検証

```bash
./scripts/verify.sh
./scripts/run-collaborative-nightly.sh   # pending=true 解除後
```

## 5. 完了後

Phase 6 全 AC が `pending = false` かつ `#[ignore]` 解除後、本書を `docs/done/` へ移動。
