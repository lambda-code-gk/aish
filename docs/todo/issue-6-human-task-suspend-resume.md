## 概要

Collaborative Modeで実行中のHuman Taskを途中で中断し、`ai`プロセス終了後も、後から同じHuman Taskを再開できるようにする。

Human Taskを再開して作業が完了した後は、その結果とEvidenceを使って**新しいCollaborative Modeターン**を開始し、元の仕事を意味的に継続する。

```text
Human Task開始
  ↓
Human Shellで作業
  ├─ Ctrl+D / exit
  │    → Human Task完了
  │    → 親エージェント継続
  │
  └─ human-task suspend
       → Human Task中断・永続化
       → aiプロセス終了
       → ai human-task resume
       → 新しいHuman Shellで再開
       → 完了後に新しい協調ターンで元の仕事を継続
```

---

## mainブランチの現状

PR #5までmainへ統合済みであり、現在は以下が存在する。

- 正式導線 `ai collab "..."`
- 互換導線 `--collaborative`
- `shell_exec`から独立した`human_task`ツール
- Human Shell
- Human Task briefing
- Human Task Evidence
- 構造化された`HumanTaskResult`
- `Done`でも`verified=false`として親エージェントが検証する契約

現在のHuman Taskは同期処理である。

1. aibeがUnix socket経由でHuman Taskをaiへ要求
2. aiがHuman Shellを起動
3. aiはHuman Shell終了を待機
4. 結果を同じ接続上でaibeへ返却
5. aibeのエージェントループが続行

したがって、本Issueでは同一のLLM API呼び出し、Unix socket接続、agent loopスタック、PTYプロセスを保存・復元しない。

---

## 「再開」の定義

### 再開するもの

- `HumanTaskRequest`
- Human Task briefing
- 最後に作業していたディレクトリ
- 中断までに収集したEvidence / Observation
- 元のユーザー要求
- 元のAI session ID
- 元のconversation ID
- Human Task完了後の継続に必要な親コンテキスト

### 再開しないもの

- 同一のLLM APIリクエスト
- 同一のprovider stream
- 同一のUnix socket接続
- 同一のaibe agent loop
- 同一のai / aishプロセス
- 同一のPTY

Human Task完了後は、保存した結果を入力として**新しいCollaborative Modeターンを開始する**。

---

## 対象範囲

### 対象

明示的な`human_task`ツールから開始されたHuman Taskのみ。

```text
ai collab
  └─ human_task
       └─ Human Shell
            └─ 中断・再開可能
```

### 対象外

`shell_exec`をHuman Shellへ変換する旧`--collaborative` handoff経路には中断・再開を追加しない。

旧経路は従来どおり同期的に完了させる。

---

## ユーザー操作

### 通常完了

現在の操作を維持する。

```text
Ctrl+D
```

または、

```text
exit
```

意味：Human Taskの作業を完了し、結果を親エージェントへ返す。

### 中断

Human Shell内で次を実行する。

```text
human-task suspend
```

任意で理由を付けられる。

```text
human-task suspend "認証情報を確認してから続ける"
```

表示例：

```text
Human Task suspended.

Task:
  ht-20260714-7f31c2

Reason:
  認証情報を確認してから続ける

Resume:
  ai human-task resume
```

`human-task`はHuman Shellの一時rcfileへ注入するshell functionとし、通常のユーザーシェルへ恒久インストールしない。

### 状態確認

```text
ai human-task status
```

表示例：

```text
Human Task: ht-20260714-7f31c2
Status: suspended
Objective: ステージング環境のデプロイ状態を確認する
Suspended at: 2026-07-14 22:18:30 +09:00
Reason: 認証情報を確認してから続ける
Working directory: /home/user/project
Resume: ai human-task resume
```

中断中Taskがない場合は`No suspended Human Task.`を表示し、終了コードは0とする。

### 再開

```text
ai human-task resume
```

将来拡張に備え、Task ID指定も受け付ける。

```text
ai human-task resume ht-20260714-7f31c2
```

本段階では同時に一つのactive Human Taskのみ許可する。

Human Shellは保存された最後の作業ディレクトリから、新しいプロセス・新しいshell sessionとして起動する。

### 取消

```text
ai human-task cancel
```

確認後、`Cancelled`相当の結果を生成して新しい協調ターンへ通知する。

```text
ai human-task cancel --yes
```

で確認を省略できる。

---

## 状態モデル

Human Taskのワークフロー状態と最終結果を分離する。

```rust
pub enum HumanTaskWorkflowState {
    Running,
    Suspended,
    ResultPending,
    Continuing,
    Finished,
}
```

- `Running`: Human Shell起動中
- `Suspended`: Human Shellは終了済みだがTask未完了
- `ResultPending`: 最終結果確定済み、継続ターン未開始
- `Continuing`: 継続ターン開始済み
- `Finished`: 親エージェントへの引き渡し完了

既存の`HandoffExecutionOutcome`へ追加する。

```rust
pub enum HandoffExecutionOutcome {
    HumanControlReturned,
    Done,
    Blocked,
    Cancelled,
    Suspended,
}
```

`Suspended`は最終結果ではなく非終端の制御結果とする。

```text
Running → Suspended → Running → ... → ResultPending → Continuing → Finished
```

複数回の中断・再開を許可する。

---

## 永続化モデル

```rust
pub struct HumanTaskCheckpoint {
    pub version: u8,
    pub task_id: String,
    pub state: HumanTaskWorkflowState,

    pub task: HumanTaskRequest,
    pub parent: HumanTaskParentContext,

    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub suspended_at_ms: Option<u64>,
    pub suspend_reason: Option<String>,

    pub current_cwd: PathBuf,
    pub segments: Vec<HumanShellSegment>,

    pub final_result: Option<HumanTaskResult>,
    pub continuation: HumanTaskContinuationState,
}
```

### 親コンテキスト

```rust
pub struct HumanTaskParentContext {
    pub ai_session_id: String,
    pub conversation_id: Option<String>,
    pub original_turn_id: String,
    pub original_user_message: String,
    pub original_cwd: PathBuf,
    pub llm_profile: Option<String>,
}
```

`ClientRequest`全体をそのまま保存してはならない。

保存禁止：

- 環境変数
- Unix socket情報
- provider内部状態
- APIキー
- callback
- cancel flag
- terminal設定
- PTY fd

### Human Shell区間

再開のたびに新しいHuman Shellとshell sessionを生成する。

```rust
pub struct HumanShellSegment {
    pub index: u32,
    pub shell_session_id: String,
    pub started_at_ms: u64,
    pub ended_at_ms: Option<u64>,
    pub initial_cwd: PathBuf,
    pub final_cwd: Option<PathBuf>,
    pub shell_log_range: Option<ShellLogRange>,
    pub observation: Option<PostHandoffObservation>,
    pub end_reason: Option<HumanShellSegmentEnd>,
}
```

```rust
pub enum HumanShellSegmentEnd {
    Suspended,
    Done,
    Cancelled,
    Interrupted,
}
```

---

## Evidence統合

最終結果作成時に全segmentのEvidenceを統合する。

- `commands`: segment順に連結し、Human Task全体でindexを0から振り直す
- `truncated`: 一つでも`true`なら最終結果も`true`
- `shell_log_tail`: 最後のsegmentを優先
- git状態: 最終segment終了時の状態
- `observation_errors`: 重複排除して統合

raw shell logをCheckpointディレクトリへ複製する必要はない。中断時に必要なEvidence / Observationを取り込んだ後、既存runtime directoryは削除してよい。

---

## 保存場所とセキュリティ

現在のhandoff runtime directoryは一時領域のままとし、永続Checkpointには使用しない。

推奨保存先：

```text
<AiConfig.history_dir>/human-tasks/
```

例：

```text
human-tasks/
└── ht-20260714-7f31c2/
    ├── checkpoint.json
    └── lock
```

要件：

- ディレクトリ `0700`
- Checkpointファイル `0600`
- symlink拒否
- 現在ユーザー以外の所有者を拒否
- `O_NOFOLLOW`
- 一時ファイルへ書いてatomic rename
- 書き込み後`fsync`
- JSON破損時に自動削除しない

既存のHuman Shell result file用secure write処理を共通化して再利用する。

---

## 所有権と多重起動防止

同一ユーザーにつき、次の状態のHuman Taskは同時に一つだけ許可する。

- Running
- Suspended
- ResultPending
- Continuing

既存active Taskがある場合、新しい`human_task`は開始せず次を返す。

```text
code: human_task_already_active
message: another Human Task is already active
```

Checkpointディレクトリ内の`lock`へ排他的ファイルロックを取得する。

PIDだけで所有判定しない。

Checkpointが`Running`だが排他ロックを取得できる場合は、以前の所有プロセスが異常終了したものとして自動的に次へ遷移する。

```text
Running → Suspended
reason = unexpected_process_termination
```

---

## Human Shellプロトコル変更

現在の`normal_return: bool`では完了と中断を区別できないため、次へ変更する。

```rust
pub enum HumanShellOutcome {
    Done,
    Suspended,
}
```

```rust
pub struct HumanShellResult {
    pub outcome: HumanShellOutcome,
    pub exit_code: Option<i32>,
    pub final_cwd: PathBuf,
    pub shell_session_id: String,
    pub shell_session_dir: PathBuf,
    pub shell_log_start: u64,
    pub shell_log_end: u64,
    pub suspend_reason: Option<String>,
}
```

旧形式を読む必要がある場合は、`normal_return=true`を`Done`へ変換する。

control FIFOへ追加するイベント：

```json
{
  "event": "human_suspend",
  "reason": "認証情報を確認してから続ける",
  "cwd": "/home/user/project"
}
```

`human_suspend`受信後にEXIT trapの`human_return`が届いても、最初に受信した終端イベントを採用し、`Suspended`を上書きしない。

---

## Coordinator

既存`ExecuteHumanTask`を肥大化させず、上位にCoordinatorを追加する。

```rust
pub struct HumanTaskCoordinator<'a> {
    store: &'a dyn HumanTaskStore,
    shell_launcher: &'a dyn HumanShellLauncher,
    environment_observer: &'a dyn EnvironmentObserver,
}
```

責務：

- Task ID生成
- Checkpoint作成
- 排他ロック
- Human Shell起動
- segment追加
- 中断状態保存
- Evidence統合
- 最終結果保存
- 継続ターン起動

`ExecuteHumanTask`は単一Human Shell区間の実行に限定する。

初回開始順序：

```text
1. active Taskがないことを確認
2. Task ID生成
3. CheckpointをRunningとしてatomic保存
4. lock取得
5. Human Shell起動
6. 終了結果を取得
7. Observation / Evidence収集
8. segment追加
9. outcomeに応じて状態更新
10. Checkpointをatomic保存
11. aibeへ結果返却
```

Checkpoint作成前にHuman Shellを起動してはならない。

---

## aibeの中断制御

Human Taskが`Suspended`を返した場合、aibeは次のLLM tool roundへ進んではならない。

system instructionだけに依存せず、内部制御として実装する。

概念例：

```rust
pub enum ToolExecutionDisposition {
    Continue,
    SuspendTurn { task_id: String },
}
```

```rust
RoundOutcome::Suspended {
    task_id,
    executed,
}
```

`AgentTurnService`は追加LLM呼び出しをせず、固定応答でターンを正常終了する。

```text
Human Task suspended.

Resume:
  ai human-task resume
```

中断はエラーではない。

---

## 再開処理

```text
1. suspended Taskを検索
2. Task ID指定時は一致確認
3. Checkpoint検証
4. lock取得
5. stateをRunningへ変更
6. current_cwdの存在確認
7. 新しいsegment作成
8. 保存したbriefing表示
9. 新しいHuman Shellを起動
10. 終了後にsegment保存
11. Suspendedなら再度終了
12. DoneならResultPendingへ移行
13. 継続ターンを開始
```

以前のcwdが存在しない場合はHuman Shellを起動せず、Checkpointを`Suspended`のまま維持する。

本段階では`--cwd`変更機能は追加しない。

---

## 親エージェントの継続

Human Task完了後、新しいCollaborative Modeターンを開始する。

継続メッセージ概念：

```text
[Collaborative Mode continuation]

A previous agent turn delegated a Human Task and then stopped.

Original user request:
<original_user_message>

Human Task:
<objective / reason / instructions / completion criteria>

Human Task result:
<serialized HumanTaskResult>

Important:
- The Human Task result is unverified.
- Re-observe the environment where possible.
- Verify the completion criteria before claiming completion.
- Continue the original user request from this point.
```

引き継ぐもの：

- `ai_session_id`
- `conversation_id`
- `cwd`
- `llm_profile`
- Collaborative Mode
- 元のユーザー要求

引き継がないもの：

- 元のtool round数
- timeout残時間
- cancel flag
- streaming状態
- shell approval session cache
- Unix socket connection

継続開始に失敗した場合、Checkpointを削除せず`ResultPending`として残す。

`ResultPending`で`ai human-task resume`を実行した場合はHuman Shellを再実行せず、保存済み結果を使って継続だけ再試行する。

---

## 重複実行防止

- 継続ターン用`continuation_turn_id`をCheckpointへ保存
- 再試行時も同じIDを使用
- aibeは同じturn IDの二重実行を拒否
- 少なくとも同一aibeプロセス内で重複を防止

本Issueではaibe再起動をまたぐ完全なexactly-once保証は対象外。

保証対象：

- Human Shellの二重起動防止
- 最終結果の二重追加防止
- Evidenceの重複統合防止
- 同一プロセス内の継続ターン二重開始防止

---

## CLI追加

```text
ai human-task status
ai human-task resume [TASK_ID]
ai human-task cancel [TASK_ID] [--yes]
```

Clap概念：

```rust
pub enum AiCommand {
    HumanTask {
        #[command(subcommand)]
        command: HumanTaskCommand,
    },
}

pub enum HumanTaskCommand {
    Status,
    Resume { task_id: Option<String> },
    Cancel {
        task_id: Option<String>,
        #[arg(long)]
        yes: bool,
    },
}
```

`ai collab "..."`は変更しない。

---

## 想定モジュール

### ai

```text
ai/src/domain/human_task_checkpoint.rs
ai/src/application/human_task_coordinator.rs
ai/src/ports/outbound/human_task_store.rs
ai/src/adapters/outbound/file_human_task_store.rs
ai/src/application/human_task_resume.rs
```

既存`ai/src/application/execute_human_task.rs`は単一区間実行として維持する。

### aish

変更候補：

```text
aish/src/human_shell.rs
aish/src/adapters/outbound/shell_completion.rs
aish/src/adapters/inbound/clap_cli.rs
aish/src/main.rs
```

`aish`は永続Checkpointを直接読み書きしない。

### aibe-protocol

```text
aibe-protocol/src/collaborative_handoff.rs
```

追加候補：

- `HandoffExecutionOutcome::Suspended`
- `HumanTaskResult::task_id`
- `HumanShellOutcome`

### aibe

変更候補：

```text
aibe/src/adapters/outbound/tools/human_task.rs
aibe/src/application/tool_round.rs
aibe/src/application/agent_turn.rs
```

---

## エラーコード

```text
human_task_already_active
human_task_checkpoint_unavailable
human_task_checkpoint_invalid
human_task_checkpoint_version_unsupported
human_task_checkpoint_permission_denied
human_task_not_found
human_task_not_suspended
human_task_lock_unavailable
human_task_resume_cwd_unavailable
human_task_resume_launch_failed
human_task_continuation_failed
```

Checkpoint破損時は自動削除しない。

---

## 互換性要件

1. `ai collab "..."`が従来どおり動く
2. `--collaborative`が互換導線として動く
3. 中断しない場合は従来と同じ操作感
4. Ctrl+D / `exit`は引き続き`Done`
5. `Done`の`verified=false`を維持
6. 開始時requestをクライアント側で変更しない
7. 既存briefingを維持
8. 既存Evidence収集を維持
9. shell_exec handoffの結果形式を変更しない
10. bash / zshを維持

---

## 非対象

- 複数Human Taskの並列実行
- 複数ユーザー間共有
- 別PCからの再開
- 実行中PTYへの再attach
- tmux / screen統合
- Human Taskの入れ子
- Task履歴UI
- 自動期限切れ
- lease更新
- 複数クライアント間ownership移譲
- 同一LLM API呼び出しの復元
- aibe再起動をまたぐ完全なexactly-once保証
- Human Shell内AI補佐（第6段階）

---

## 受け入れ条件

### 通常完了

- `ai collab`から`human_task`開始
- Ctrl+Dで`status=done`
- `verified=false`
- 親エージェントが従来どおり継続
- 中断用Checkpointが残らない

### 明示中断

- `human-task suspend`でHuman Shell終了
- Checkpointが`suspended`
- aiコマンド終了
- aibeは追加LLM呼び出しをしない
- 再開コマンドを表示

### 理由付き中断

- 理由がCheckpointへ保存される
- `status`で確認可能
- 再開briefingに表示

### 再開

- 同じ`HumanTaskRequest`を使用
- 最後のcwdから開始
- 新しいshell sessionを作成
- 過去Evidenceを保持

### 複数回中断

```text
Running → Suspended → Running → Suspended → Running → Done
```

が成立し、全segmentのEvidenceが順序どおり統合される。

### 再開後完了

- `status=done`
- `verified=false`
- 全segmentのEvidence統合
- 新しいCollaborative Modeターン開始
- 元のユーザー要求を継続

### ai異常終了

- `Running` Checkpointと解放済みlockを検出
- `Suspended`へ復旧
- `reason=unexpected_process_termination`
- 再開可能

### active Task衝突

- `human_task_already_active`
- 既存Checkpointを変更しない
- 新しいHuman Shellを起動しない

### 継続失敗

- `ResultPending`として残る
- Human Shellを再実行しない
- `resume`で継続だけ再試行

### セキュリティ

- directory `0700`
- file `0600`
- symlink拒否
- 他ユーザー所有を拒否
- JSON破損時に削除しない
- 環境変数・raw API requestを保存しない

### 旧shell_exec経路

- suspendを提供しない
- 従来どおり同期完了
- `HumanHandoffResult`形式を変更しない

---

## 推奨実装順序

### 0063-A: Checkpoint基盤

- domain / store
- secure file store
- atomic保存
- lock
- status CLI
- 単体テスト

### 0063-B: Human Shell suspend

- `human_suspend` event
- `HumanShellOutcome`
- `human-task suspend`
- `HandoffExecutionOutcome::Suspended`
- aibe agent loop停止

### 0063-C: Human Shell resume

- `ai human-task resume`
- briefing / cwd復元
- 新segment
- 複数回中断
- Evidence統合

### 0063-D: Agent continuation

- Parent context保存
- 継続メッセージ生成
- 新しいCollaborative Modeターン
- `ResultPending`再試行
- continuation turn ID

### 0063-E: Recovery hardening

- stale lock検出
- ai異常終了復旧
- corrupt checkpoint処理
- permissions
- session pruning競合試験
- bash / zsh受け入れ試験

各段階を独立してmainへマージ可能な状態に保つ。

---

## 完了イメージ

```text
$ ai collab "ステージング環境を調査して問題を解決して"

AIがhuman_taskを要求

Human Shell:
$ kubectl get pods
$ human-task suspend "VPN接続後に続ける"

Human Task suspended.
Resume:
  ai human-task resume

$ ai human-task resume

Human Shell:
$ kubectl get pods
$ kubectl logs ...
$ exit

Human Task completed.
Continuing Collaborative Mode...

AI:
  Evidenceを確認し、必要な再観測を行い、
  元のユーザー要求の処理を継続する
```

Checkpointは`ai`が所有し、`aish`は一回のHuman Shell区間だけを担当する。既存のPorts & Adapters構造を維持し、第6段階のHuman Shell内AI補佐へ接続できる構造にする。
