# 0055 — Collaborative Human Handoff Phase 3 実装指示書

> **種別**: 実装指示書（`docs/done/`）  
> **設計正本**: [0055_collaborative-human-handoff-spec.md](../spec/0055_collaborative-human-handoff-spec.md)  
> **マスター**: [0055_collaborative-human-handoff-implementation-spec.md](0055_collaborative-human-handoff-implementation-spec.md)  
> **状態**: 実装済み（Phase 3）
> **前提**: Phase 2 完了（human shell は env 付きで起動済み）

## 0. 目的

human shell 内 `ai` の **検証と side agent 接続**、親文脈継承、会話要約、人間待ち、裸 `ai` 分岐、`--standalone`、**既存 `ai status` 拡張**を実装する。復旧は Phase 4。

**注意**: handoff env の **設定**は Phase 2。本 Phase は **読取・検証・接続**。

## 1. スコープ

### 1.1 対象

| 項目 | 設計参照 |
|------|----------|
| 起動判定順（standalone → env 欠落 → token 検証 → 状態） | spec §13.5, §23.4 |
| token / generation / host ID / UID 検証 | spec §13.4, §29 |
| 無効 handoff の明示エラー（黙ってフォールバック禁止） | spec §13.5 |
| side conversation 遅延作成・一意性 | spec §11.1 |
| 親文脈注入（§23.1 全項目） | spec §11.3, §23.1 |
| 会話要約更新（side run 前後・人間待ち時） | spec §25 |
| side agent の通常 `shell_exec` | spec §11.4 |
| `request_human_action` + `HumanControlReturned` 全フィールド | spec §12, §23.2–23.3 |
| `HUMAN_ACTIVE` 裸 `ai` → 通常入力 UI | spec §5.3, §23.4 |
| `SIDE_AGENT_WAITING` 裸 `ai` / `ai <note>` 再開 | spec §5.3 |
| `SIDE_AGENT_RUNNING` 時の新 run 拒否 | spec §13.6, §23.4 |
| `ai --standalone` + handoff env 除去 | spec §5.4, §23.3 |
| **既存 `ai status` / `doctor` 拡張**（§5.1） | spec §5.5 |
| 入れ子 `ai --collaborative` 拒否 | spec §5.7 |
| side から入れ子 human shell 禁止 | spec §12.1 |
| side agent run lease 排他 | spec §24.2 |
| token が LLM 入力に入らない | spec §29 |

### 1.2 非対象

| 項目 | Phase |
|------|-------|
| `ai resume` / ORPHANED 復旧 | 4 |
| heartbeat / lease 失効監視 | 4 |
| プロンプト hook 表示 | 5 |

## 2. 受け入れ条件（`spec = "0055"`）

| id | 条件 | テスト関数 | pending |
|----|------|------------|---------|
| `side_conversation_continues` | 同一 handoff で side conversation 継続 | `side_agent_reuses_conversation_in_handoff` | false |
| `side_conversation_unique` | handoff あたり side conversation は 1 つ | `side_conversation_unique_per_handoff` | false |
| `side_inherits_parent_context` | side turn に親要約・文脈が含まれる | `side_agent_receives_parent_task_context` | false |
| `side_contextual_memory_block` | side system context に contextual memory block を含める | `side_agent_includes_contextual_memory_block` | false |
| `conversation_summary_on_side_turn` | side 前後で要約が更新される | `conversation_summary_updates_on_side_turn` | false |
| `side_shell_exec_normal` | side の `shell_exec` は自動実行 | `side_agent_shell_exec_executes_normally` | false |
| `request_human_action` | 人間待ちで新 shell を作らない | `side_agent_waiting_does_not_spawn_new_shell` | false |
| `bare_ai_human_active_ui` | HUMAN_ACTIVE で裸 `ai` が入力 UI を開く | `bare_ai_in_human_active_opens_input_ui` | false |
| `bare_ai_resumes_side` | WAITING で裸 `ai` が side 再開 | `bare_ai_resumes_side_agent_from_waiting` | false |
| `ai_note_becomes_user_note` | `ai <補足>` → `user_note` | `ai_with_note_sets_user_note_on_resume` | false |
| `human_control_returned_fields` | HumanControlReturned に必須フィールド | `human_control_returned_includes_required_fields` | false |
| `side_agent_running_blocks_new_run` | RUNNING 中は新 run 拒否 | `side_agent_running_rejects_new_run` | false |
| `standalone_ignores_handoff` | `--standalone` は handoff 無視 | `standalone_mode_ignores_handoff_context` | false |
| `standalone_strips_handoff_env` | standalone 子に token 無し | `standalone_child_process_has_no_handoff_token` | false |
| `ai_status_no_llm` | `ai status` は LLM / turn を作らない | `ai_status_does_not_invoke_llm` | false |
| `status_no_token` | status 出力に token 無し | `ai_status_never_prints_handoff_token` | false |
| `status_shows_handoff_fields` | 親タスク・状態・候補・再開ヒントを表示 | `ai_status_shows_collaborative_handoff_fields` | false |
| `existing_ai_status_regression` | handoff 無し時は従来 status と同じ | `ai_status_unchanged_without_active_handoff` | false |
| `stale_token_rejected` | 古い generation を拒否 | `stale_handoff_token_is_rejected` | false |
| `uid_mismatch_rejected` | UID 不一致を拒否 | `handoff_rejected_when_effective_uid_mismatches` | false |
| `host_id_mismatch_rejected` | host ID 不一致を拒否 | `handoff_rejected_when_host_id_mismatches` | false |
| `tampered_handoff_id_rejected` | 存在しない handoff ID を拒否 | `tampered_handoff_id_is_rejected` | false |
| `incomplete_env_no_fallback` | env 一部欠落で通常 ai に落とさない | `incomplete_handoff_env_shows_error_not_fallback` | false |
| `nested_collaborative_rejected` | 入れ子 `--collaborative` 拒否 | `nested_collaborative_flag_is_rejected` | false |
| `side_no_nested_human_shell` | side から human shell を起動しない | `side_agent_cannot_spawn_nested_human_shell` | false |
| `orphaned_requires_resume` | ORPHANED は `ai resume` を促す | `orphaned_handoff_direct_ai_shows_resume_hint` | false |
| `token_not_in_llm_context` | side turn の LLM 入力に token 無し | `handoff_token_not_in_llm_context` | false |

## 3. 変更ファイル（目安）

| 区分 | パス |
|------|------|
| ai CLI | `ai/src/main.rs`, `ai/src/clap_cli.rs`（Status 拡張） |
| ai app | `StartOrResumeSideAgent`, `ReadCollaborativeStatus`, 要約更新 |
| ai domain | `HumanControlReturned` |
| aibe | side system 注入、control outcome |
| tests | `ai/tests/0055_collaborative_side_agent.rs` |

## 4. 実装手順

### 4.1 `ai status` 拡張（spec §5.1）

1. handoff store を読む
2. active handoff があれば人間向けブロックを出力（§5.5 例）
3. 従来の aibe クライアント status を続行
4. JSON 時は `collaborative_handoff` フィールドに構造化

### 4.2 side 起動判定

spec §13.5 の順序をそのまま実装。エラー時は spec 記載の案内文を stderr に出す。

## 5. 検証

```bash
./scripts/verify-targeted.sh --package ai
./scripts/verify-targeted.sh --package aibe
cargo test -p ai --test 0055_collaborative_side_agent -j 1
```
