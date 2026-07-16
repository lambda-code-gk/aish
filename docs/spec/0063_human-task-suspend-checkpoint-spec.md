# 0063 Collaborative Mode Human Task Suspend Checkpoint 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **関連**: [`0063_human-task-suspend-resume-overview.md`](0063_human-task-suspend-resume-overview.md)、[`0062_collab-mode-human-task-tool-spec.md`](0062_collab-mode-human-task-tool-spec.md)、[`0061_collab-mode-human-task-evidence-spec.md`](0061_collab-mode-human-task-evidence-spec.md)、[`0060_collab-mode-human-task-briefing-spec.md`](0060_collab-mode-human-task-briefing-spec.md)、[`0055_minimal-human-handoff-spec.md`](0055_minimal-human-handoff-spec.md)、[`docs/feature-development-policy.md`](../feature-development-policy.md)

## 0. Core outcome

ユーザーが明示 `human_task` の Human Shell を `human-task suspend` で中断し、`ai` 終了後も安全に保存されたtaskを `ai human-task status` で確認し、未実装resumeに依存せずlocal cancelで復旧できる。

## 1. Minimum vertical slice

```text
ai collab "依頼内容"
→ LLM が既存 human_task を呼ぶ
→ ai が version 1 checkpoint を Running として安全に保存
→ 既存 briefing と Human Shell を起動
→ ユーザーが human-task suspend "理由" を実行
→ aish が最初の終端eventを Suspended として返す
→ ai が bounded Evidence / Observation をsegmentへ取り込み Suspendedをatomic保存
→ aibeが追加LLM呼び出しなしでturnを正常終了
→ aiコマンド終了
→ 別の ai human-task status がtask ID・目的・理由・cwdを表示して成功終了
→ ai human-task cancel --yes がSuspended checkpointを削除し、新しいHuman Taskを開始可能にする
```

このspecは Issue の論理区分0063-Aに、Vertical Slice成立に必要な0063-Bの最小経路だけを加える。`resume`、複数segment統合、継続agent turnは含めない。

### 1.1 Checkpoint domain

Checkpoint は `ai` domain の一つの versioned aggregate とする。`ClientRequest` 全体やruntime objectを保存しない。

```text
HumanTaskCheckpointV1
  version: 1
  task_id: HumanTaskId
  state: HumanTaskWorkflowState
  task: HumanTaskRequest
  parent: HumanTaskParentContext
  created_at_ms: u64
  updated_at_ms: u64
  suspended_at_ms: Option<u64>
  suspend_reason: Option<String>
  current_cwd: PathBuf
  segments: Vec<HumanShellSegment>
  final_result: Option<HumanTaskResult>
  continuation: HumanTaskContinuationState
```

このspecで固定する境界値とwire表現は次のとおりとする。

- task IDは正規表現 `^ht-[0-9]{8}-[0-9a-f]{6}$` に一致するASCII文字列
- suspend reasonはUTF-8で最大4096 bytes。NULを含むUnicode control characterをすべて拒否
- encoded checkpointは最大1 MiB。write前とdecode前の両方で上限を検査
- timestampはUnix epochからのmillisecondsをUTC値として保存し、CLI表示時だけlocal timezoneへ変換

`HumanShellSegment` は次のfieldを持つ。0063ではindex `0` の一件だけを保存し、`end_reason` は `Suspended` だけを永続checkpointへ書く。

```text
HumanShellSegment
  index: u32
  shell_session_id: String
  started_at_ms: u64
  ended_at_ms: u64
  initial_cwd: PathBuf
  final_cwd: PathBuf
  shell_log_range: ShellLogRange
  observation: PostHandoffObservation
  end_reason: HumanShellSegmentEnd

HumanShellSegmentEnd
  Suspended
  Done        # 後続spec用予約語彙
  Cancelled   # 後続spec用予約語彙
  Interrupted # 後続spec用予約語彙
```

`HumanTaskContinuationState` はversion 1 envelope内の `{ continuation_turn_id: Option<String> }` とし、0063では常に`None`とする。`final_result`も常に`None`とする。0063が受理するstate invariantは次に固定する。

- 全stateで `created_at_ms <= updated_at_ms`、task ID・task・parent・cwd・encoded sizeが各validationを満たす
- `Running`: `suspended_at_ms` / `suspend_reason` / `final_result` / continuation turn IDがなく、segmentは空
- `Suspended`: `suspended_at_ms`があり、index 0の完全な`Suspended` segmentが一件あり、`current_cwd == segment.final_cwd`。reasonは未指定またはvalidation済みの文字列
- `ResultPending` / `Continuing` / `Finished`: 後続spec用の予約stateであり、0063のreaderは状態不変条件違反として拒否

これらに反するcheckpointを状態不変条件違反として拒否する。

version 1 envelope は後続C/Dで必要なfieldを最初から持つ。0063で到達可能なworkflow遷移は次だけである。

```text
新規 → Running → Suspended
新規 → Running → terminal result（checkpoint削除）
```

`ResultPending` / `Continuing` / `Finished` と continuation field はversion 1語彙として予約するが、このspecのapplication serviceは生成・遷移させない。未知version、未知state、必須field欠落、状態不変条件違反は `human_task_checkpoint_invalid` または `human_task_checkpoint_version_unsupported` で拒否し、自動修復・自動削除しない。

`HumanTaskParentContext` は後続継続に必要な `ai_session_id`、`conversation_id`、元turn ID、元ユーザー要求、元cwd、解決済み `llm_profile` だけを明示fieldとして保存する。環境変数、API key、provider request/stream、socket情報、callback、cancel flag、terminal設定、PTY fd、approval cacheを含めない。

元ユーザー要求、task、Observation、reasonは秘密を含み得る機微データとして扱う。checkpoint以外のログ・error・固定応答へ本文を複製せず、`status`は仕様で列挙したfieldだけを表示する。

task ID の例を `ht-20260714-7f31c2` とする。固定形式に一致しない空、`.`、`..`、slash、backslash、制御文字を含む値を拒否し、未検証IDをpath joinしない。

### 1.2 Store と保存場所

`ai` application は `HumanTaskStore` portだけに依存し、file adapterを直接参照しない。

```text
<AiConfig.history_dir>/human-tasks/
├── lock
└── <task-id>/
    └── checkpoint.json
```

file adapter は次を満たす。

- `human-tasks/` とtask directoryは `0700`、checkpointは `0600`
- path componentごとにsymlinkを拒否し、checkpoint openは `O_NOFOLLOW`
- directory / fileのownerが現在UIDでない場合は拒否
- 同一directoryの一時fileへ全量writeし、file `fsync`、atomic rename、parent directory `fsync` の順で確定
- decode前に上限を設け、JSON破損・未知versionを自動削除しない
- write失敗時に既存の有効checkpointを破壊しない
- error表示へcheckpoint本文、元ユーザー要求、reason、秘密候補値を含めない
- `lock` は0600/current UID/symlink拒否で開き、`flock(LOCK_EX)` によりcreate/resumeのroot操作を直列化する。status/cancelは`LOCK_EX|LOCK_NB`で非ブロッキング取得する

0055/0057のruntime handoff directoryは一時領域のままであり、checkpoint保存先に転用しない。secure file処理は既存実装から共通化可能なprimitiveだけを再利用し、`aibe` のfile-change journalへ依存しない。

### 1.3 Coordinator と開始順序

既存 `ExecuteHumanTask` は一つのHuman Shell segment実行に限定する。その上位に `HumanTaskCoordinator` を置き、次の順序を固定する。

1. rootの排他file lockを取得する
2. storeを読んで非終端taskがないことを確認する
3. task IDと親コンテキストを生成し、`Running` checkpointを保存する
4. 既存runtime directoryを作り、既存launcherでHuman Shellを起動する
5. shell終端後、0061のbounded Observation / Evidenceを収集してsegmentを一度だけ追加する
6. `Suspended`ならreason、final cwd、segmentとともにcheckpointを確定する
7. `Done` / `Blocked` / `Cancelled`なら0062の既存結果を返し、このspec用checkpointを残さず、最後にlockを解放する

Checkpoint作成前にHuman Shellを起動してはならない。保存失敗時はshellを起動せず `human_task_checkpoint_unavailable` を返す。既存非終端checkpointがある場合は `human_task_already_active` とし、既存fileを変更せず新しいshellを起動しない。

createはlockをactive確認前からHuman Shell終了後の最終save/removeまで保持する。statusとcancelは同じroot lockを非ブロッキング取得し、取得できた場合のみ一貫読取/removeする。これにより複数`ai` process間でもsingle-active-task invariantを守る一方、Human Shell子プロセスからのstatusがハングしない。flockはoperation-scoped exclusionであり、process消失後の所有権を表すlease/heartbeatではない。

### 1.4 Human Shell suspend protocol

内部aish→ai Human Shell resultの`normal_return: bool`を次の終端結果へ置換し、ai側`HumanShellReturn`も同じoutcomeを使う。単一の`result.json`は`Suspended`時だけvalidation済みの任意`suspend_reason`を持ち、field欠落は後方互換の`None`として扱う。別sidecarは正本にせず、`outcome=Suspended`ならreason欠落でも中断を維持する。旧`shell_exec` handoff adapterは`Done`だけを従来の正常復帰へ写像し、aibeへ返す旧wire DTO `HumanHandoffResult`は変更しない。

```text
HumanShellOutcome
  Done
  Suspended
```

`human-task` はHuman Shellの一時rcfileだけへ注入するshell functionとし、通常のbash/zsh設定やPATHへ恒久インストールしない。

```text
human-task suspend
human-task suspend "認証情報を確認してから続ける"
```

functionは引数を一つの理由文字列としてsize/control-character validation後にcontrol FIFOへ `human_suspend` eventを送り、現在cwdを既存の安全なcontrol transportで通知してshellを終了する。shell command文字列の組立てでJSONを生成しない。validationまたはevent送信に失敗した場合はnon-zeroを返してshellを終了せず、suspend成功を表示しない。

`human_suspend` 後にEXIT trap由来の `human_return` が届いても、aishは最初に受信した終端eventだけを採用し、`Suspended`を`Done`で上書きしない。EOF / `exit` は従来どおり `Done` であり、暗黙にsuspendへ変換しない。

### 1.5 Protocol と aibe turn停止

`HandoffExecutionOutcome::Suspended` を明示 `HumanTaskResult` 用に追加し、task IDを返せるようにする。`Suspended`は `verified=false`、終端segment metadataとObservationを持ち、最終完了を意味しない。0055の `HumanHandoffResult` は `Suspended` を生成せず、旧 `shell_exec` handoff形式を維持する。

`HumanTaskTool` がSuspended resultを受けた場合、aibeの既存tool roundは内部 disposition `SuspendTurn { task_id }` を返す。`AgentTurnService` は同じround以後のtool実行と次のLLM callを行わず、次の固定応答でturnを正常終了する。

```text
Human Task suspended.

Task:
  <task-id>

Cancel:
  ai human-task cancel --yes
```

中断はtool errorやturn cancelではない。固定応答の生成にLLMを使わない。未実装の`resume`は案内せず、動作するlocal cancelだけを案内する。

### 1.6 `ai human-task status` / `cancel`

CLIはaibe socketへ接続せずlocal storeだけを参照する。statusはまずroot lockを非ブロッキング取得し、取得できた場合は一貫読取する。Suspended taskがある場合はtask ID、`suspended`、objective、suspended time、任意reason、current cwd、`ai human-task cancel --yes` を表示してexit 0とする。root lockを取得できた状態でRunningが残る場合は所有processの予期しない終了後に残ったorphaned Runningとしてtask ID、objective、cwd、cancel復旧案内を表示する。root lockが他processに保持されている場合（Human Shell実行中）はブロックせず、保存済みRunningをbest-effort読取して`running`（active）とsuspend案内を表示する。task entryがない場合は厳密に `No suspended Human Task.` を表示してexit 0とする。

cancelはroot lockを非ブロッキング取得し、取得できない場合は`human_task_checkpoint_busy`で失敗する（file非変更）。取得できた場合はSuspendedまたはorphaned Runningを再確認してremoveする。`--yes`なしはTTY stdinで状態を区別した確認を行い、非TTY、拒否、入力失敗はnon-zeroかつfile非変更とする。taskなしはstatusと同じ成功文でexit 0とする。破損、未知version、権限不正は「taskなし」へ丸めず、削除しない。有効task ID directoryが存在するのに`checkpoint.json`が欠落する空directory、temp fileだけのdirectory、その他残骸も`Invalid`として保持する。cancelはagent continuationやCancelled tool resultをaibeへ送らないlocal復旧に限定する。

`resume` parserとsegment再開は公開しない。

## 2. Fault model

### 2.1 保証対象

標準Fault Modelを基礎とし、明示的な正常操作 `human-task suspend` に限って、単一ホスト・単一ユーザー上の `ai` process正常終了後もSuspended checkpointを保持する。create/status/cancelはroot flockで直列化し、並行`ai` processに対するsingle-active-task invariantを保証する。`aibe` processは中断turnを正常終了できるまで生存しているものとする。

### 2.2 保証対象外

- `ai` / `aish` / `aibe` crash、SIGKILL、OS再起動の途中状態からの自動復旧
- `Running` checkpointの所有process消失検出と `Suspended` への自動遷移
- 複数user / host、exactly-once、create/status/cancel以外の複数process協調
- stale ownership判定、PID ownership、lease、heartbeat、reconciler
- 旧schema migration。version 1以外は保持したまま拒否
- suspend確定前に外部副作用が起きた結果不明状態の解消

## 3. Non-goals

- `ai human-task resume` の実行
- 新しいHuman Shell segmentの開始、複数回中断、Evidence統合
- `ResultPending`、agent continuation、新しいCollaborative Modeターン
- 同一LLM/socket/agent loop/ai・aish process/PTYの復元
- 0055旧 `shell_exec` handoffへのsuspend追加
- 複数active task、task一覧・履歴UI、自動期限切れ
- crash recovery、schema migration、journal、idempotency key、lease

## 4. Complexity inventory

| 項目 | 数・内容 |
|------|----------|
| 実行主体 | 2（既存aibe agent turn、既存ai同期Human Shell callback） |
| 状態機械 | 1（Human Task checkpointの `Running → Suspended/terminal`） |
| 永続 aggregate | 1（version 1 `HumanTaskCheckpoint`） |
| 外部副作用 | 2（既存Human Shell/control event、secure checkpoint filesystem） |
| プロセス境界 | 2（既存ai↔aibe socket、既存ai→aish Human Shell） |
| 新規基盤機構 | 1（suspendable Human Task checkpoint） |
| 他機能統合 | 3（0062 human_task、0055/0057 Human Shell、0060/0061 briefing/Evidence） |

`scripts/feature-scope.toml` の0063 entryと一致させる。

## 5. Complexity Gate

- 判定: **Yellow（Scope Gate承認済み、設計レビュー済み）**
- 理由: actors、state machine、aggregate、effectはGreen上限内だが、既存process boundaryが2、既存機能統合が3でYellow閾値に達する
- 分割判断: Aだけでは本番経路からcheckpointを生成できないため最小Bを含める一方、resume、agent continuation、recovery hardeningを別specへ送ってnovel mechanismを一つに固定する
- 承認例外: 不要（Redではない）

Issue全体は複数novel mechanismとcrash recovery候補を含むためRedであり、本分割なしでは実装しない。

## 6. Complexity budget

| 項目 | 上限 |
|------|------|
| 新規実行主体 | +0 |
| 状態機械 | +0（1個で固定） |
| 永続aggregate / 正本 | +0（checkpointのみ） |
| external effect | +0 |
| process boundary / socket | +0 |
| novel mechanism | +0 |
| integrations | +0 |
| agent loop | 新設 +0（既存loopの停止dispositionのみ） |

## 7. Split triggers

次が必要になったらSTOP-THE-LINEし、0063へ追加せずscope revisionとComplexity Gateを再判定する。

- `resume`、二つ目のsegment、Evidence統合、agent continuation
- 二つ目の状態機械・永続正本・実行主体・agent loop
- file lockを越えるownership、lease、heartbeat、reconciler
- crash後自動再開、`Running`自動復旧、schema migration、journal
- idempotency key、exactly-once、外部副作用の結果不明状態の解消
- create/status/cancel以外の複数process協調、複数user / host、task list/history UI

## 8. パック構成の適用

**No** — 0045 §6の候補条件を満たさない。中断checkpointは明示 `human_task` lifecycleのcore契約であり、無効化した別basic runtime、重い依存のlink除外、optional配備を目的としない。`aish`はパック構成対象外で、`ai` / `aibe`も既存portとturn policyを必要最小限に拡張する。Pack境界、Active/Basic Pack、runtime toggle、Cargo feature、disabled testは作らない。

## 9. Acceptance Criteria

| ID | 条件 |
|----|------|
| `human_task_suspend_checkpoint_vertical_e2e` | 明示 `human_task` 開始、Running保存、理由付きsuspend、segment観測、Suspended保存、aibe追加LLM呼出しなしの正常turn終了、別process相当のstatus表示までが一貫して通る |
| `human_task_checkpoint_is_saved_before_shell_launch` | checkpoint保存成功後だけHuman Shellを起動し、保存失敗時は安定codeでfail-closedする |
| `human_task_checkpoint_v1_preserves_resume_context` | version 1 round-tripがtask、親session/conversation/turn/user message/profile、cwd、index 0の完全なsegmentを保持し、reserved continuation/final resultを未使用に保ち、禁止runtime情報をserialized JSONに含まない |
| `human_task_checkpoint_store_is_secure_and_atomic` | directory 0700、file 0600、owner確認、componentごとのsymlink拒否、1 MiBのwrite/read上限、同一directoryのtemp write + file fsync + rename + directory fsyncを満たす |
| `human_task_checkpoint_invalid_is_preserved` | 破損JSON、未知version、状態不変条件違反を自動削除・上書きせず安定codeで拒否する |
| `human_task_id_is_safe_path_component` | task IDを `^ht-[0-9]{8}-[0-9a-f]{6}$` で検証し、`.`、`..`、separator、control characterをpathとして受理しない |
| `human_task_suspend_function_is_ephemeral` | `human-task suspend [reason]` はHuman Shell一時rcfileだけに存在し、通常shell設定やPATHを変更せず、4096 bytes超過・control character・event送信失敗ではshellを終了しない |
| `human_task_suspend_first_terminal_event_wins` | `human_suspend` 後のEXIT trap `human_return`でSuspendedをDoneへ上書きせず、reasonとfinal cwdを保持する |
| `human_task_suspend_stops_agent_turn_without_llm` | Suspended result後は同roundの残りtoolと次LLM callを実行せず、固定cancel案内でturnを非エラー終了する |
| `human_task_status_reports_suspended_checkpoint` | `ai human-task status` がsocket不要でtask ID、state、objective、time、任意reason、cwd、動作するcancel案内を表示しexit 0となる |
| `human_task_status_reports_no_task_as_success` | Suspended taskがない場合に `No suspended Human Task.` を表示しexit 0となる |
| `human_task_status_does_not_hide_invalid_checkpoint` | 破損、未知version、owner/permission不正をtaskなしへ丸めずnon-zeroで返しfileを変更しない |
| `human_task_active_collision_fails_closed` | 非終端checkpointがある場合は `human_task_already_active` とし、既存checkpointを変更せずshellを起動しない |
| `human_task_normal_done_leaves_no_suspend_checkpoint` | Ctrl+D / exitは既存Done・verified=false・同じ親agent継続を維持し、中断用checkpointを残さない |
| `human_task_cancel_clears_suspended_checkpoint` | local cancelがSuspendedだけを削除し、taskなしをexit 0に保ち、削除後に新しいHuman Taskを開始できる |
| `human_task_cancel_requires_confirmation_without_yes` | `--yes`なしの非TTY・拒否・入力失敗はnon-zeroでcheckpointを変更せず、TTY承認だけ削除へ進む |
| `human_task_create_holds_root_lock_until_terminal` | createがroot lockをactive確認前から最終save/removeまで保持し、並行processがHuman Shell実行中に同lockを取得できない |
| `human_task_status_reports_active_running_without_blocking` | Human Shell所有中にroot lockが保持されていても`ai human-task status`はブロックせずactive `running`を表示し、cancelはbusyで失敗する |
| `human_task_suspend_is_explicit_tool_only` | suspendは0062の明示human_taskだけで利用でき、0055旧shell_exec handoffと通常shell_execのprotocol/resultを変更しない |
| `human_task_suspend_preserves_bash_zsh_and_prior_stages` | bash/zsh、0055/0057 cleanup、0060 briefing、0061 Evidence、0062明示toolの既存正常経路が回帰しない |
| `human_task_orphaned_running_cancel_recovers` | root lock取得後に残るRunningをorphanedとしてstatus表示し、確認付きcancelで削除した後に新規Human Taskを開始できる |
| `human_task_checkpoint_directory_without_checkpoint_is_invalid` | 有効task ID directoryが空、temp fileのみ、または`checkpoint.json`欠落ならNotFoundへ丸めずInvalidとして残骸を保持する |
| `human_task_suspended_result_without_sidecar_preserves_checkpoint` | 単一result JSONのSuspendedをsidecarなしでも維持し、reason field欠落時はNoneとしてSuspended checkpointを保存する |

Scope Lockは設計レビュー後の実装開始時に全rowを `scripts/spec-acceptance.toml` と1:1登録して固定する。現時点ではgovernance checkerが要求するVertical Slice ACだけをpending scaffoldとして登録する。

Vertical Sliceは `aibe/tests/` に置き、scripted LLM、fake Human Shell launcher / observer、tempdir storeを使って既存Unix socket経路を通す。最初のLLM応答に同一roundの `human_task` と後続tool callを返させ、suspend後に後続toolが未実行かつLLM call countが1であること、server終了後にstoreを新しく開いた`status`が同じcheckpointを表示することを検証する。実provider、実PTY、ユーザーの通常rcfileには依存しない。

## 10. Deferred specs

- **0063-C（別4桁番号）**: `resume [TASK_ID]`、cwd/briefing復元、新segment、複数回中断、Evidence順序統合
- **0063-D（別4桁番号）**: `ResultPending`、保存済み最終結果、親contextからの新Collaborative Mode turn、continuation turn ID、同一aibe process内重複防止
- **0063-E（別4桁番号）**: stale ownership、予期しないprocess終了、破損checkpointの追加回復UX、permission/session pruning競合、通常系以外のbash/zsh hardening。0063本体のoperation-scoped root flock、local cancel、fail-closedな破損拒否・owner/mode/symlink検査とbash/zsh正常系は後送しない
- crash recovery、schema migration、lease / heartbeat / reconciler、aibe再起動越しexactly-onceは標準Fault Model外。必要性が確定するまでspec化しない

## 11. Scope change log

| Revision | 分類 | 変更 | 理由 |
|----------|------|------|------|
| 1 | INITIAL | Checkpoint Aと本番suspend Bの最小経路を一つのVertical Sliceとしてdraft登録 | Aだけではcheckpointを作る製品導線がなく、B全体にresume/continuationまで含めるとRedになるため |
| 2 | SCOPE_LOCK | 設計レビュー済みの全16 ACを実装Scope Lockとして固定 | Step 4の本番実装前にAC集合とPhase gateを機械検査可能にするため |
| 3 | BLOCKER_ORIGINAL_AC / SAFETY_WITHIN_FAULT_MODEL / REGRESSION | local cancel復旧、固定応答/statusのcancel案内、create/status/cancel root flock、3 ACを追加 | P1: 未実装resume案内でSuspendedが恒久拒否になる原AC blockerと安全性を解消。P2: 並行ai processでsingle-active-task invariantが破れる回帰を防止 |
| 4 | REGRESSION / SAFETY_WITHIN_FAULT_MODEL | orphaned Runningの確認付きcancel、task directory残骸のInvalid判定、Suspended resultの単一JSON化、3 ACを追加 | PR #7追加レビューで、root lock取得後の復旧不能、checkpoint欠落のno-task誤判定、sidecar欠落によるSuspended消失を解消するため |
