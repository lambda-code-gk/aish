# 0062 Collaborative Mode Human Task Tool 実装指示書

設計書: [`docs/spec/0062_collab-mode-human-task-tool-spec.md`](../spec/0062_collab-mode-human-task-tool-spec.md)

## 0. 目的

`ai collab "依頼内容"` を Collaborative Mode の正式導線とし、LLM が通常の `shell_exec` と独立した `human_task` tool から、同一 turn・同一 socket 接続上で既存 Human Shell へ同期委譲できるようにする。開始時は0060 briefing、終了後は0061 `PostHandoffObservation` / Evidenceを再利用し、structured `HumanTaskResult` を同じ親 agent loopへ返す。設計内容の正本は0062設計書だけであり、本書は実装順序と変更箇所を具体化する。

パック構成は設計書 §8 の判定どおり **No**。requestごとの core `ExecutionMode` と mode-dependent allowlistで実現し、Pack trait、Active / Basic Pack、Cargo feature、runtime toggleは追加しない。

## 0.1 Scope Lock

- Feature scope registry: `scripts/feature-scope.toml`
- Scope revision: **4**（implementation scope lock）
- Status: `locked`
- Complexity class: **Yellow**（`scope_review = "approved"`）
- Vertical slice AC ID: `collab_human_task_vertical_with_fakes`
- Locked AC IDs: 設計書 §9 の全16 ID（registry記載順）。追加・削除・改名は禁止

Scope Lock後のレビュー指摘は `BLOCKER_ORIGINAL_AC` / `REGRESSION` / `SAFETY_WITHIN_FAULT_MODEL` / `NEW_REQUIREMENT` / `HARDENING` / `OUT_OF_FAULT_MODEL` に分類する。前3分類だけが現在の0062をブロックできる。後3分類は本実装へ無条件に取り込まず、Deferredまたは別spec候補とする。

## 1. Phase 分割

| Phase | 内容 | ゲート（`scripts/spec-acceptance.toml`） |
|-------|------|------------------------------------------|
| 1 | CLI mode、mode policy、tool schema/executor、同期 gate、ExecuteHumanTask、briefing transport、result、prompt instructionをfake縦断で接続 | Phase 1の13 ACを `pending = false` にし、`collab_human_task_vertical_with_fakes` が緑になるまでPhase 2へ進まない |
| 2 | fail-closed異常系、0055/0057/0060/0061・通常経路回帰、CLI help / architecture / security / manual同期 | 残り3 ACを `pending = false` にする |

**Vertical Slice Gate**: Phase 1成功前に、永続化、schema migration、crash recovery、並列・入れ子、side agent、GUI観測、汎用callback framework化、旧 `shell_exec` interception削除を実装してはならない。

**実装中の禁止事項**: 新しい実行主体、状態機械、永続aggregate、process boundary、socket、agent loop、外部副作用が必要になったらSTOP-THE-LINEし、設計書と `feature-scope.toml` のrevision / Complexity Gateを再判定する。`HumanTaskGate` は独立基盤ではなく、`human_task` と一体の同期callback adapter一件に留める。

## 2. 受け入れ条件

全行は `scripts/spec-acceptance.toml` に同名test関数で1:1登録済み。未実装中は `ai/tests/0062_collab_mode_human_task_tool_red.rs` の `#[ignore]` REDを維持する。test名をまとめるmacroはcheckerが検出できないため使用しない。

| ID | 条件 | テスト関数 | Phase | pending |
|----|------|------------|-------|---------|
| `collab_cli_selects_collaborative_mode` | `ai collab`は既存ask入力解決を再利用してCollaborative、通常askはNormal | 同名 | 1 | true |
| `collab_legacy_flag_maps_to_same_mode` | 旧flagを同じmodeへ正規化し内容からmode推測しない | 同名 | 1 | true |
| `collab_preserves_explicit_tools_without_exec_requirement` | `@exec`必須なし、明示tools順序保持、`human_task`重複なし | 同名 | 1 | true |
| `human_task_is_published_only_in_collaborative_mode` | common registry/definition経路でCollaborativeだけ公開し、Normal/forged allowlist拒否 | 同名 | 1 | true |
| `human_task_schema_matches_request_contract` | request schema、正規化、未知field/型/空element/64 KiB拒否 | 同名 | 1 | true |
| `human_task_briefing_uses_task_labels_and_omits_empty_sections` | 0060 rendererの固定ラベルを使い空sectionを省略 | 同名 | 1 | true |
| `execute_human_task_uses_existing_human_shell_ports` | fake launcher/observerで順序とversioned JSON一個のtransportを検証 | 同名 | 1 | true |
| `human_task_result_reuses_status_and_observation_types` | 既存outcome/observation再利用とstatus/error不変条件 | 同名 | 1 | true |
| `human_task_result_requires_no_manual_summary` | 終了後入力なし、commands空でも正常done | 同名 | 1 | true |
| `human_task_done_does_not_mean_verified` | done/exit/Evidenceから達成やstatusを推定しない | 同名 | 1 | true |
| `human_task_is_independent_from_shell_exec` | `ShellExecTool` / `@exec` / allowlist / approval非依存 | 同名 | 1 | true |
| `collab_instruction_is_mode_scoped_and_not_in_cli` | pure builderから既存system instruction合成点へCollaborative限定追加 | 同名 | 1 | true |
| `collab_human_task_vertical_with_fakes` | modeからtool call、fake shell、観測、result、親継続まで縦断 | 同名 | 1 | true |
| `human_task_errors_are_structured_and_fail_closed` | schema/mode/gate/wire、blocked/cancelled/観測失敗の安定契約 | 同名 | 2 | true |
| `collab_human_task_preserves_prior_stage_regressions` | 0055/0057/0060/0061、通常shell_exec、Normal、明示tools非回帰 | 同名 | 2 | true |
| `collab_docs_use_official_entrypoint` | help/docsの正式・互換導線と`@exec`非必須表記 | 同名 | 2 | true |

## 3. 変更ファイル候補と責務

### 3.1 CLI・mode・prompt（`ai`）

| ファイル | 作業 |
|----------|------|
| `ai/src/clap_cli.rs` | `AiCommand::Collab`を`Ask`と同じ`TurnOptions` / `--file` / messageで追加し、root completionへ出す。既存`TurnOptions.collaborative`は互換flagとして残す |
| `ai/src/domain/ask_invocation.rs` | `collab`を既知headへ追加しimplicit `ask`挿入を防ぐ。新しいeditor/stdin/message joinは作らない |
| `ai/src/main.rs` | `Collab`を既存`run_ask`へdispatchし、parse直後に`ExecutionMode`へ正規化する。callback composition rootで`ExecuteHumanTask`を既存launcher/observerへ接続する。prompt本文やschema validationは置かない |
| `ai/src/domain/execution_mode.rs`（新規候補）、`ai/src/domain/mod.rs` | `ExecutionMode::{Normal, Collaborative}`をmodeの正本とし、旧booleanはwire互換境界でのみ変換する |
| `ai/src/domain/collab_instruction.rs`（新規候補）、`ai/src/domain/request_context.rs`、`ai/src/domain/console_context.rs` | Collaborative instructionの純粋builderと既存`RequestContextInput.system_instruction`合成を実装。console hint等の既存本文を上書きせず、Normalでは追加しない |
| `ai/src/domain/tools.rs`、`ai/src/application/ask_launch.rs` | 通常`resolve_tools`結果を基礎にmode policyを適用し、Collaborativeだけ末尾へ`human_task`をdedup追加する。`none`は`human_task`だけ、`@exec`は`shell_exec`だけというカテゴリ契約を維持する |

### 3.2 Protocol・client callback

| ファイル | 作業 |
|----------|------|
| `aibe-protocol/src/tool_name.rs`、`aibe-protocol/src/lib.rs` | `HUMAN_TASK`を組み込み名として一度だけ追加・exportする。`@full`やread-onlyカテゴリの意味は変更しない |
| `aibe-protocol/src/collaborative_handoff.rs` | `HumanTaskRequest`、`HumanTaskResult`、`HandoffExecutionOutcome::{Done, Blocked, Cancelled}`、validationを追加する。`HumanControlReturned`と旧`HumanHandoffResult` wire互換を残す |
| `aibe-protocol/src/request.rs` / `response.rs` | `HumanTaskExecutionResult` / `HumanTaskExecutionRequest`を追加し、turn id・tool call id・prompt id・request/resultを明示する。approval DTOを流用しない |
| `aibe-client/src/transport.rs`、`aibe-client/src/lib.rs` | `HumanTaskExecutionRequest` callback型を`AgentTurnCallbacks`へ追加し、同一streamでresultを書き戻す。既存shell/tool approval、client tool callbackを壊さない |
| `aibe-protocol/tests/` または各module unit test | old payload decode、new round trip、未知field、status/error不変条件、相関IDのwire表現を固定する |

`prompt_id`はgate生成値を一つの待機callに束縛する。重複response、不一致、decode失敗、待機callなしを別callへ回送してはならない。

### 3.3 `aibe` tool・registry・gate

| ファイル | 作業 |
|----------|------|
| `aibe/src/application/tool_defs.rs` | `human_task` JSON Schemaを共通definition経路へ追加。`additionalProperties: false`、requiredは`objective`だけ、array itemはstring |
| `aibe/src/adapters/outbound/tools/human_task.rs`（新規）、`aibe/src/adapters/outbound/tools/mod.rs` | `HumanTaskTool`でarguments decode/trim/64 KiB validation、mode確認、gate呼出し、result validation、tool result JSON/error code mappingを担う。`ShellExecTool`へ分岐を足さない |
| `aibe/src/adapters/outbound/tools/registry.rs`、同`build_registry` | executorを一度だけ組み込みregistryへ登録する。`ToolRoundExecutor`やdispatch箇所ごとのad hoc追加は禁止 |
| `aibe/src/ports/outbound/human_task.rs`（新規候補）、`aibe/src/ports/outbound/mod.rs` | 同期`HumanTaskGate::execute_human_task(turn_id, tool_call_id, prompt_id/request)` portとunavailable表現を定義する |
| `aibe/src/ports/outbound/tool_context.rs` | `execution_mode`と任意`HumanTaskGate`をturn contextへ運び、server側Normal拒否を可能にする。旧`collaborative_handoff`は0055互換として残す |
| `aibe/src/application/agent_turn.rs`、`aibe/src/ports/inbound/client_request.rs` | request contextからmodeとgateをtool contextへ一度だけ渡す。tool名別実行ロジックは置かない |
| `aibe/src/adapters/inbound/connection_human_task.rs`（新規候補）、`aibe/src/adapters/inbound/unix_socket_server.rs` | `ConnectionApprovalGate`の相関/取消patternを参照して専用gateを同じwriter/lines/cancellationへ接続する。新socket・汎用protocol subsystemは作らない |
| `aibe/src/application/tool_round/executor.rs`、`aibe/tests/agent_turn_loop.rs` | tool result後に既存roundが同じ親agentを継続することだけを検証し、executorへ`human_task`固有分岐を追加しない |

### 3.4 ExecuteHumanTask・Human Shell・briefing・Evidence

| ファイル | 作業 |
|----------|------|
| `ai/src/application/execute_human_task.rs`（新規）、`ai/src/application/mod.rs` | request→briefing→`launch_and_wait`→`observe`→resultの順序だけを担う。fake portsで実shellなしのunit testを置く |
| `ai/src/ports/outbound/human_handoff.rs` | `HumanShellLaunchRequest`へ任意の構造化briefingを追加。旧`parent_request_summary` / `suggested_command`を残し、既存trait signatureとcancel経路を再利用する |
| `ai/src/adapters/outbound/human_handoff.rs` | 構造化briefingを上限検証済みversioned JSON一個として`AISH_HANDOFF_TASK_JSON`へ設定する。secret値をlog/errorへ出さず、既存runtime/termios/cleanup guardを維持する |
| `ai/src/domain/human_handoff.rs`、`aish/src/human_shell.rs` | 両側の`HANDOFF_ENV_KEYS`へ一個だけ追加し、aish側で64 KiB以下・既知versionだけdecodeする。不正/未知/超過時はshell開始前にblocked相当へする |
| `aish/src/human_shell.rs` | 0060 `render_human_task_briefing`を一つの表示モデルへ拡張し、固定ラベル・行単位escape・stderrを再利用する。旧env欠落時は従来表示へfallbackする |
| `aish/src/adapters/outbound/shell_completion.rs` | bash/zshとも起動直後に`AISH_HANDOFF_TASK_JSON`を読み取り対象とし、子shell環境からunsetする。値を補完scriptの表示へ出さない |
| `ai/src/adapters/outbound/human_handoff.rs` の `ProcessEnvironmentObserver` | 0061 observerをそのまま呼ぶ。新observer、独自range pairing、commandからstatus推定を追加しない |

`HumanTaskBriefing`はapplication/domain側の開始時モデルとし、env JSONは既存ai→aish process境界専用のversioned表現に限定する。個別fieldごとのenv、protocol用の別renderer、aishからLLM/protocol層への依存は追加しない。

### 3.5 テスト・docs・registry

| ファイル | 作業 |
|----------|------|
| `ai/tests/0062_collab_mode_human_task_tool_red.rs` | 16 ignored skeletonを同名の本番assertionへ段階的に置換する。CLI/process縦断は既存0055 mock aibe fixtureを再利用する |
| `aish/tests/0060_collab_mode_human_task_briefing.rs` | explicit list表示、空section省略、JSON invalid/version/size、env unset、旧4-key経路回帰を追加する |
| `ai/tests/0055_minimal_human_handoff.rs`、`ai/tests/0055_collaborative_handoff_vertical_e2e.rs`、`ai/tests/0057_pty_process_cleanup_hardening.rs`、`ai/tests/0061_collab_mode_human_task_evidence.rs`、`ai/tests/normal_shell_exec_regression.rs` | 旧handoff、cleanup/cancel、briefing、Evidence、通常shell_execの期待値を新env一個だけに合わせて回帰確認する |
| `scripts/feature-scope.toml`、`scripts/spec-acceptance.toml` | Scope Lockを維持し、Phaseごとに本番test緑化後だけ該当`pending`をfalseにする |
| `docs/architecture.md`、`docs/security.md`、`docs/manual/0062_collab-mode-human-task-tool.md`（新規）、`docs/manual/README.md` | mode/tool/callback、env JSONの秘密値取扱い、手動縦断手順を同期する |
| `docs/0000_spec-index.md`、本書 | 全16 AC緑・verify成功後だけ本書を`docs/done/`へ移し、0062を実装済みにする |

## 4. 実装順序

1. **Scope確認**: revision 4、16 locked IDs、16 pending/ignored testsをcheckerで確認する。
2. **Mode first**: `ExecutionMode`、`Collab` parse/dispatch、legacy flag正規化、known head/completion、Normal非昇格testを実装する。
3. **Tool composition**: `HUMAN_TASK`名、mode policy、順序保持dedup、`none` / `@exec` / Normal fail-closedを実装する。
4. **DTO/schema**: request/result/outcome、JSON Schema、trim・未知field・空element・64 KiB、result invariantをprotocol/domain unit testから実装する。
5. **ExecuteHumanTask vertical core**: fake launcher/observerで開始時requestのclone、呼出順序、done semantics、commands空、観測非fatalを緑にする。
6. **Briefing transport**: launch request拡張、versioned JSON一個、aish decode/renderer/env unset、旧handoff fallbackを実装する。
7. **Gate往復**: `HumanTaskGate` port、connection adapter、wire callback、相関ID全一致、duplicate/mismatch/decode/unavailable/cancelをfail-closedにする。
8. **Tool executor接続**: common definition/registryへexecutorを一度登録し、mode/gate/result validationを接続する。親agent継続までfake vertical testを緑にする。
9. **Collaborative instruction**: pure builderを既存`RequestContext.system_instruction`合成点へ追加し、NormalとCLI sourceに本文がないことを固定する。
10. **Phase 1 gate**: Phase 1の13 testsを本番assertionへ置換して`#[ignore]`を外し、targeted testと両checkerを通してから該当13 rowを`pending=false`にする。
11. **Phase 2**: structured errors、0055/0057/0060/0061/normal回帰、docs/help/manualを実装し、残り3 testsとregistryを緑にする。
12. **最終検証・完了処理**: targeted検証後に`./scripts/verify.sh`。全ACとverify成功後のみstatus=`done`、tasks→done、index実装済みへ更新する。

## 5. テスト方針と AC↔テスト対応

- CLI parser/unit: `clap_cli.rs` / `ask_invocation.rs`で正式subcommand、legacy flag、implicit ask、completionを検証する。
- Domain/unit: mode policy、allowlist順序、instruction合成、request正規化、64 KiB境界、result invariantをpure testにする。
- Protocol/client: callback request/result round trip、old payload互換、turn/tool/prompt ID mismatch・duplicate・decode failureをsocket pairで検証する。
- Application/unit: `ExecuteHumanTask`へrecording fake `HumanShellLauncher` / `EnvironmentObserver`を注入し、呼出順序と開始時request不変を検証する。
- Aish/integration: versioned JSON、固定ラベル、空section、ANSI/C0 escape、stderr、64 KiB、未知version、child env unset、旧表示fallbackを検証する。
- Vertical E2E: 既存mock aibe/Unix socket fixtureを拡張し、`ai collab`→tool公開→LLM `human_task`→fake Human Shell→0061 observation→tool result→次LLM roundを、`shell_exec`なしで通す。
- Regression: 0055/0057実PTYは直列・bounded timeoutで実行し、通常`ai ask`と通常`shell_exec`にHuman Task callbackやinstructionが出ないことを負のassertionで固定する。

各ACの代表testは §2 の同名関数とする。schemaの全境界を同名integration test一個へ詰めずmodule unit testへ分解してよいが、ACを`pending=false`にするには同名代表testが本番経路をassertし、関連unit/integration testも緑でなければならない。source文字列検索だけ、DTO直組みだけ、fake result返却だけでvertical ACを完了にしない。

## 6. エラー・安全契約の実装要点

- `invalid_arguments`: objective欠落/空、型不一致、未知field、空array element、正規化briefing >64 KiB。request本文をerrorへ複製しない。
- `tool_not_allowed`: Normal mode、forged allowlist。clientのtool一覧だけを信頼せず`ToolExecutionContext.execution_mode`で再検査する。
- `human_task_unavailable`: gateなし、相関ID不一致、decode失敗、矛盾result。別callを完了させない。
- `blocked`: cwd/runtime/shell/return marker等の既知lifecycle failure。安定codeの`HumanHandoffFailure`必須。
- `cancelled`:既存turn cancel/SIGINT/timeout cleanupを再利用し、自由文errorを複製しない。
- observation failure: task全体を失敗させず0061 `observation_errors`へ安定codeを残す。
- `done`: human control returnだけを意味し、command実行・criteria達成・自動検証済みを意味しない。

環境変数のversioned JSON、request、shell log、生のterminal入力を通常log/errorへ出さない。`AISH_HANDOFF_TASK_JSON`はshell起動直後に読み、子shellへ残さない。

## 7. ドキュメント更新

- `docs/architecture.md`: `ExecutionMode`、mode policy、`human_task` definition/registry/executor、ai↔aibe同一接続callback、ai→aish versioned briefing JSON、ExecuteHumanTaskと既存ports/observerの依存方向を追記する。
- `docs/security.md`: Normal fail-closed、相関ID検証、64 KiB、未知field/version拒否、envのunsetと非logging、errorへの秘密値非包含を追記する。
- `docs/manual/0062_collab-mode-human-task-tool.md`: `ai collab`正式導線、`--collaborative`互換、`--tools none`と`@exec`併用、表示section、終了後追加入力なし、親agent再観測を手動確認する。実PTY最終確認は人間が行い、未実施なら報告する。
- `docs/manual/README.md`: 0062 manualを一覧へ追加する。
- CLI help /既存docsから「Collaborative Modeには`--tools @exec`が必須」という説明を削除し、`@exec`は`human_task`を含まないと明記する。
- `docs/0000_spec-index.md`: 実装中はtasks rowを維持し、全AC完了後だけ実装済みへ変更する。

## 8. STOP-THE-LINE / scope外

次が必要なら0062へ追加しない: 新しいsocket/process boundary、永続task/resume/list/history、parallel/nested Human Task、side agent、Human Shell内`ai ask`、manual summary/status/reason、GUI/screenshot、自動criteria検証、ownership/lease/heartbeat/reconciler/exactly-once、独自PTY/observer/range pairing、新クレート、旧`shell_exec` interception削除。

レビューで見つかった便利機能や追加hardeningは分類を記録する。`NEW_REQUIREMENT` / `HARDENING` / `OUT_OF_FAULT_MODEL`は0062の完了条件へ昇格させない。

## 9. 完了条件

1. 全16行の`spec-acceptance.toml`が`pending = false`で、対応testに`#[ignore]`がない
2. Phase 1 vertical E2EとPhase 2回帰・docs testが成功
3. `./scripts/check-feature-scope.py`と`./scripts/check-spec-acceptance.py`が成功
4. `./scripts/verify.sh`が成功し、`.verify-timing-last`のsummaryを報告
5. architecture/security/manual/helpが本番挙動と同期
6. 上記完了後だけ本書を`docs/done/`へ移し、scope statusとindexを`done` / 実装済みに更新

## 10. 仕様との差分（意図的に縮小する場合のみ）

なし。

設計書のNon-goals / Complexity budgetを超える変更は、本節へ追記して済ませずSTOP-THE-LINEとscope revision再判定を行う。
