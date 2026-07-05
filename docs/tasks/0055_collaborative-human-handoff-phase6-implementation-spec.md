# 0055 — Collaborative Human Handoff Phase 6 実装指示書（縦切り統合）

> **種別**: 実装指示書（`docs/tasks/`）  
> **設計正本**: [0055_collaborative-human-handoff-spec.md](../spec/0055_collaborative-human-handoff-spec.md)  
> **状態**: 進行中（Phase 6）  
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

## 2. 受け入れ条件（`spec = "0055"` phase = 6）

| id | 条件 | テスト関数 | pending |
|----|------|------------|---------|
| `slice_lease_transfer_real_shell` | 実 aish human-shell で lease 所有者が parent→shell→release→parent resume→completed | `slice_lease_transfer_real_shell` | false |
| `slice_work_push_pop_real_store_shell` | 実 store + mock work client + 実 shell で Push/Pop | `slice_work_push_pop_real_store_shell` | false |
| `slice_candidate_cache_store_to_real_shell` | handoff store → suggestion cache → 実 aish recall | `slice_candidate_cache_store_to_real_shell` | false |
| `slice_side_run_exclusive_real_store` | 実 store 上で side-run lock 排他（並行 dispatch） | `slice_side_run_exclusive_real_store` | false |
| `slice_parent_resume_real_store_work` | Returned checkpoint → resume + child Work 完了 | `slice_parent_resume_real_store_work` | false |
| `slice_structured_human_action_real_shell_ai` | WAITING → human shell 内 `ai` → side 再開 | `slice_structured_human_action_real_shell_ai` | true |
| `slice_full_collaborative_pty_nightly` | 上記縦切りの最小連結（full PTY 通し） | `slice_full_collaborative_pty_nightly` | true |

## 3. 変更ファイル

| 区分 | パス |
|------|------|
| tests | `ai/tests/0055_collaborative_vertical_slice.rs` |
| nightly | `scripts/run-collaborative-nightly.sh` |
| registry | `scripts/spec-acceptance.toml` |
| docs | `docs/testing.md` |

## 4. 検証

```bash
./scripts/verify.sh
./scripts/run-collaborative-nightly.sh   # pending=true 解除後
```

## 5. 完了後

Phase 6 全 AC が `pending = false` かつ `#[ignore]` 解除後、本書を `docs/done/` へ移動。
