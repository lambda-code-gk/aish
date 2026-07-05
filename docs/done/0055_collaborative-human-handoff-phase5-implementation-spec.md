# 0055 — Collaborative Human Handoff Phase 5 実装指示書

> **種別**: 実装指示書（`docs/done/`）  
> **設計正本**: [0055_collaborative-human-handoff-spec.md](../spec/0055_collaborative-human-handoff-spec.md)  
> **マスター**: [0055_collaborative-human-handoff-implementation-spec.md](0055_collaborative-human-handoff-implementation-spec.md)  
> **状態**: 実装済み（Phase 5）  
> **前提**: Phase 4 完了

## 0. 目的

プロンプト状態表示、シグナル・job control、ログ redaction、監査イベント、`collaborative.*` 設定、docs / manual を仕上げ、設計書 §17（25 項目）を最終照合する。

## 1. スコープ

### 1.1 対象

| 項目 | 設計参照 |
|------|----------|
| human shell プロンプト常時表示（無効化不可） | spec §9.5 |
| nested shell への表示継承（ベストエフォート） | spec §9.5 |
| SIGWINCH 伝播確認 | spec §9.3 |
| shell log / replay の token redaction | spec §29 |
| 監査イベント（§29 一覧） | spec §29 |
| `[collaborative]` TOML 設定 | spec §15, §26 |
| `docs/architecture.md`, `security.md`, `manual/` | 全体 |
| §17 受け入れ条件 1–25 最終監査 | spec §17, §32 |

## 2. 受け入れ条件（`spec = "0055"`）

| id | 条件 | テスト関数 | pending |
|----|------|------------|---------|
| `prompt_shows_collaborative_state` | プロンプトに協調状態 | `human_shell_prompt_shows_collaborative_status` | true |
| `prompt_waiting_shows_hint` | 人間待ち時の表示 | `human_shell_prompt_shows_waiting_for_side_agent` | true |
| `token_redacted_in_shell_log` | shell log に token 無し | `handoff_token_redacted_from_shell_log` | true |
| `token_not_in_replay` | replay に token 無し | `handoff_token_not_in_replay_output` | true |
| `ctrl_c_does_not_kill_parent` | Ctrl+C が親を終了しない | `ctrl_c_in_human_shell_does_not_terminate_parent` | true |
| `job_control_works` | Ctrl+Z / fg / bg | `human_shell_job_control_fg_bg` | true |
| `long_running_lease_held` | 長時間コマンド中 lease 維持 | `heartbeat_maintains_lease_during_long_command` | true |
| `audit_events_emitted` | 主要監査イベントが events.jsonl に記録 | `collaborative_audit_events_are_emitted` | true |
| `collaborative_config_wired` | `[collaborative]` 設定が読める | `collaborative_config_defaults_match_spec` | true |
| `normal_ai_unchanged_regression` | 非協調 `ai` smoke | `normal_ai_entry_unchanged_regression` | true |
| `docs_architecture_synced` | architecture.md に協調節 | `docs_architecture_mentions_collaborative_handoff` | true |
| `manual_checklist_exists` | manual 手順あり | `manual_collaborative_handoff_checklist_exists` | true |
| `reconcile_orphan_side_run_lock` | HUMAN_ACTIVE 時に stale side-run lock を除去 | `reconcile_orphan_side_run_lock_clears_stale_lock_for_human_active` | false |
| `durable_tool_lifecycle` | checkpoint へ RUNNING/完了 tool lifecycle を記録 | `tool_lifecycle_records_running_and_syncs_completed` | false |
| `parent_resume_tool_lifecycle` | 親 RESUMING_PARENT turn の tool lifecycle を checkpoint へ同期 | `parent_resume_tool_lifecycle_syncs_completed_tools` | false |

## 3. 手動検証（`docs/manual/collaborative-handoff.md`）

1. `ai --collaborative` → handoff → Alt+. / Alt+, → 編集実行 → Ctrl+D
2. human shell 内 `ai` 継続
3. side 人間待ち → `ai` 再開
4. `ai --standalone`
5. kill → `ai resume`
6. `ai status`（handoff あり/なし）

## 4. 検証

```bash
./scripts/verify.sh
```

## 5. 完了後

`docs/tasks/0055_*` → `docs/done/`、`docs/0000_spec-index.md` 更新。
