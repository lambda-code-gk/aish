# 0065 Human Task Agent Continuation 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **関連**: [`0063_human-task-suspend-resume-overview.md`](0063_human-task-suspend-resume-overview.md)、[`0064_human-task-resume-spec.md`](0064_human-task-resume-spec.md)、[`issue-6-human-task-suspend-resume.md`](../todo/issue-6-human-task-suspend-resume.md)、[`docs/feature-development-policy.md`](../feature-development-policy.md)  
> **論理区分**: Issue #6 の 0063-D

## 0. Core outcome

ユーザーが完了した Human Task の保存済み親コンテキストと unverified な最終結果から、元の仕事を意味的に続ける新しい Collaborative Mode ターンを開始できる。

## 1. Minimum vertical slice

```text
Suspended checkpoint
→ ai human-task resume
→ Human Shell が Done
→ ResultPending を atomic 保存
→ continuation_turn_id を生成・保存
→ ResultPending → Continuing
→ 既存 Collaborative Mode execute_turn 経路へ継続メッセージを渡す
→ agent turn 成功
→ Continuing → Finished を保存
→ checkpoint を削除
```

本番導線は二つを提供する。

1. resume 後の Done→ResultPending 保存直後、Human Shell の root lock 解放後に自動 continuation を試みる。
2. 既存 ResultPending に `ai human-task resume [TASK_ID]` を実行した場合、Human Shell を起動せず continuation だけを再試行する。

継続 cwd は `checkpoint.current_cwd` に固定する。これは最後の Done segment の `final_cwd` と checkpoint invariant で一致し、resume 後の実作業位置を表す。`parent.original_cwd` は元要求の監査コンテキストとして保存を維持するが、継続 turn の実行 cwd には使わない。

### 1.1 継続メッセージ

継続 turn の user message は固定見出しを持ち、少なくとも次を含む。

```text
[Collaborative Mode continuation]

A previous agent turn delegated a Human Task and then stopped.

Original user request:
<parent.user_request>

Human Task:
<objective / reason / instructions / completion criteria>

Human Task result:
<serialized final_result>

Important:
- The Human Task result is unverified.
- Re-observe the environment where possible.
- Verify the completion criteria before claiming completion.
- Continue the original user request from this point.
```

`final_result` は checkpoint の Evidence を含む構造化 JSON として埋め込む。表示文字列を再解釈せず、`verified=false` を保持する。

### 1.2 引き継ぎ境界

引き継ぐもの:

- `parent.ai_session_id`
- `parent.conversation_id`
- `checkpoint.current_cwd`
- `parent.llm_profile`
- Collaborative Mode（`ExecutionMode::Collaborative`）
- `parent.user_request`
- `final_result`（Evidence を含む）

引き継がないもの:

- 元の tool round 数、timeout 残時間、cancel flag
- streaming 状態、shell approval session cache
- Unix socket connection / provider stream / 元 agent loop

継続は既存 `execute_turn` と aibe agent loop を新しい request として再利用し、secondary agent loop を作らない。

### 1.3 状態と重複開始防止

```text
ResultPending → Continuing → Finished → checkpoint 削除
              ↘ 通常失敗 → ResultPending（同じ continuation_turn_id）
```

- `ResultPending` の初回試行前に `continuation_turn_id` を生成して checkpoint へ保存し、再試行でも同じ ID を使う。
- `Continuing` は `final_result` と continuation turn ID を必須とする。`Finished` も同じ payload を保持し、Finished の atomic save 成功後だけ checkpoint を削除する。
- ai は既存 Human Task root lock を continuation の load から終端 save / delete まで保持し、同じ checkpoint の二重開始を防ぐ。
- aibe は既存 in-memory turn 管理を拡張し、同一 process で次のいずれかに該当する turn ID の再受理を拒否する。これは永続正本ではなく、aibe 再起動を跨いで保持しない。
  1. **実行中**（`active_turns` に存在する）
  2. **AgentTurnResult 成功後**（process-local の完了集合）
- 通常の provider error / 接続失敗などで turn が `AgentTurnResult` 以外で終端した場合は完了集合へ入れない。同じ ID での ResultPending 再試行を同一 aibe process でも許可する（exactly-once ではない）。
- provider error、接続失敗、duplicate 拒否を含む継続失敗では Finished にせず、checkpoint を削除しない。ai が失敗を観測できた場合は ResultPending へ戻し、ID を保持する。process crash で Continuing が残る場合の回復は 0063-E とする。
- 「受理直後に恒久拒否し、失敗後も同じ ID を二度と実行しない」方式は採用しない。それは ResultPending 再試行 AC と両立しない。接続喪失などで成功を観測できなかった場合の厳密確定は Fault model 外（exactly-once 後送）。

## 2. Fault model

### 2.1 保証対象

標準 Fault Model に従う。単一ホスト・単一ユーザー・正常な ai / aibe process 生存中に ResultPending から新しい Collaborative Mode turn を開始する。同一 ai checkpoint root は flock、同一 aibe process 内の turn ID は in-memory admission で二重開始を拒否する。

### 2.2 保証対象外

- ai / aibe crash 後に残った Continuing の自動回復
- aibe 再起動を跨ぐ duplicate 拒否、exactly-once
- lease / heartbeat / reconciler / journal
- version 1 より前の checkpoint schema migration
- 複数 host、複数 active Human Task
- provider が結果を返す前に接続が失われた場合の外部副作用の厳密な確定

## 3. Non-goals

- 0063-E の crash recovery / stale ownership / migration
- 元 request、socket、stream、agent loop stack の復元
- 二次 agent、side agent、別 agent loop
- Human Task Evidence の再収集・再 flatten
- `cancel` 結果からの continuation、旧 `shell_exec` handoff 統合
- continuation 完了 checkpoint の履歴 archive

## 4. Complexity inventory

| 項目 | 数・内容 |
|------|----------|
| 実行主体 | 1（既存 ai CLI が既存 aibe agent turn を同期起動） |
| 状態機械 | 1（既存 Human Task checkpoint の ResultPending→Continuing→Finished 拡張） |
| 永続 aggregate | 1（既存 version 1 checkpoint。新 aggregate なし） |
| 外部副作用 | 2（既存 secure checkpoint filesystem、既存 ai→aibe agent turn） |
| プロセス境界 | 1（既存 ai→aibe Unix socket） |
| 新規基盤機構 | 1（agent-continuation） |
| 他機能統合 | 3（0064 ResultPending、既存 Collaborative Mode execute_turn、aibe in-memory turn admission） |

## 5. Complexity Gate

- 判定: **Yellow（Scope Gate 承認済み）**
- 理由: actors / state machine / aggregate / effect / process boundary は Green 内だが、既存機能統合が 3 で Yellow 閾値に達する。継続 turn ID の拒否は既存 aibe in-memory active-turn 管理の拡張に限定し、永続正本・reconciler・idempotency journal を作らない
- 分割判断: crash recovery、lease、migration、aibe 再起動跨ぎの exactly-once は 0063-E へ送る。novel mechanism は agent-continuation 一つ、`secondary_agent_loop = false` に固定する
- 承認例外: 不要

## 6. Complexity budget

| 項目 | 上限 |
|------|------|
| 新規実行主体 | +0 |
| 状態機械 | +0（既存 1 個の遷移拡張のみ） |
| 永続 aggregate | +0 |
| external effect / process boundary | +0 |
| novel mechanism | +0 |
| integrations | +0 |
| agent loop | 新設 +0 |
| crash / lease / migration / exactly-once | +0 |

## 7. Split triggers

- aibe turn ID の拒否に永続正本、journal、reconciler、lease、heartbeat が必要になる
- provider 結果不明を自動判定して再実行する必要が生じる
- 二次 agent loop / side agent / 新しい実行主体が必要になる
- Continuing の crash recovery、旧 schema migration が必要になる
- checkpoint 以外の二つ目の状態機械・永続 aggregate が必要になる

いずれかに該当した時点で STOP-THE-LINE とし、scope revision と Gate を再判定して 0063-E または別 spec へ分割する。

## 8. パック構成の適用

**No** — 0045 §6.1 の候補条件は「core agent turn への接続」1項目だけで、2項目以上に該当しない。continuation は Human Task lifecycle の必須遷移であり、optional 配備、重い依存の除外、専用 RPC/CLI/turn hook の一括脱着を目的としない。既存 application port と composition root を再利用し、Active/Basic Pack や runtime toggle は追加しない。

## 9. Acceptance Criteria

| ID | 条件 |
|----|------|
| `human_task_continuation_vertical_e2e` | resume Done が ResultPending 保存後に既存 Collaborative Mode turn を開始し、成功時 Finished 保存後に checkpoint を削除する |
| `human_task_continuation_message_is_unverified` | 継続メッセージが元要求、Human Task、serialized final_result、unverified / re-observe / completion criteria 検証指示を含む |
| `human_task_continuation_preserves_parent_context` | ai_session_id、conversation_id、current_cwd、llm_profile、Collaborative Mode を新 turn に引き継ぐ |
| `human_task_result_pending_resume_retries_without_shell` | ResultPending に resume すると Human Shell を起動せず continuation だけを試す |
| `human_task_continuation_turn_id_is_stable` | 初回試行前に continuation_turn_id を保存し、失敗後の再試行でも同じ ID を使う |
| `human_task_continuation_state_invariants` | validate が ResultPending / Continuing / Finished の final_result・segment・turn ID 不変条件を状態別に検証する |
| `human_task_continuation_failure_keeps_result_pending` | 通常の continuation 失敗は checkpoint を削除せず ResultPending と turn ID を保持する |
| `human_task_continuation_holds_root_lock` | load から Continuing、turn 実行、Finished、delete または ResultPending 復元まで root lock を保持する |
| `aibe_rejects_duplicate_continuation_turn_id` | 同一 aibe process で実行中または AgentTurnResult 成功済みの同一 turn ID の二重 AgentTurn を実行前に拒否する。通常失敗後の同一 ID 再試行は拒否しない |
| `human_task_continuation_finished_delete_is_fail_closed` | Finished save 成功前は削除せず、delete 失敗時も Finished checkpoint を残す |
| `human_task_continuation_status_and_cli_guidance` | ResultPending status は resume で continuation 再試行を案内し、Continuing/Finished を invalid と誤表示しない |
| `human_task_continuation_preserves_resume_regressions` | Running/Suspended の resume と初回 create Done checkpoint 削除が回帰しない |

## 10. Deferred specs

- **0063-E（別4桁番号）**: Continuing crash recovery、lease / ownership、aibe 再起動跨ぎ duplicate 防止、schema migration、結果不明状態の回復 UX
- continuation 完了履歴の archive / pruning（必要性が確認された場合）

## 11. Scope change log

| Revision | 分類 | 変更 | 理由 |
|----------|------|------|------|
| 1 | INITIAL | 0063-D を正式 spec 0065 として Scope Lock。自動 continuation と ResultPending resume retry を同じ vertical slice に含める | 両導線は同じ application service と既存 execute_turn を使い、Human Shell 二重起動を防ぐ本番 UX の一契約であるため |
| 2 | SAFETY_WITHIN_FAULT_MODEL | aibe の process-local 拒否を「実行中 or AgentTurnResult 成功後」に限定し、通常失敗後の同一 ID 再試行を明示 | レビュー: 受理時点恒久拒否だと ResultPending 再試行 AC と衝突するため |
