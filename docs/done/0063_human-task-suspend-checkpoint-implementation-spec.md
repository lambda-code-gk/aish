# 0063 Human Task Suspend Checkpoint 実装指示書

設計書: [`docs/spec/0063_human-task-suspend-checkpoint-spec.md`](../spec/0063_human-task-suspend-checkpoint-spec.md)  
親概要: [`docs/spec/0063_human-task-suspend-resume-overview.md`](../spec/0063_human-task-suspend-resume-overview.md)

## 0. 目的

明示 `human_task` から起動した Human Shell を `human-task suspend [reason]` で中断し、version 1 checkpointを `ai` の安全なlocal storeへ保存する。中断後はaibe turnを追加LLM呼び出しなしで正常終了し、別の `ai human-task status` がsocketなしで保存内容を確認できるところまでを本番経路として実装する。設計の正本は0063設計書であり、本書は実装順序、テスト配置、fake境界、Phase gateを固定する。

パック構成は設計書 §8 の判定どおり **No**。checkpointは明示Human Task lifecycleのcore契約であり、Pack trait、Active / Basic Pack、runtime toggle、Cargo featureを追加しない。

## 0.1 Scope Lock

- Feature scope registry: `scripts/feature-scope.toml`
- Scope revision: **3**（PR #7 review対応）
- Status: `locked`
- Complexity class: **Yellow**（`scope_review = "approved"`）
- Vertical slice AC ID: `human_task_suspend_checkpoint_vertical_e2e`
- Locked AC IDs: 設計書 §9 の全19 ID（registry記載順）

Scope Lock後の発見事項は `BLOCKER_ORIGINAL_AC` / `REGRESSION` / `SAFETY_WITHIN_FAULT_MODEL` / `NEW_REQUIREMENT` / `HARDENING` / `OUT_OF_FAULT_MODEL` に分類する。前3分類だけが0063をブロックできる。後3分類を現在の実装へ取り込まない。

## 0.2 Vertical Slice

```text
scripted LLMの明示human_task
→ ai HumanTaskCoordinatorがRunning checkpointを実file adapterへ保存
→ fake Human Shellがreason付きSuspendedとfinal cwdを返す
→ 既存observerがbounded Observationを返す
→ aiがindex 0 segmentを含むSuspended checkpointをatomic保存
→ aibeが同roundの後続toolと次LLM callを止め固定応答で正常終了
→ server終了後に新しいai process相当のstatusが同じstoreを開いて表示
```

Vertical E2EはDTOの直組みやstore fakeだけで済ませない。既存Unix socket、client callback、`HumanTaskTool`、coordinator、実file adapter、status application serviceを通し、外部providerと実PTYだけをfakeにする。

## 1. Phase 分割

| Phase | 内容 | ゲート（`scripts/spec-acceptance.toml`） |
|-------|------|------------------------------------------|
| 1 | checkpoint domain/port/file adapter、coordinator、aish suspend protocol、aibe SuspendTurn、local statusを一本の本番Vertical Sliceとして接続 | Phase 1の10 ACを `pending = false` にする。特に `human_task_suspend_checkpoint_vertical_e2e` が緑になるまでPhase 2へ進まない |
| 2 | invalid保存、no-task/invalid status、active collision、旧tool分離、bash/zsh・0055/0057/0060/0061/0062回帰、docs/manual | 残り6 ACを `pending = false` にする |

**Phase 1完了条件**: Phase 1の同名代表testが全て本番assertionとなり、`#[ignore]`が外れ、対象crate test・両checkerが成功すること。Vertical E2EはLLM call count 1、後続tool未実行、Suspended checkpoint再open、status exit 0と許可field表示を同時に検証する。

**Phase 2完了条件**: 残り6 ACと関連unit/integration/regressionが緑になり、security/architecture/testing/manualが同期し、`./scripts/verify.sh`が成功すること。

**Vertical Slice Gate**: Phase 1成功前にresume、複数segment、continuation、crash recovery、schema migration、lease、汎用永続framework、性能最適化へ進まない。PR #7 reviewでoriginal ACの恒久拒否を解消するlocal cancelと、single-active-task invariantを守るroot flockはrevision 3で追加した。

## 2. 受け入れ条件

全行を `scripts/spec-acceptance.toml` に同名test関数で1:1登録する。現在は全行 `pending = true` かつ `#[ignore]` scaffoldであり、対応する本番経路のassertionが緑になった行だけ `pending = false` にする。

| ID / テスト関数 | Phase | 配置 | 検証責務 |
|------------------|-------|------|----------|
| `human_task_suspend_checkpoint_vertical_e2e` | 1 | `aibe/tests/0063_human_task_suspend_checkpoint_red.rs` | socket、scripted LLM、ai callback、実tempdir store、fake shell/observer、turn停止、store再open statusを縦断 |
| `human_task_checkpoint_is_saved_before_shell_launch` | 1 | `ai/tests/0063_human_task_suspend_checkpoint_red.rs` | recording store/launcherの順序、save失敗時launcher zero call、安定code |
| `human_task_checkpoint_v1_preserves_resume_context` | 1 | 同上 | V1 round-trip、全resume context、segment index 0、reserved field `None`、serialized禁止語彙なし |
| `human_task_checkpoint_store_is_secure_and_atomic` | 1 | 同上 | 実filesystemで0700/0600、UID、symlink、1 MiB前後、既存file保全、fsync/rename fault seam |
| `human_task_checkpoint_invalid_is_preserved` | 2 | 同上 | corrupt/unknown version/invariant violationの安定codeとbyte-for-byte非変更 |
| `human_task_id_is_safe_path_component` | 1 | 同上 | valid例、空、`.`、`..`、slash/backslash、Unicode/C0、長さ/hex形式をpath join前に検証 |
| `human_task_suspend_function_is_ephemeral` | 1 | `aish/tests/0063_human_task_suspend_checkpoint_red.rs` | bash/zsh一時rcfile、通常rc/PATH非変更、reason 4096境界、control/event送信失敗時shell継続 |
| `human_task_suspend_first_terminal_event_wins` | 1 | 同上 | `human_suspend`→EXIT `human_return`を実control transportへ流しreason/final cwd/Suspendedを保持 |
| `human_task_suspend_stops_agent_turn_without_llm` | 1 | `aibe/tests/0063_human_task_suspend_checkpoint_red.rs` | 同round後続tool zero call、LLM count 1、固定応答、非error turn終端 |
| `human_task_status_reports_suspended_checkpoint` | 1 | `ai/tests/0063_human_task_suspend_checkpoint_red.rs` | socket不在、許可field、local時刻、任意reason、動作するcancel案内、exit 0 |
| `human_task_status_reports_no_task_as_success` | 2 | 同上 | 厳密な固定文とexit 0、storeへのwriteなし |
| `human_task_status_does_not_hide_invalid_checkpoint` | 2 | 同上 | Running/corrupt/version/owner/mode不正をnon-zero、安定code、file非変更で返す |
| `human_task_active_collision_fails_closed` | 2 | 同上 | 既存Running/Suspendedを非変更、`human_task_already_active`、launcher zero call |
| `human_task_normal_done_leaves_no_suspend_checkpoint` | 1 | 同上 | Ctrl+D/exitのDone・verified=false・親agent継続・checkpoint削除 |
| `human_task_cancel_clears_suspended_checkpoint` | 2 | 同上 | Suspendedだけをlocal削除、taskなし成功、削除後の新規開始 |
| `human_task_cancel_requires_confirmation_without_yes` | 2 | 同上 | `--yes`なしの非TTY/拒否でnon-zeroかつcheckpoint非変更 |
| `human_task_create_holds_root_lock_until_terminal` | 2 | 同上 | root flockをactive確認前から最終save/removeまで保持し、別fdの`LOCK_NB`競合で検証 |
| `human_task_suspend_is_explicit_tool_only` | 2 | `aibe/tests/0063_human_task_suspend_checkpoint_red.rs` | 0062 `human_task`だけがSuspendedを生成し、0055 DTO/wireと通常`shell_exec`を不変にする |
| `human_task_suspend_preserves_bash_zsh_and_prior_stages` | 2 | `aish/tests/0063_human_task_suspend_checkpoint_red.rs` | bash/zsh、cleanup、briefing、Evidence、explicit toolの既存正常経路を実fixtureで回帰確認 |

代表testを文字列検索、型の存在確認、fakeの固定値だけで通してはならない。個々の境界値はmodule unit testへ分割してよいが、同名代表testは該当adapter/applicationまたはprotocol本番経路を必ず通す。

## 3. レイヤー別実装タスク

### 3.1 Domain（最初に実装）

| 候補ファイル | 作業 |
|--------------|------|
| `ai/src/domain/human_task_checkpoint.rs`（新規）、`ai/src/domain/mod.rs` | `HumanTaskId`、`HumanTaskWorkflowState`、`HumanTaskCheckpointV1`、`HumanTaskParentContext`、`HumanShellSegment`、`HumanShellSegmentEnd`、`HumanTaskContinuationState`を定義する。task ID/reason/timestamp/size/state invariantはpure validationに集約する |
| `aibe-protocol/src/collaborative_handoff.rs` | 明示Human Task用に`HandoffExecutionOutcome::Suspended`とtask ID/segment metadataを追加し、`HumanTaskResult` invariantを更新する。予約stateをapplicationから生成しない |
| `ai/src/ports/outbound/human_handoff.rs` | `normal_return: bool`を`HumanShellOutcome::{Done,Suspended}`へ置換し、Suspendedだけvalidation済みreasonを持てるようにする |

serialization envelopeは明示version fieldを持ち、1 MiB判定はencode前後双方で行う。`ClientRequest`、provider request、API key、env、socket、callback、cancel flag、terminal/PTY情報をaggregateへ入れない。未知version、予約state到達、欠落fieldを自動修復しない。

### 3.2 Ports

| 候補ファイル | 作業 |
|--------------|------|
| `ai/src/ports/outbound/human_task_store.rs`（新規）、`ai/src/ports/outbound/mod.rs` | `HumanTaskStore`のroot lock/load/save/removeをdomain型と安定errorで定義する。applicationからpath/JSON/fs APIを隠し、fakeはno-opまたはMutex guardを返す |
| `ai/src/ports/outbound/human_task_identity.rs`（新規候補） | task ID生成とUTC millisecond clockを注入可能にする。production adapterはOS entropy/time、testはdeterministic fakeを使う |
| 既存 `HumanShellLauncher` / `EnvironmentObserver` | 一segment実行・bounded Observationのportを再利用する。checkpoint APIやresume責務を追加しない |

portのerrorへcheckpoint本文、objective、元ユーザー要求、reason、秘密候補値を含めない。removeはterminal Done/Blocked/Cancelled後のcleanupと、Suspendedを確認済みのlocal cancelだけに使い、invalid checkpointを消す回復APIにはしない。

### 3.3 Adapters

| 候補ファイル | 作業 |
|--------------|------|
| `ai/src/adapters/outbound/human_task_file_store.rs`（新規）、`ai/src/adapters/outbound/mod.rs` | checkpointを0700/0600、current UID、component symlink拒否、`O_NOFOLLOW`、1 MiB上限、same-dir temp→file fsync→rename→parent fsyncで実装し、rootの0600 `lock`へ`flock(LOCK_EX)`する |
| `ai/src/adapters/outbound/human_handoff.rs` | aish resultのDone/Suspended decodeを接続し、legacy shell handoffはDoneへ写像する。reasonやcheckpoint本文をlog/errorへ出さない |
| `aish/src/human_shell.rs`、`aish/src/adapters/outbound/shell_completion.rs`、必要ならcontrol event module | 一時rcfileへだけ`human-task` functionを注入し、shell command内でJSONを組み立てずversioned control eventを送る。最初のterminal eventだけを確定する |

secure storeの低水準処理は既存secure filesystem primitiveを再利用してよいが、aibe file-change journal、二つ目の永続正本、lease/reconcilerを導入しない。root flockはcreate/status/cancelのoperation-scoped exclusionだけとし、ownership recoveryに使わない。

### 3.4 Application

| 候補ファイル | 作業 |
|--------------|------|
| `ai/src/application/human_task_coordinator.rs`（新規）、`ai/src/application/mod.rs` | load collision確認→ID/parent作成→Running save→既存runtime/launcher→observer→Suspended saveまたはterminal cleanupの順を唯一のorchestrationとして実装する |
| `ai/src/application/execute_human_task.rs` | 一segment executorとして維持し、coordinator上位から呼ぶ。Observationを一度だけ収集し、statusや永続fileを直接扱わない |
| `ai/src/application/human_task_status.rs` / `human_task_cancel.rs` | root lock下でSuspended invariantを検証し、status表示または確認済みremoveを行う。taskなしだけをsuccess emptyへ写像する |

Running保存失敗はshell開始前に `human_task_checkpoint_unavailable`。既存非終端は `human_task_already_active`。Done/Blocked/Cancelledのcleanup失敗を成功に丸めず、既存有効checkpointを破壊しない。coordinatorはroot lockをactive確認前からHuman Shell終了後の最終save/removeまで保持する。status/cancelも同じlock下でloadし、cancelはSuspendedだけをremoveする。

### 3.5 CLI / aibe agent / aish protocol（最後に縦断接続）

| 境界 | 候補ファイル | 作業 |
|------|--------------|------|
| CLI | `ai/src/clap_cli.rs`、`ai/src/main.rs` | `AiCommand::HumanTask { Status }`だけを公開し、`resume`/`cancel` parserを追加しない。`AI_CONFIG.history_dir`からstoreを構成しaibe socketへ接続しない |
| aibe tool | `aibe/src/adapters/outbound/tools/human_task.rs` | Suspended resultを通常tool JSON完了へ流さず内部turn dispositionへ変換する。0055 `shell_exec` adapterにはSuspended分岐を足さない |
| aibe round | `aibe/src/application/tool_round/executor.rs`、`aibe/src/application/agent_turn.rs` | `RoundOutcome::SuspendTurn { task_id }`相当を追加し、同round残りtoolと次LLM callを止め固定文を返す。cancel/error扱いにしない |
| wire/client | `aibe-protocol`、`aibe-client/src/transport.rs`、`aibe/src/adapters/inbound/connection_human_task.rs` | Suspended result/task IDのround-tripと既存相関検査を維持する。旧 `HumanHandoffResult` wire DTOは変更しない |
| aish | `aish` Human Shell/control FIFO | `human_suspend`がreason/final cwdを返しshellを終了する。validation/send失敗時はnon-zeroでshellを継続し成功表示しない |

固定応答は設計書の本文と改行を正本とし、LLM、objective、reason、Observationから生成しない。statusはtask ID/state/objective/time/reason/cwd/cancel案内以外を表示しない。

## 4. Fake / fixture 方針

- `RecordingHumanTaskStore`: call順、保存snapshot、remove回数を記録するin-memory fake。coordinator unit専用でありVertical E2Eには使わない。
- `FailingHumanTaskStore`: load/save/removeの指定段階で安定errorを返す。shell zero-callと既存checkpoint保全を検証する。
- `DeterministicTaskIdGenerator` / `FakeClock`: `ht-20260714-7f31c2`と単調なUTC millisecondsを返し、snapshotを安定化する。
- `FakeHumanShellLauncher`: Done/Suspended/errorをscriptし、launch requestとcall countを記録する。Suspended時はsession ID、log range、final cwd、reasonを返す。
- `RecordingEnvironmentObserver`: 既存`PostHandoffObservation`をbounded値で返し、1回だけ呼ばれたことを検証する。
- `ScriptedLlmProvider`: 最初の応答に`human_task`と後続tool callを同一roundで返す。2回目が呼ばれたらtest failureとする。
- `CountingToolExecutor`: suspend後の後続tool未実行を検証する。HumanTaskTool自体は本物を使う。
- file store fixture: `tempfile::TempDir`配下へ実directory/fileを作り、`symlink_metadata`、mode、UID、inode/content、1 MiB境界を検査する。production HOME/configへ触れない。
- shell fixture: temp HOME、temp rcfile、実FIFO/control transport、bounded timeoutを使う。通常のbash/zsh rcfileとPATHのbefore/afterを比較する。

fakeは外部非決定要因を閉じるためだけに使う。checkpoint serializer/file adapter、coordinator、turn disposition、status formatterをfakeへ置換してACを緑にしない。

## 5. 実装順序

1. **Scope確認**: revision 2、16 locked IDs、16 pending/ignored test、tasks配置を両checkerで確認する。
2. **Domain**: ID/reason/V1 aggregate/state invariant/segment/parent contextとprotocol outcomeをunit test firstで実装する。
3. **Ports**: store、clock/ID source、HumanShellOutcomeを定義し、依存方向をarchitecture checkerで確認する。
4. **File adapter**: secure create/read/atomic replace/size/version/permissionを実tempdir testで実装する。
5. **Coordinator**: recording fakesでRunning-before-launch、Suspended確定、Done cleanup、active collisionを実装する。
6. **aish protocol**: ephemeral function、reason validation、final cwd、first-terminal-event-winsをbash/zsh fixtureで実装する。
7. **aibe turn停止**: HumanTaskTool resultからSuspendTurnを伝播し、残りtool/次LLM callを止め固定応答を返す。
8. **Status / cancel**: root lockを使うapplication serviceとCLI parse/dispatchを接続し、socket不使用、empty/invalid区別、確認fail-closedを実装する。
9. **Vertical E2E**: scripted LLM + fake shell/observer以外は本番経路で最小sliceを緑にする。
10. **Phase 1 gate**: Phase 1の10 testsから`#[ignore]`を外し、targeted testsと両checker成功後だけ10 rowを`pending=false`にする。
11. **Phase 2**: invalid保全、collision、legacy分離、bash/zsh/prior stage回帰、docs/manualを完成させ、残り6 rowを緑化する。
12. **完了処理**: targeted検証後に`./scripts/verify.sh`。全19 ACとverify成功後だけstatus=`done`を維持する。

## 6. テスト計画

### Unit

- Domain: ID/reasonのUTF-8 byte境界、Unicode control、timestamp順序、各state invariant、reserved state拒否、V1 round-trip、禁止field非serialization。
- Application: coordinator call順と分岐、status display model、安定error mapping。
- aibe: Suspended result invariant、round disposition、固定応答、LLM/tool call count。
- aish: event parser、first-terminal selection、shell function引数validation。

### Integration

- `ai`: 実file adapterのmode/owner/symlink/size/atomicity、CLI status、store再open、checkpoint非変更。
- `aibe`: scripted provider、実registry/tool executor/agent turn、Unix socket/client callback。
- `aish`: bash/zsh一時rcfile、FIFO event、EXIT trap競合、send failure、通常exit。
- protocol/client: new HumanTaskResult round-tripと旧HumanHandoffResult fixtureのbyte/shape回帰。

### E2E

- `human_task_suspend_checkpoint_vertical_e2e`をMinimum Vertical Sliceの正本とする。
- 既存0062 socket fixtureを拡張し、real provider、network、実PTY、ユーザーrcfileなしで再現する。
- server終了後、別process相当でstoreとstatus serviceを再構成し、in-memory state共有に依存しないことを確認する。

### Smoke / regression

実装中は順に次を実行する（全て直列）。

```bash
cargo test -p ai -j 1 --test 0063_human_task_suspend_checkpoint_red
cargo test -p aish -j 1 --test 0063_human_task_suspend_checkpoint_red
cargo test -p aibe -j 1 --test 0063_human_task_suspend_checkpoint_red
cargo test -p ai -j 1 --test 0062_collab_mode_human_task_tool_red
cargo test -p aish -j 1 --test 0055_minimal_human_handoff
cargo test -p aish -j 1 --test 0057_pty_process_cleanup_hardening
cargo test -p aish -j 1 --test 0060_collab_mode_human_task_briefing
cargo test -p ai -j 1 --test 0061_collab_mode_human_task_evidence
cargo test -p aibe -j 1 --test 0062_collab_mode_human_task_tool
./scripts/check-feature-scope.py
./scripts/check-spec-acceptance.py
```

Step 6用の正常系自動再現は、上記Vertical E2E（scripted LLM + fake Human Shell + tempdir実store）とbash/zsh integrationを正式commandとする。local CLI smokeは本番設定を読まない一時 `AI_CONFIG` に `history_dir` を指定し、次を確認する。

```bash
AI_CONFIG="$TMPDIR/ai.toml" cargo run -p ai -- human-task status
```

期待値はcheckpointなしなら厳密に `No suspended Human Task.` とexit 0。Suspended表示の手動fixture生成は製品外のJSON手書きにせず、Vertical E2Eまたは専用test helperだけで作る。

## 7. ドキュメント更新

- `docs/architecture.md`: checkpoint aggregate/owner、`HumanTaskStore` port/file adapter、coordinator順序、aish terminal event、aibe SuspendTurn、local statusと依存方向を追記する。
- `docs/testing.md`: 0063 unit/integration/E2E、scripted LLM/fake shell、実tempdir store、bash/zsh bounded testを追記する。
- `docs/security.md`: 0700/0600、UID/symlink/`O_NOFOLLOW`、1 MiB、atomic write、secret非logging、status表示allowlist、invalid非削除を追記する。
- `docs/manual/0063_human-task-suspend-checkpoint.md`: 明示Human Task開始、reasonあり/なしsuspend、別command status、local cancel、通常Done、validation失敗を記載する。
- `docs/manual/README.md`: 0063手動検証へのリンクを追加する。
- `docs/0000_spec-index.md`: 実装中はtasks rowを維持し、全AC完了後だけ実装済みへ変更する。
- CLI help: `human-task status`と`cancel [--yes]`だけを公開し、resumeが実行可能だと誤記しない。

実PTYの最終手動検証は人間が行う。未実施なら最終報告の「残リスク」に明記する。

## 8. エラー・安全契約

- `human_task_checkpoint_unavailable`: initial save/read/secure store操作に失敗。本文やpath由来の秘密値を出さない。
- `human_task_checkpoint_invalid`: JSON/schema/state invariant/size不正。fileを自動削除・上書きしない。
- `human_task_checkpoint_version_unsupported`: version 1以外。migrationやfallbackを行わない。
- `human_task_already_active`: RunningまたはSuspendedが存在。既存file非変更、shell zero-call。
- suspend reason validation error / event send failure: shell function non-zero、shell継続、成功表示なし。
- Suspended: `verified=false`、final result/continuationなし。tool error、cancel、完了として扱わない。

checkpoint/objective/user message/reason/Observationの本文は通常log、error、固定turn応答へ複製しない。statusだけが設計書で許可されたfieldを表示する。

## 9. STOP-THE-LINE / scope外

次が必要なら実装を停止し、0063へ追加しない。`feature-scope.toml`の`scope_revision`を増やし、設計書とComplexity Gateを再判定する。

- `ai human-task resume`、segment再開
- 二つ目のsegment、Evidence統合、ResultPending、continuation、新しいagent turn
- crash/OS再起動後の自動復旧、Running ownership検出、schema migration
- root flockを越えるPID ownership、stale判定、lease、heartbeat、reconciler、journal、idempotency key、exactly-once
- 二つ目の状態機械、永続正本、実行主体、agent loop、process boundary
- create/status/cancel以外の複数process協調、複数user/host、task list/history UI

予約語彙をversion 1に保持することは将来機能の実装許可ではない。`resume`案内文字列は表示するがcommand parserを先行公開しない。

## 10. 完了条件

1. 全19行の`spec-acceptance.toml`が`pending = false`で対応testに`#[ignore]`がない
2. Phase 1 Vertical SliceとPhase 2異常系・回帰が本番経路で成功
3. `./scripts/check-feature-scope.py`と`./scripts/check-spec-acceptance.py`が成功
4. architecture/testing/security/manual/helpが本番挙動と同期
5. `./scripts/verify.sh`が成功し、`.verify-timing-last`のsummaryを報告
6. 上記完了後だけ本書を`docs/done/`へ移し、scope statusとindexを`done` / 実装済みにする

## 11. 仕様との差分（意図的に縮小する場合のみ）

なし。

resume / agent continuation / crash recoveryは設計書のNon-goals / Deferredどおり実装しない。cancelはlocal checkpoint復旧だけでaibeへ結果を送らない。仮serializer、in-memory production store、常に成功するlauncher、固定Suspended result、source文字列検査で本機能を完了にしない。
