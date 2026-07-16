# 0064 Human Task Resume 実装指示書

設計書: [`docs/spec/0064_human-task-resume-spec.md`](../spec/0064_human-task-resume-spec.md)  
親概要: [`docs/spec/0063_human-task-suspend-resume-overview.md`](../spec/0063_human-task-suspend-resume-overview.md)

## 0. 目的

Suspended checkpoint を `ai human-task resume [TASK_ID]` で再開し、保存 cwd / briefing から新しい Human Shell を起動する。再 suspend で segment を追記し、status で確認できるところまでを本番経路として実装する。Done 後の agent continuation は実装しない。

パック構成は設計書 §8 どおり **No**。

## 0.1 Scope Lock

- Feature scope registry: `scripts/feature-scope.toml`
- Scope revision: **1**
- Status: `done`
- Complexity class: **Yellow**（`scope_review = "approved"`）
- Vertical slice AC ID: `human_task_resume_vertical_e2e`
- Locked AC IDs: 設計書 §9 の全 11 ID

## 0.2 Vertical Slice

```text
Suspended checkpoint fixture（実 file store）
→ HumanTaskResume が Running 保存
→ fake launcher が briefing/cwd を受け取り Suspended を返す
→ segment index 1 を追記して Suspended 保存
→ HumanTaskStatus が Resume/Cancel 案内を表示
```

## 1. Phase 分割

| Phase | 内容 | ゲート |
|-------|------|--------|
| 1 | domain invariant 拡張、resume application、CLI、status/固定応答の Resume 案内、vertical E2E | Phase 1 の 5 AC が `pending = false` |
| 2 | 複数 suspend、ID/state/cwd 異常、Done fail-closed、0063 回帰、docs/manual | 残り 6 AC が `pending = false` |

**Vertical Slice Gate**: Phase 1 成功前に Done→ResultPending、continuation、crash recovery へ進まない。

## 2. 受け入れ条件

| ID / テスト関数 | Phase | 配置 |
|------------------|-------|------|
| `human_task_resume_vertical_e2e` | 1 | `ai/tests/0064_human_task_resume_red.rs` |
| `human_task_resume_restores_cwd_and_briefing` | 1 | 同上 |
| `human_task_resume_appends_suspended_segment` | 1 | 同上 |
| `human_task_resume_holds_root_lock` | 1 | 同上 |
| `human_task_status_shows_resume_command` | 1 | 同上（aibe 固定文は同 test または aibe unit） |
| `human_task_resume_supports_multiple_suspends` | 2 | 同上 |
| `human_task_resume_rejects_missing_or_mismatched_id` | 2 | 同上 |
| `human_task_resume_rejects_non_suspended` | 2 | 同上 |
| `human_task_resume_cwd_unavailable_fails_closed` | 2 | 同上 |
| `human_task_resume_done_restores_suspended` | 2 | 同上 |
| `human_task_resume_preserves_single_segment_regression` | 2 | 同上 |

未到達 AC は `#[ignore]` + `pending = true` を先に置き、緑になった行だけ外す。

## 3. レイヤー別実装タスク

### 3.1 Domain

`ai/src/domain/human_task_checkpoint.rs` の `validate()` を拡張する。

- Running: segments が空、またはすべて Suspended 終端の連番
- Suspended: 1 件以上、連番、last.final_cwd == current_cwd

### 3.2 Application

| ファイル | 作業 |
|----------|------|
| `ai/src/application/human_task_resume.rs`（新規） | lock → load → ID/state/cwd 検査 → Running save → launch → Suspended append or Done restore |
| `human_task_status.rs` | Resume 案内行を追加 |
| `aibe/.../agent_turn.rs` | SuspendTurn 固定文に Resume 案内 |

### 3.3 CLI

`HumanTaskCommand::Resume { task_id: Option<String> }` と `main.rs` の `run_human_task_resume`。runtime dir は `allocate_runtime_handoff_path` + guard。

### 3.4 Tests / Docs

`ai/tests/0064_human_task_resume_red.rs`、`docs/manual/0064_human-task-resume.md`、architecture / testing / overview / index 同期。

## 4. 完了条件

1. 全 AC が `pending = false` かつ代表 test 緑
2. `./scripts/verify.sh` 成功
3. 本書を `docs/done/` へ移動し index を更新
