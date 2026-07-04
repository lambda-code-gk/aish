# 0055 — Collaborative Human Handoff Phase 2 実装指示書

> **種別**: 実装指示書（`docs/tasks/`）  
> **設計正本**: [0055_collaborative-human-handoff-spec.md](../spec/0055_collaborative-human-handoff-spec.md)  
> **マスター**: [0055_collaborative-human-handoff-implementation-spec.md](0055_collaborative-human-handoff-implementation-spec.md)  
> **状態**: 未着手  
> **前提**: Phase 1 完了

## 0. 目的

`ai --collaborative` と親 `shell_exec` の handoff 化、human shell 起動（**handoff 環境変数設定含む**）、command candidate の Alt+. / Alt+, 連携、親への synthetic tool result、正常フロー状態遷移、再観測までを通す。side agent 接続検証・復旧は Phase 3–4。プロンプト hook 表示は Phase 5。

## 1. スコープ

### 1.1 対象

| 項目 | 設計参照 |
|------|----------|
| `ai --collaborative "task"` CLI フラグ | spec §5.1 |
| `CollaborativeExecutionContext`（`role=PARENT`, `policy=ENABLED`） | spec §6, §30 |
| `CollaborativeShellExecPolicy`（**承認 UI ではなく execution policy**） | spec §30 |
| 親 `shell_exec` 前に他親ツール完了待ち（§7.1 バリア） | spec §7.1 |
| `CreateHumanHandoff` + child goal 作成（§22） | spec §10, §22 |
| cwd 不存在時は handoff 起動せずエラー | spec §3.1, §8.3 |
| checkpoint 保存 **後に** human shell 起動 | spec §16, §24.3 |
| human shell 起動時 env 設定（`AISH_CONTROL_MODE` 等） | spec §13.1, §31 |
| `HumanShellLauncher` → `aish` PTY 子シェル | spec §9 |
| handoff 候補を 0053 recall へ登録（source=PARENT_AGENT） | spec §14, §21 |
| Ctrl+D / `exit` / `exit N`、control channel 正常終了マーカー | spec §9.6–9.7 |
| `final_shell_cwd` 記録 | spec §9.4, §24.1 |
| 親 loop 同期待機 → `HumanHandoffResult` tool result + LLM 説明文 | spec §17, §26 |
| `RETURNED` → `RESUMING_PARENT` → `COMPLETED` 正常遷移 | spec §9, §19 |
| 親再観測（git / cwd / shell log delta 等） | spec §18, §27 |
| 複数 `shell_exec` 直列 handoff（同時 human shell 禁止） | spec §7.2, §20 |

### 1.2 非対象

| 項目 | Phase |
|------|-------|
| human shell 内 `ai` の token 検証・side 接続 | 3 |
| `ai resume` / ORPHANED | 4 |
| プロンプト状態表示 hook | 5 |
| `shell_exec.environment`（schema 未存在） | 将来（spec §20） |

## 2. 受け入れ条件（`spec = "0055"`）

| id | 条件 | テスト関数 | pending |
|----|------|------------|---------|
| `collaborative_flag_starts_parent` | `--collaborative` で親ループが開始される | `collaborative_flag_enables_parent_policy` | true |
| `parent_shell_exec_handoff` | 親のみ `shell_exec` が handoff される | `parent_shell_exec_creates_handoff_instead_of_exec` | true |
| `normal_shell_exec_unchanged` | 非協調モードは既存自動実行 | `normal_mode_shell_exec_still_auto_executes` | true |
| `non_parent_role_skips_handoff` | PARENT 以外は handoff しない | `non_parent_role_skips_handoff` | true |
| `shell_exec_not_yes_no_approval` | 協調モードで yes/no 承認に置換されない | `collaborative_shell_exec_skips_approval_prompt` | true |
| `parent_tools_barrier_before_handoff` | handoff 前に開始済み親ツールを完了させる | `parent_tools_complete_before_handoff_starts` | true |
| `cwd_missing_rejects_handoff` | cwd 不存在時は shell 起動せずエラー | `missing_cwd_rejects_human_shell_spawn` | true |
| `checkpoint_before_shell` | shell 起動前に checkpoint が存在 | `checkpoint_persisted_before_human_shell_spawn` | true |
| `handoff_env_set_on_spawn` | human shell 子プロセスに handoff env が設定される | `human_shell_child_has_handoff_env_vars` | true |
| `candidate_in_recall_queue` | 候補が recall から取得できる | `handoff_candidate_available_via_recall` | true |
| `candidate_inserts_command_only` | Alt+. は command 文字列のみ挿入 | `recall_inserts_command_text_only` | true |
| `recall_prev_cycles_candidates` | Alt+, で逆順に候補をたどれる | `recall_prev_cycles_handoff_candidates` | true |
| `candidate_not_treated_as_executed` | 候補挿入だけでは達成扱いにしない | `candidate_insertion_does_not_mark_command_executed` | true |
| `human_shell_ctrl_d_returns` | Ctrl+D で親へ戻る | `human_shell_ctrl_d_returns_control_to_parent` | true |
| `human_shell_exit_returns` | `exit` / `exit 1` も正常返却 | `human_shell_exit_returns_control_regardless_of_code` | true |
| `final_shell_cwd_recorded` | 終了時 final cwd が handoff に保存される | `handoff_records_final_shell_cwd_on_return` | true |
| `tool_result_not_success` | `requested_command_completion=unknown` | `handoff_tool_result_marks_command_completion_unknown` | true |
| `parent_reobserves` | 返却後に再観測コンテキストが親へ渡る | `parent_receives_reobservation_after_handoff` | true |
| `parent_state_normal_flow` | RETURNED→RESUMING_PARENT→COMPLETED | `handoff_completes_normal_parent_resume_flow` | true |
| `no_parallel_human_shells` | 2 つ目の handoff は直列待ち | `second_shell_exec_waits_for_first_handoff` | true |

## 3. 変更ファイル（目安）

| 区分 | パス |
|------|------|
| ai CLI | `ai/src/main.rs`, `ai/src/clap_cli.rs` |
| ai app | `ai/src/application/collaborative_handoff.rs` |
| ai policy | `ai/src/adapters/outbound/collaborative_shell_exec_policy.rs` |
| aibe-client | tool result メタ / handoff 合成 |
| aish | `human_shell.rs`, `pty_shell.rs` 拡張 |
| recall | `ai/src/application/suggested_command_recall.rs` |
| tests | `ai/tests/0055_collaborative_handoff_e2e.rs`, `aish/tests/0055_collaborative_handoff_red.rs` |

## 4. 実装手順

### 4.1 `CollaborativeShellExecPolicy`（spec §30）

親かつ協調 policy 時、承認プロンプトの **前** に handoff へ分岐。`ShellExecApproval` UI は出さない。

フロー:

1. 進行中の他親ツールを完了待ち
2. handoff + child goal + `RequestedShellExec`
3. candidate 登録 + recall queue
4. checkpoint（失敗時は `CANCELLED` — Phase 4 で fault テスト）
5. `SpawnHumanShell`（cwd 検証済み）+ **env 注入**
6. 親 loop ブロック待ち
7. `HumanHandoffResult` + 再観測を tool result として返却
8. `RETURNED` 永続化 → `RESUMING_PARENT` → 親継続 → `COMPLETED`

### 4.2 human shell（aish）

- `PtyShell` ラップ + control FIFO 正常終了マーカー
- 異常終了はマーカー無し（Phase 4 で ORPHANED）

### 4.3 再観測

`EnvironmentObserver`: cwd、git HEAD/branch/status、shell log 範囲。詳細は spec §27。

## 5. 検証

```bash
./scripts/verify-targeted.sh --package ai
./scripts/verify-targeted.sh --package aish
cargo test -p ai --test 0055_collaborative_handoff_e2e -j 1
cargo test -p aish --test 0055_collaborative_handoff_red -j 1
```
