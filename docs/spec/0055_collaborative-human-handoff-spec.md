# 0055 — AISH Human-in-the-loop 協調作業（Collaborative Handoff）設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **起票**: 2026-07-04  
> **実装**: [マスター指示書](../tasks/0055_collaborative-human-handoff-implementation-spec.md)（Phase 1–5 に分割）  
> **関連**: [0030_ai-smart-entry-spec.md](0030_ai-smart-entry-spec.md)、[0034_aibe-contextual-memory-spec.md](0034_aibe-contextual-memory-spec.md)、[0036_shell-exec-approval-ux-spec.md](0036_shell-exec-approval-ux-spec.md)、[0049_aish-command-output-replay-spec.md](0049_aish-command-output-replay-spec.md)、[0050_client-provided-replay-tool-spec.md](0050_client-provided-replay-tool-spec.md)、[0052_ai_work.md](0052_ai_work.md)、[0053_ai-suggested-command-recall-spec.md](0053_ai-suggested-command-recall-spec.md)、[0045_pack-composition-spec.md](0045_pack-composition-spec.md)、[architecture.md](../architecture.md)、[security.md](../security.md)、[testing.md](../testing.md)

## 0. 目的

親エージェント（`ai --collaborative`）が `shell_exec` を要求したとき、AISH は自動実行せず **human shell**（実対話子シェル）へ制御を移譲する。人間は Alt+. / Alt+, で候補コマンドを取り込み、確認・編集・実行する。Ctrl+D / `exit` で親へ制御が戻り、親は **成功を仮定せず再観測** して継続する。

協調制御ループ:

```text
親エージェント (ai --collaborative)
  ↓ shell_exec 要求
human shell へ制御移譲
  ↓
人間が確認・編集・実行
  ↓
必要なら side agent (human shell 内の ai) へ相談
  ↓
Ctrl+D または exit
  ↓
親エージェントが再観測して続行
```

初版から **同一ホスト・同一 OS ユーザー** におけるクラッシュ、端末切断、再起動後の復旧を含む。

## 1. 非目標（初版）

- 別ホスト handoff、複数人共同操作、SSH 先への親文脈自動転送
- 親と side agent の並列実行、複数 side conversation
- provider 固有の未完了 tool-call ID への再接続
- human shell 内 `export` の完全復元、プロセスイメージ復元
- 自動コマンド成功判定、候補の構文解析・分割
- 協調作業専用の完了承認画面
- human shell 内の `ai --collaborative`（入れ子協調作業）

## 2. パック構成の適用

**No**（core 固定 + CLI フラグ駆動）

理由:

1. 協調作業は `ai` / `aibe` / `aish` の **横断フロー** であり、単一 optional pack の trait 1 本に閉じない。
2. 無効時は `--collaborative` 未指定の既存経路がそのまま動く。Basic Pack 相当の no-op は「通常モード」そのもの。
3. Contextual Memory Pack（0038）や Smart Features（0041）とは独立。side agent は既存 memory / replay を **再利用** するのみ。

`enabled=false` の管理設定は将来可。`--collaborative` 指定時に全体無効なら **明示エラー**（黙って通常モードに落とさない）。

## 3. リポジトリ既存名称マッピング（実装前確認の正本）

論理名（本仕様）とリポジトリ実名の対応。不要な一括改名は行わない。

| 論理名 | リポジトリ実名 / 所在 | 備考 |
|--------|----------------------|------|
| `ShellExec` | `shell_exec`（`aibe_protocol::SHELL_EXEC`） | args: `command` + `args[]`。cwd は `ToolExecutionContext::base_dir`（`context.cwd`） |
| Parent agent | `ai --collaborative` 起動の agent turn loop | 通常 `ai ask` / smart entry と同じ `aibe-client` 経路 |
| Side agent | human shell 内の `ai`（handoff 関連付け時） | 別 `conversation_id`、同一 `AI_SESSION_ID` 可 |
| `goal` | Contextual Memory kind `goal` + `ai goal set` / `ai work` | child goal は handoff メタ + memory `goal` 子エントリ |
| `conversation` | `ConversationStore`（`~/.local/share/aibe/conversations/<AI_SESSION_ID>/`） | `conversation_id` per thread |
| `session` | `AI_SESSION_ID`（`ai` が発行・export） | `memory_space_id` / `AIBE_CONTEXT_ID` は別軸（0035） |
| `run` | agent turn 1 回 + tool round 連鎖 | 明示的 Run エンティティは無い。**handoff は turn 中断点** として新設 |
| Contextual Memory | `MemoryService` / `contextual_memory_pack` | 注入は turn 前 `TurnHook` |
| replay | `aish replay` + client tool `aish.replay_show` | shell span は `aish` session JSONL |
| shell log | `aish` session log + `AgentTurnContext.shell_log_tail` | `AISH_SESSION_DIR` 連携（0019） |
| Command candidate（Alt+.） | `SuggestedCommandCache`（0053）+ **handoff 専用 queue** | 0053 は assistant fenced block 由来。handoff は **別 source** で同一 recall 機構へ統合 |
| Alt+. / Alt+, | `ai recall next` / `ai recall prev` + bash/zsh hook | `aish shell` / `ai complete` 経由（0053） |
| Tool 実行状態 | `ExecutedToolCall` / `ExecutedToolStatus`（`aibe-protocol`） | handoff 結果は `execution_outcome = human_control_returned` を拡張 |
| Human shell | `PtyShell`（`aish/src/adapters/outbound/pty_shell.rs`） | 新設 `HumanShellLauncher` port が `aish` PTY を起動 |
| Agent execution context | **新設** `CollaborativeExecutionContext`（`ai` + protocol 拡張） | `AgentTurnContext` とは別。role / policy / handoff_id |
| Work / 親タスク | `WorkState` / `ai work`（0052） | 協調モード開始時の親タスク文は work + conversation に保存 |

### 3.1 `shell_exec` と候補コマンド文字列

現行 `shell_exec` は **シェル 1 行文字列ではなく** `command` + `args[]` を受け取る。協調作業では:

1. **保存・表示用 `candidate_command`** を組み立てる（分解・再解釈はしない）
2. 組み立て規則: `args` が空なら `command` のみ。非空なら各 arg をシェル安全引用し空白連結（`command` 本体は変更しない）
3. **初版対象外**: `environment` フィールドは現行 `shell_exec` schema に無い（§20 意図的簡略化参照）

cwd: `ToolExecutionContext::base_dir()` を human shell 起動 cwd とする。候補へ `cd` を付加しない。cwd が存在しない・解決不能なら human shell を起動せず、親 `shell_exec` へ明示エラーを返す。

### 3.2 永続化レイアウト（新設）

```text
~/.local/share/aibe/handoffs/
  index.jsonl
  <handoff_id>/
    handoff.json          # Handoff 本体
    lease.json            # HandoffLease（原子的更新）
    shell_sessions.jsonl  # HandoffShellSession 世代
    checkpoint.json       # 直近 checkpoint
    candidates.jsonl      # CommandCandidate イベント
    events.jsonl          # 監査イベント（token 除外）
```

権限: ディレクトリ `0700`、ファイル `0600`（conversation store と同様）。

## 4. 用語

| 用語 | 意味 |
|------|------|
| Parent agent | `ai --collaborative` の親エージェント |
| Human shell | 親 `shell_exec` により起動される実対話子シェル |
| Side agent | human shell 内 `ai`（handoff 接続時） |
| Handoff | 親 → human shell → 親 の制御単位 |
| Child goal | 親 goal の子。human shell 作業目的 |
| Command candidate | プロンプト挿入候補（source: PARENT_AGENT / SIDE_AGENT / HISTORY / MANUAL） |

## 5. CLI 仕様

| コマンド | 挙動 |
|----------|------|
| `ai --collaborative "…"` | 親エージェント全体に協調 policy 適用 |
| human shell 内 `ai …` | side agent（有効 handoff 時） |
| human shell 内 `ai` | 状態別 dispatch（`HUMAN_ACTIVE` / `SIDE_AGENT_WAITING_FOR_HUMAN`） |
| `ai --standalone "…"` | handoff 無視の 1 回限り通常セッション |
| `ai status` | 下記 §5.1（既存コマンド拡張） |
| `ai resume` / `ai resume <id>` | ORPHANED / RETURNED 復旧 |
| human shell 内 `ai --collaborative` | **拒否**（入れ子不可） |

### 5.1 `ai status` と既存 `Status` の統合

リポジトリには既に `ai status` / `ai doctor`（aibe ソケット・クライアント状態）がある。初版では **既存サブコマンドを拡張** し、協調作業状態を追加表示する（新サブコマンドは作らない）。

挙動:

1. LLM / aibe turn は **作成しない**（従来どおり）
2. 現ユーザーの復旧可能 handoff をローカル store から読む
3. **handoff が 1 件以上あるとき**、人間向けテキストブロックを stdout に出力する（§5.5 表示例）
4. その後、**従来の aibe クライアント status** を続けて出力する（`--format json` 時は collaborative ブロックを JSON の別フィールド `collaborative_handoff` に入れる）
5. handoff が無いときは従来の `ai status` と同一挙動（回帰）

`ai doctor` も同様に collaborative 節を含める。

token・秘密情報は表示しない。conversation へユーザーメッセージを追加しない。

環境変数（human shell 起動時）:

```bash
AISH_CONTROL_MODE=human-shell
AISH_HANDOFF_ID=<uuid>
AISH_HANDOFF_TOKEN=<opaque>   # 平文はプロセス内のみ。永続化は hash
AISH_HANDOFF_CONTEXT_VERSION=1
```

正本は永続化 handoff。環境変数は hint。無効時は黙って通常 `ai` にフォールバック **しない**。

## 6. 親 `shell_exec` 実行ポリシー

条件（**両方**）:

```text
CollaborativeExecutionContext.role == PARENT
CollaborativeExecutionContext.collaborative_policy == ENABLED
```

満たすとき:

```text
shell_exec → command candidate 登録 → checkpoint → human shell 起動 → 親 loop 停止待ち
```

side agent / standalone は通常 `shell_exec`（自動実行）。親 policy を継承しない。

同一 assistant turn の複数 `shell_exec`: 各候補を独立登録。実装都合で **1 ShellExec = 1 handoff** 直列でも可。複数 human shell 同時起動は禁止。

## 7. Human shell

- 内部モード切替ではなく **子 PTY シェル**（`PtyShell`）
- 既存 `aish` shell 設定 / ユーザー対話シェルを使用
- Ctrl+C / job control / TUI はシェルに委譲
- プロンプトに協調状態を常時表示（文言・色は設定可、無効化不可）
- 正常返却: Ctrl+D、`exit`、`exit N`（exit code は handoff 失敗ではない）
- 異常: SIGHUP / SIGKILL / クラッシュ等 → `ORPHANED`、親自動再開しない

## 8. Side agent

- conversation は **遅延作成**（初回 `ai` 時）。`UNIQUE(handoff_id)` 相当
- 親文脈: work goal、conversation 要約、直近ターン、memory、replay 参照、child goal
- 人間待ち: `request_human_action` 相当の control outcome → `SIDE_AGENT_WAITING_FOR_HUMAN`
- 裸 `ai` 再開: 構造化 `HumanControlReturned` イベント（`user_note` 可）

## 9. 状態モデル

状態一覧: `CREATING`, `HUMAN_ACTIVE`, `SIDE_AGENT_RUNNING`, `SIDE_AGENT_WAITING_FOR_HUMAN`, `RETURNED`, `ORPHANED`, `RESUMING_PARENT`, `COMPLETED`, `CANCELLED`

```text
CREATING → HUMAN_ACTIVE → RETURNED → RESUMING_PARENT → COMPLETED
                ↕ SIDE_AGENT_RUNNING ↔ SIDE_AGENT_WAITING_FOR_HUMAN
CREATING → CANCELLED（checkpoint 後・shell 起動前失敗）
任意 active → ORPHANED（異常）→ HUMAN_ACTIVE（ai resume）
RETURNED → RESUMING_PARENT 失敗 → RETURNED
```

`SIDE_AGENT_RUNNING` 中に human shell が Ctrl+D / `exit` された場合: side run を中断扱い、実行中ツールを `UNKNOWN` または `CANCELLED` に確定してから `RETURNED` へ。

## 10. 永続化・リース・セキュリティ

- `HandoffLease`: 原子的取得。heartbeat は supervisor が prompt 非依存で更新
- token: ランダム発行、保存は hash、ログ / `ai status` / LLM へ露出禁止
- UID / host ID / shell generation 検証。`sudo -E ai` は拒否
- ツール子プロセス・`--standalone` から handoff env を除去

## 11. 親ツール結果と再観測

human shell 終了は **コマンド成功ではない**。tool result:

```text
execution_outcome: human_control_returned
requested_command_completion: unknown
```

親再開前に cwd / git / shell log delta 等を再観測（高コスト走査は必須にしない）。

## 12. 復旧（概要）

| 状態 | `ai resume` の挙動 |
|------|-------------------|
| `ORPHANED` | 新 human shell、token rotation、同一 handoff_id |
| `RETURNED`（親再開失敗） | human shell なしで親 run を新規作成 |

`RUNNING` ツールは `UNKNOWN`。自動再実行禁止。

## 13. アーキテクチャ責務

### Domain（`ai` クレート中心 + 共有型は `aibe-protocol`）

Handoff 状態遷移、lease 規則、command candidate source、role、human control return 意味、復旧可否。

### Application services（`ai`）

`StartCollaborativeRun`, `InterceptParentShellExec`, `CreateHumanHandoff`, `SpawnHumanShell`, `ReturnControlToParent`, `StartOrResumeSideAgent`, `RequestHumanActionFromSideAgent`, `ReadCollaborativeStatus`, `ResumeOrphanedHandoff`, `ResumeReturnedParent`, `ReconcileStaleHandoffs`

### Ports（例）

`HandoffRepository`, `LeaseRepository`, `CheckpointRepository`, `CommandCandidateStore`, `HumanShellLauncher`, `EnvironmentObserver`, `TokenGenerator`, `HostIdentityProvider`

### Adapters

- `ai`: CLI、`CollaborativeShellExecPolicy`（`aibe-client` 承認コールバック拡張）
- `aish`: `PtyShell` + handoff wrapper + prompt integration + control FIFO 拡張
- `aibe`: tool result 拡張、side agent turn（既存 loop 再利用）
- filesystem: handoff store

**禁止**: agent loop / CLI から DB 具象への直接依存。

## 14. 0053（提案コマンド recall）との統合

handoff 由来候補は `SuggestedCommandRecallStore` を **拡張** し、source メタデータ付き queue を handoff スコープで保持する。

- Alt+. 挿入は **command 文字列のみ**
- 0053 の assistant fenced block 抽出とは queue を分離し、recall UI は統合表示可
- `ai recall` は handoff active 時に handoff 候補を優先（Phase 2）

## 15. 設定（初版候補）

```toml
[collaborative]
enabled = true
prompt_template = "..."
heartbeat_interval_secs = 30
lease_timeout_secs = 120
recent_parent_turns = 6
recent_side_turns = 8
summary_token_limit = 4096
```

`collaborative.shell` は未指定時 `aish` 設定の shell を使用。

## 16. Phase 分割（実装順）

| Phase | 内容 | ゲート |
|-------|------|--------|
| 1 | Domain、永続化、lease、checkpoint、candidate 拡張、unit test | Phase 1 AC すべて `pending = false` |
| 2 | `--collaborative`、親 `shell_exec` handoff、child goal、human shell、recall、親再観測 | 同上 |
| 3 | side agent、env 検証、`--standalone`、`ai status`、人間待ち再開 | 同上 |
| 4 | heartbeat、ORPHANED、`ai resume`、token rotation、fault injection | 同上 |
| 5 | prompt 表示、signal、log redaction、docs / manual | 同上 |

各 Phase の実装指示書: `docs/tasks/0055_collaborative-human-handoff-phaseN-implementation-spec.md`

## 17. 受け入れ条件（初版完了）

1. `ai --collaborative` で親エージェントを開始できる
2. 親の `shell_exec` が自動実行されず human shell へ変換される
3. 親提示コマンドを Alt+. / Alt+, から取り込める
4. コマンドは自動実行されない
5. 人間が自由に編集・実行・非実行を選べる
6. Ctrl+D / `exit` / `exit N` で親へ制御が戻る
7. shell 終了コードを要求コマンド結果として扱わない
8. 親が再観測してから作業を継続する
9. human shell 内 `ai` が同一 side conversation を継続する
10. side agent が親タスク文脈を参照できる
11. side agent 人間待ちで新 human shell を作らない
12. `ai` / `ai <補足>` で side agent を再開できる
13. `ai --standalone` で独立セッションを開始できる
14. `ai status` がローカル状態のみ表示する
15. human shell プロンプトに協調状態が表示される
16. 異常終了時に親を自動再開しない
17. 異常 handoff を `ai resume` で復旧できる
18. 復旧時に古い token が失効する
19. 同じ handoff を二重再開できない
20. provider tool-call ID なしで親 run を意味的に再開できる
21. 不確定ツールを自動再実行しない
22. 通常 `ai` の既存挙動を壊さない
23. 通常モードで既存 `shell_exec` 動作が変わらない
24. Contextual Memory / replay / candidate 機構を再利用する
25. domain 層が PTY / DB / CLI 具象へ直接依存しない

## 18. 実装上の禁止事項

§31（ユーザー仕様）と同一。要約:

- yes/no 承認への置換、候補自動実行、Ctrl+D を成功扱い、exit code をコマンド結果と混同
- side agent の一問一答化、handoff 毎の新 conversation、side からの入れ子 human shell
- 無効 handoff での黙ってフォールバック、ORPHANED からの親自動再開、`RUNNING` ツールの自動再実行
- コマンドの `&&` / パイプ分解、token 露出、handoff ID のみでの認証、lease なし復旧

## 19. 設計原則

```text
制御が返った ≠ コマンドを実行した ≠ コマンドが成功した ≠ 依頼が達成された
```

人間は作業主体。LLM は再観測で判断する。

## 20. 意図的簡略化（元仕様との差分）

| 項目 | 元仕様 | 初版の判断 |
|------|--------|------------|
| `shell_exec.environment` | 候補先頭へ `KEY=value` 付与 | 現行 schema 無し。**初版対象外**。protocol 拡張は別タスク |
| 同一 turn 複数 `shell_exec` の cwd 集約 | 同一 cwd は 1 handoff にまとめ可 | **1 ShellExec = 1 handoff 直列**で実装可（同時 human shell 禁止は維持） |
| コマンド表現 | シェル 1 行文字列 | リポジトリ実装に合わせ `command` + `args[]` から `candidate_command` を組み立て |
| human shell プロンプト表示 | 初版必須 | 機能は Phase 5 で完了。**Phase 2–4 は control channel のみ**（プロンプト hook 未注入でも可） |
| `ai status` | 協調専用表示 | **既存 `ai status` を拡張**（§5.1） |

## 21. Command candidate（§8 詳細）

### 21.1 データモデル

```text
CommandCandidate {
    id
    command              # プロンプト挿入用。出所メタは含めない
    description?
    source               # PARENT_AGENT | SIDE_AGENT | HISTORY | MANUAL
    source_run_id?
    target_handoff_id
    created_at
}
```

### 21.2 挿入と実行

- Alt+. / Alt+, は `command` のみ `READLINE_LINE` / `BUFFER` へ挿入（0053 機構を拡張）
- 候補挿入・shell log 上の一致は **依頼達成の根拠にしない**（§8.6）

## 22. Child goal（§10 詳細）

handoff 作成時（human shell 開始前）に必ず作成。side conversation は遅延可。

保存内容（最低限）:

- 親タスク目的、作業段階、human shell 起動理由
- 親 `shell_exec` 要求、人間への依頼操作、想定完了条件
- `parent_goal_id`, `handoff_id`

終了: human shell 正常返却時のみ。`close_reason = CONTROL_RETURNED`, `achievement = UNKNOWN`。**ORPHANED では閉じない**。

実装: `Handoff` メタ + Contextual Memory `goal` 子エントリ（`ai work` active work と連携）。

## 23. Side agent 文脈と control（§11–§12 詳細）

### 23.1 継承文脈（side turn 注入）

親タスク最終目的、作業段階、親計画、未解決事項、起動理由、依頼操作、完了条件、親 conversation 要約・直近ターン、Contextual Memory、cwd、shell log / replay 参照、child goal、handoff ID。

完全履歴は保存するが LLM へ毎回全投入しない（§25）。

### 23.2 `request_human_action`

side 専用 control outcome（新シェルは起動しない）:

```text
request_human_action {
    instruction
    reason
    command_candidates[]
    expected_completion
}
```

実行時: 永続化 → candidate 登録 → side run 停止 → `SIDE_AGENT_WAITING_FOR_HUMAN` → human shell へ制御返却。

### 23.3 `HumanControlReturned`

```text
HumanControlReturned {
    pending_request
    shell_log_delta
    current_cwd
    current_observation
    user_note?
}
```

`ai`（補足なし）再開: 空ユーザーメッセージではなく上記イベント。`ai <補足>` は `user_note` に設定。

### 23.4 接続拒否・待機

| 状態 | human shell 内 `ai` |
|------|---------------------|
| `HUMAN_ACTIVE` | 裸 `ai` は通常入力 UI → 確定後 side 作成/再利用 |
| `SIDE_AGENT_WAITING_FOR_HUMAN` | 裸 `ai` は入力 UI なしで side 再開 |
| `SIDE_AGENT_RUNNING` | 新 run 開始しない。「already running」+ `ai status` 案内 |
| `ORPHANED` | 直接 side 接続せず `ai resume` を要求 |
| `RETURNED` / `COMPLETED` / `CANCELLED` | 古いコンテキストとして拒否 |

## 24. 永続化エンティティ（§15–§16 詳細）

### 24.1 Handoff（主要フィールド）

`id`, `schema_version`, `parent_task_id`, `parent_conversation_id`, `parent_run_id`, `parent_goal_id`, `child_goal_id`, `side_conversation_id?`, `state`, `initial_cwd`, `final_shell_cwd?`, `parent_request_summary`, `requested_shell_execs[]`, `pending_human_request?`, `conversation_snapshot_ref`, `conversation_summary`, `checkpoint_ref`, `before_observation_ref`, `after_observation_ref?`, `shell_log_start`, `shell_log_end?`, `shell_generation`, `return_reason?`, `human_shell_exit_code?`, `resume_error?`, タイムスタンプ群。

### 24.2 HandoffLease

`handoff_id`, `owner_client_id`, `owner_process_id`, `owner_tty`, `owner_host`, `owner_uid`, `lease_acquired_at`, `lease_expires_at`, `last_heartbeat_at`。取得・更新・解放は原子的。`lease.json` は親が human shell lifetime 全体（Ctrl+D後の親再開完了まで）で保持し、PTY heartbeat だけが更新する。side agent は lifetime lease を変更せず、1 side run ごとの `side-run-lock.json` を原子的に取得・解放する。

### 24.3 Checkpoint（必須）

`parent_task_id`, `parent_conversation_id`, `parent_run_id`, `pending_shell_exec`, `parent_goal`, `child_goal`, `conversation_snapshot`, `conversation_summary`, `cwd`, `environment_metadata`, `handoff_id`, `side_conversation_id?`, `command_candidates`, `shell_log_start`, `control_state`, `provider_metadata`（診断用・復旧必須にしない）。`environment_metadata` には PTY 返却後の `shell_session_id`, `shell_session_dir`, `shell_log_start`, `shell_log_end` と共有 suggestion cache path を保存する。

### 24.4 Tool execution（handoff 関連）

`REQUESTED` / `RUNNING` / `COMPLETED` / `FAILED` / `CANCELLED` / `UNKNOWN`。親 `shell_exec` は AISH 処理完了時点で `COMPLETED` 可だが `execution_outcome = HUMAN_CONTROL_RETURNED`, `requested_command_completion = UNKNOWN`。

## 25. 会話・要約（§22 詳細）

### 25.1 完全保存

親 / side conversation、control events、tool requests/results、candidates、human request、handoff 遷移、shell log 参照、recovery events。

### 25.2 LLM 投入

`conversation 要約 + 直近ターン + 必要時参照`。大きな tool 出力は replay / artifact 参照。

### 25.3 要約更新タイミング

human shell 開始前、side run 終了後、side 人間待ち遷移時、human shell 終了時、ORPHANED 復旧時、親 run 復旧時。要約失敗で完全履歴を失わない。

## 26. 親ツール結果（§17 詳細）

`HumanHandoffResult` は最低限: `handoff_id`, `execution_outcome`, `return_reason`, `human_shell_exit_code`, `requested_command`, `requested_command_completion`, `final_shell_cwd`, `shell_log_range`, `child_goal_summary`, `side_conversation_summary`, `before_observation_ref`, `after_observation_ref`, `uncertain_tool_executions[]`。

LLM 向け本文（要旨）:

```text
Control returned from the human shell.
The requested command was not automatically executed by AISH.
Do not infer success from the shell exit code.
Inspect the current environment and verify the completion condition.
```

## 27. 親再観測（§18 詳細）

返却後・復旧親 run 開始前に利用可能な範囲で: 元 cwd 存在、final cwd、Git HEAD / branch / `git status`、変更ファイル、shell log 追加範囲、child goal 状態、side 要約、不確定ツール、handoff 前後差分。Git 外は git 省略。全ファイル走査は必須にしない。

状態遷移は Ctrl+D / `exit` の正常返却で `RETURNED` までとし、親 run の開始時に `RESUMING_PARENT`、成功後だけ `COMPLETED` とする。親 run が失敗・cancelされた場合は `RETURNED` に戻して `resume_error` を残す。`ORPHANED` の `ai resume` は新 human shell の正常返却後、同じ invocation 内で親 run まで自動再開する。

## 28. 復旧で復元しないもの（§20.3）

消失シェルプロセス、ジョブテーブル、foreground プロセス、任意 `export`、shell 内 alias/関数、SSH、TUI 状態。復旧 shell 開始時に警告表示可。

## 29. セキュリティ・監査（§23, §27 詳細）

信頼境界: 同一ホスト・同一 OS ユーザー・同一 AISH データ領域。token は status / LLM / replay / shell log / 通常ログへ出力しない。`sudo -E ai` は UID 検証で拒否。host ID / generation / token hash で誤接続防止。

監査イベント（秘密・command 全文を無条件に含めない）:

`handoff_created`, `human_shell_started`, `human_shell_returned`, `human_shell_orphaned`, `side_conversation_created`, `side_agent_started`, `side_agent_waiting_for_human`, `side_agent_returned`, `candidate_registered`, `handoff_resumed`, `parent_resume_started`, `parent_resume_completed`, `parent_resume_failed`, `lease_acquired`, `lease_lost`, `stale_token_rejected`

## 30. `shell_exec` インターセプト方式（確定）

協調モードでは **yes/no 承認 UI に置き換えない**。`ai` 側 `CollaborativeShellExecPolicy`（execution policy adapter）が親 `shell_exec` を検知し、handoff フローへ入る。通常モードの `shell_exec` 承認 UX（0036）は変更しない。

`aibe` の `ShellExecTool` 本体は原則変更せず、client が handoff 結果を synthetic tool result として組み立てる。side / standalone は通常経路。

## 31. Phase と human shell 環境変数

| タイミング | 責務 |
|------------|------|
| Phase 2 | human shell **起動時**に `AISH_CONTROL_MODE` 等を子プロセスへ設定 |
| Phase 3 | human shell 内 `ai` の **検証・side 接続** |

## 32. 受け入れ条件と AC 対応（§29 → spec-acceptance）

| §29 | 内容 | AC id（代表） |
|-----|------|---------------|
| 1 | `--collaborative` 開始 | `collaborative_flag_starts_parent` |
| 2 | 親 handoff 変換 | `parent_shell_exec_handoff` |
| 3–4 | Alt+. / 非自動実行 | `candidate_in_recall_queue`, `candidate_not_treated_as_executed` |
| 5 | 編集・非実行自由 | `candidate_inserts_command_only`（統合テスト） |
| 6–7 | Ctrl+D / exit / exit code | `human_shell_ctrl_d_returns`, `human_shell_exit_returns`, `tool_result_not_success` |
| 8 | 親再観測 | `parent_reobserves` |
| 9–12 | side 継続・文脈・待ち・再開 | `side_conversation_continues`, `side_inherits_parent_context`, `request_human_action`, `bare_ai_resumes_side`, `ai_note_becomes_user_note` |
| 13 | `--standalone` | `standalone_ignores_handoff` |
| 14 | `ai status` | `ai_status_no_llm`, `status_shows_handoff_fields`, `existing_ai_status_regression` |
| 15 | プロンプト表示 | `prompt_shows_collaborative_state` |
| 16–21 | 異常・復旧・token・二重・意味的再開・UNKNOWN | Phase 4 AC 群 |
| 22–23 | 通常 ai / shell_exec 回帰 | `normal_ai_unchanged_regression`, `normal_shell_exec_unchanged` |
| 24 | memory / replay / candidate 再利用 | `side_inherits_parent_context`, `candidate_in_recall_queue` |
| 25 | domain 境界 | アーキテクチャ検査 + hexagonal（Phase 1 domain 単体） |
