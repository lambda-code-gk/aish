# 0062 Collaborative Mode Human Task Tool 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **関連**: [`docs/feature-development-policy.md`](../feature-development-policy.md)、[`0045_pack-composition-spec.md`](0045_pack-composition-spec.md)、[`0055_minimal-human-handoff-spec.md`](0055_minimal-human-handoff-spec.md)、[`0060_collab-mode-human-task-briefing-spec.md`](0060_collab-mode-human-task-briefing-spec.md)、[`0061_collab-mode-human-task-evidence-spec.md`](0061_collab-mode-human-task-evidence-spec.md)

## 0. Core outcome

ユーザーが `ai collab "依頼内容"` で Collaborative Mode を正式に起動し、LLM が `shell_exec` と独立した `human_task` tool で人間へ明示的に作業を委譲して、自動観測付き結果を同じ親エージェントへ返せる。

## 1. Minimum vertical slice

```text
ai collab "依頼内容"
→ 既存 ask のプロンプト解決
→ ExecutionMode::Collaborative
→ 通常の明示 tools を保持したまま human_task を自動公開
→ Collaborative Mode 専用 system instruction を合成
→ LLM が human_task(objective, reason?, instructions[], completion_criteria[]) を呼ぶ
→ HumanTaskTool が同一接続の HumanTaskGate へ実行要求を送る
→ ai の ExecuteHumanTask が開始時コンテキストを構築
→ 0055/0057 の同期 Human Shell を開始して待機
→ 0060 の開始時 briefing を表示
→ Human Shell 終了後に 0061 の PostHandoffObservation を収集
→ HumanTaskResult(status, task, observation, lifecycle metadata) を tool result として返す
→ 同じ親エージェントが継続する
```

### 1.1 CLI と `ExecutionMode`

起動モードの正本は boolean ではなく、`ai` domain の次の値とする。

```text
ExecutionMode
  Normal
  Collaborative
```

CLI 契約は次で固定する。

| 起動 | mode | 備考 |
|------|------|------|
| `ai collab "..."` | Collaborative | 正式導線 |
| `ai "..."` | Normal | 既存 implicit ask |
| `ai --collaborative "..."` | Collaborative | 互換導線。parse 後に同じ mode へ変換 |
| `ai collab --tools @exec "..."` | Collaborative | 明示した `shell_exec` を保持し、別途 `human_task` を公開 |

`AiCommand::Collab` は `Ask` と同じ `TurnOptions`、`--file`、message を受け、dispatch 後は既存 `run_ask` と既存プロンプト解決を再利用する。Collab 専用 editor、stdin reader、message join、空入力判定は作らない。`collab` を既知 CLI head として implicit `ask` 挿入対象から除外し、completion にも正式サブコマンドとして出す。

`--collaborative` は削除せず、どの互換 ask 起動でも `ExecutionMode::Collaborative` へ正規化する。依頼文の語彙、`human_task` 呼び出しの有無、呼び出し後の状態から mode を推測または昇格しない。

### 1.2 Tool 公開とレジストリ構成

`human_task` は組み込み tool name、schema、executor を既存の tool registry / tool definition 経路へ一度だけ登録する。LLM へ渡す allowlist は通常の `resolve_tools` の結果を基礎に mode policy で構成する。

- Collaborative Mode: 明示・設定由来の tools を順序保持で残し、`human_task` を重複なく自動追加する
- Normal Mode: `human_task` を公開しない。CLI から名前を明示しても fail-closed とする
- `--tools none` を Collaborative Mode で指定しても、通常 tools は空のまま `human_task` だけを公開する
- `@exec` は `shell_exec` だけを表す既存カテゴリのままとし、`human_task` を混ぜない
- `human_task` は Collaborative Mode 専用のため、通常設定の `ask_tools` や `@full` へ暗黙追加しない

`ToolRoundExecutor` の tool 名ごとの個別追加、CLI dispatch 各所での ad hoc push、`ShellExecTool` 内の別名分岐は禁止する。server 側も `ToolExecutionContext.execution_mode` を検査し、Normal Mode の forged allowlist を `tool_not_allowed` で拒否する。

### 1.3 `human_task` schema

LLM tool schema と domain request は次の一つの語彙を使う。

```text
HumanTaskRequest
  objective: String                # 必須、trim 後非空
  reason: Option<String>           # 任意
  instructions: Vec<String>        # 任意、省略時は空 list
  completion_criteria: Vec<String> # 任意、省略時は空 list
```

JSON Schema は `objective` だけを `required` とし、`instructions` / `completion_criteria` は string array とする。未知 field は受理せず、型不一致、trim 後空の `objective`、空文字または trim 後空の array element、および正規化後の versioned briefing JSON が 64 KiB を超える request は `invalid_arguments` とする。trim 後空の `reason` は未指定へ正規化し、`instructions` と `completion_criteria` の未指定は空 list へ正規化する。LLM が情報を捏造して埋めることを要求しない。request 内容は開始時作業情報の正本であり、終了後に書き換えない。

### 1.4 `ExecuteHumanTask` ユースケース

`ExecuteHumanTask` は `ai` application 層に置き、次の順序だけを担う。

1. `HumanTaskRequest` と turn の client cwd を受け取る
2. objective / reason / instructions / completion criteria から構造化 `HumanTaskBriefing` を構築する
3. 既存 `HumanShellLauncher::launch_and_wait` を同期実行する
4. 既存 `EnvironmentObserver::observe` で終了直後を観測する
5. 開始時 request、終了状態、観測、既存 lifecycle metadata から `HumanTaskResult` を構築する

本ユースケースは新しい PTY launcher、observer、agent loop、永続 task aggregate を持たない。`AishHumanShellLauncher`、cleanup / cancel 処理、runtime dir guard、termios guard、`ProcessEnvironmentObserver` を再利用する。テストでは既存 `HumanShellLauncher` と `EnvironmentObserver` port の fake を注入し、実 shell を起動せず request → result を検証する。

`HumanShellLaunchRequest` は既存の `parent_request_summary` / `suggested_command` 経路を壊さず、任意の構造化 `HumanTaskBriefing` を追加で受け取れるよう拡張する。明示 `human_task` 経路はこの構造化値を使い、旧 `shell_exec` handoff は従来の2文字列だけを使う。outbound launcher は構造化値を一つの versioned JSON として `AISH_HANDOFF_TASK_JSON` へ渡し、`aish human-shell` は 64 KiB 以下で decode できた場合だけ新表示モデルを選ぶ。欠落時は0060の旧表示へ戻し、不正 JSON、未知 version、上限超過は Human Shell を開始せず `blocked` にする。個別 field ごとの環境変数は追加しない。値は shell 起動直後に読み取って子 shell 環境から unset し、ログおよび error message へ出さない。

0060 の「既存 handoff 環境変数だけ」という回帰条件は旧経路の正本として維持しつつ、0062 は上記の構造化値1個だけを明示的に追加する。実装時に `HANDOFF_ENV_KEYS`、shell completion の unset、architecture / security docs、0060 回帰テストの期待値を同時に更新する。これは新しい process boundary や renderer ではなく、既存 Human Shell 起動境界へ briefing model を運ぶための拡張である。

### 1.5 Protocol 往復

`HumanTaskTool` は `aibe` 側の executor であり、対話 terminal を所有しない。既存 Unix socket の同一 turn 接続に専用 `HumanTaskGate` を設ける。`HumanTaskGate` は新しい process boundary や汎用 protocol subsystem ではなく、`human_task` tool の同期 callback adapter であり、`human-task-tool-gate` として一つの novel mechanism に含める。

```text
HumanTaskTool
→ HumanTaskGate::execute_human_task(turn_id, tool_call_id, prompt_id, HumanTaskRequest)
→ ClientResponse::HumanTaskExecutionRequest(turn_id, tool_call_id, prompt_id, request)
→ ai callback
→ ExecuteHumanTask
→ ClientRequest::HumanTaskExecutionResult(turn_id, tool_call_id, prompt_id, result)
→ HumanTaskResult を通常の tool result JSON に変換
```

新しい socket、RPC server、agent loop は作らない。専用 request / response は shell 実行承認ではないため、`ShellExecApprovalPrompt`、`ShellExecApprovalDecision`、`ShellExecApprovalOrigin` を流用しない。gate が同一接続上で生成した `prompt_id` を一つの待機中 call に束縛し、turn id、tool call id、prompt id の全一致を既存 connection gate と同様に検証する。重複 response、不一致、decode 失敗、待機中 call がない response は fail-closed とし、その response で別 call を完了させない。

### 1.6 表示

0060 の Human Task briefing renderer、行単位 indentation、ANSI / C0 escape、stderr 表示を拡張して再利用し、別 renderer を並立させない。明示 `human_task` では `Objective:` に `objective`、`Why this is a Human Task:` に `reason`、`Suggested actions:` に `instructions` の各要素、`Done when:` に `completion_criteria` の各要素を表示する。これは 0060 の既存ラベル語彙と表示モデルを拡張するものであり、`Reason:` / `Instructions:` / `Completion criteria:` という二重の表示契約は作らない。`reason` が未指定、または各 list が空なら対応セクション自体を表示せず、fallback や推測内容で埋めない。`Objective:` は必須のため常に表示する。

0055 の旧 `shell_exec` handoff は既存 parent request / suggested command briefing を維持できる。新経路は suggested command を生成せず、`instructions` を command として扱わず、自動実行もしない。終了後の status / summary 入力は追加しない。

### 1.7 結果と終了状態

```text
HumanTaskResult
  status: HandoffExecutionOutcome
  task: HumanTaskRequest
  human_shell_exit_code: Option<i32>
  final_shell_cwd: Option<String>
  shell_log_range: Option<ShellLogRange>
  observation: Option<PostHandoffObservation>
  error: Option<HumanHandoffFailure>
```

第1段階の既存 `HandoffExecutionOutcome` を終了状態の正本として再利用し、`Done` / `Blocked` / `Cancelled` variant を追加する。既存 `HumanControlReturned` は旧 `HumanHandoffResult.execution_outcome` の wire 互換として残し、新しい同義 status enum は作らない。

| wire status | 構築条件 | 意味 |
|-------------|----------|------|
| `done` | Human Shell が正常 return marker とともに終了 | 人間から制御が戻った。要求作業の自動検証済みを意味しない |
| `blocked` | cwd / runtime dir / shell 起動 / normal return marker 等の理由で Human Task を完遂できる shell lifecycle を成立させられない | 機械的な handoff 阻害。人間に理由入力を要求しない |
| `cancelled` | turn cancellation、SIGINT、timeout により既存 cleanup 経路で中止 | ユーザーまたは親 turn により中止 |

`task` は開始時 request の clone、`observation` は 0061 の `PostHandoffObservation` を優先してそのまま格納する。GUI 作業など Human Shell 内で command を実行しなかった場合、`HumanTaskEvidence.commands` が空でも正常な `done` である。command exit code、git status、shell exit codeから status を推定しない。`done` は自動検証、要求達成、成功を保証しない。

結果の構築不変条件は次で固定する。`done` では `error = None` かつ正常 return marker を持つ。`blocked` では安定 code の `error = Some(HumanHandoffFailure)` を必須とし、成立しなかった lifecycle の metadata / observation は取得できた値だけを残す。`cancelled` では `error = None` とし、取消理由を自由文 error へ複製しない。これらに反する client result は wire decode 後の domain validation で `human_task_unavailable` として拒否し、親へ矛盾した structured result を渡さない。

### 1.8 `shell_exec` との責務分離

```text
shell_exec  → ShellExecTool → AISH が subprocess command を実行
human_task  → HumanTaskTool → ExecuteHumanTask → 人間が Human Shell で作業
```

`human_task` は `ShellExecTool` の別名、引数、approval mode、特殊 return ではない。新経路は `@exec`、shell command allowlist、`ShellExecApprovalGate`、`ShellExecApprovalMode`、candidate command に依存しない。Collaborative Mode だから全 `shell_exec` を Human Shell へ変換する新規ロジックも追加しない。

0055 の旧 `collaborative_handoff` による `shell_exec` interception は互換のため残してよいが、`HumanTaskTool` / `ExecuteHumanTask` から旧 interception を呼ばない。将来の旧経路削除は本 spec の完了条件に含めない。

### 1.9 Collaborative Mode 用 LLM instruction

Collaborative Mode 専用 instruction は `ai` domain/application の純粋な prompt builder に置き、CLI parser / dispatch / `main.rs` に本文を直書きしない。既存 console hint / replay manifest と同じ `RequestContext.system_instruction` 合成点で、mode が Collaborative のときだけ追加する。

instruction は少なくとも次を伝える。

- 人間への作業委譲には `human_task` を明示的に使う
- AISH 自身が許可済み command を実行する場合は `shell_exec` を使い、両者を混同しない
- `objective` は具体的に必須とし、任意 field は不明なら空または省略できる
- `done` や command Evidence を自動検証済みとみなさず、必要なら返却後に環境を再観測する
- Human Task を並列・入れ子にしない

Normal Mode の system instruction にはこの本文を入れない。依頼文解析や tool call 後の mode 変更にも使わない。

### 1.10 エラー契約

| code / status | 条件 | 親 turn |
|---------------|------|---------|
| `invalid_arguments` | schema 型不一致、objective 欠落・空 | error tool result で継続 |
| `tool_not_allowed` | Normal Mode または forged allowlist | error tool result で継続 |
| `human_task_unavailable` | interactive client / HumanTaskGate がない、wire 不一致 | error tool result で継続 |
| `blocked` + `HumanHandoffFailure` | cwd、runtime dir、shell 起動、return marker の既知失敗 | structured HumanTaskResult で継続 |
| `cancelled` | turn cancel / SIGINT / timeout | structured HumanTaskResult を返せる場合は返し、既存 turn cancel が先に確定した場合はその契約を維持 |

観測だけの失敗は 0061 と同様に task 全体を失敗させず、`observation_errors` に安定 code を残す。秘密値、未加工 shell log、手入力 summary を error message へ含めない。

### 1.11 再利用方針と実装時ドキュメント

| 段階 / 現行要素 | 再利用 |
|-----------------|--------|
| 0055 / 0057 | `HumanShellLauncher`、`AishHumanShellLauncher`、`HumanShellReturn`、cleanup / cancel、runtime / termios guard |
| 0060 | Human Task briefing renderer、escape、stderr 表示。explicit request field を同じ表示モデルへ追加 |
| 0061 | `PostHandoffObservation`、`HumanTaskEvidence`、`ProcessEnvironmentObserver`、bounded range / replay span |
| tool 基盤 | `ToolName`、`ToolDefinition`、`ToolExecutor`、`DefaultToolRegistry`、`ToolExecutionContext`、connection gate pattern |
| prompt | `RequestContext.system_instruction` と既存合成点 |

実装時は `docs/architecture.md` の tool / client callback と Collaborative Mode 節、関連 `docs/manual/`、CLI help を同期する。`ai collab` を正式導線に変更し、`--tools @exec` が Collaborative Mode の必須条件である説明を削除する。旧 `--collaborative` は互換導線として記載する。

## 2. Fault model

### 2.1 保証対象

標準 Fault Model に従う。単一ホスト・単一ユーザー・正常な `ai` / `aibe` process 生存中に、同一 turn で一つの同期 Human Task を開始し、その終了状態と best-effort 自動観測を親エージェントへ返す。

### 2.2 保証対象外

- process crash / OS restart 後の Human Task 再開または結果再送
- 複数 Human Task の同時実行、順序調停、exactly-once
- GUI 操作や要求達成の自動観測・自動検証
- Human Shell 外で行われた操作の完全な attribution
- protocol schema migration または旧 server / 新 client 混在の機能提供

## 3. Non-goals

- Human Task の永続化、resume、一覧、履歴、検索
- 並列 Human Task、入れ子 Human Task、side agent、別 agent loop
- Human Shell 内の `ai ask`
- completion criteria の自動検証、LLM による status 判定
- GUI、スクリーンショット、画面認識
- ownership、lease、heartbeat、reconciler、journal、idempotency key
- manual summary、終了後 status / reason 入力
- 新クレート追加を前提とする大規模分割
- 0055 / 0060 / 0061 の再実装
- 旧 `shell_exec` interception の削除

## 4. Complexity inventory

| 項目 | 数・内容 |
|------|----------|
| 実行主体 | 2（既存 aibe tool loop、既存 ai 同期 Human Shell 実行） |
| 状態機械 | 0（同期 call stack。永続・独立状態機械なし） |
| 永続 aggregate | 0 |
| 外部副作用 | 2（既存 Human Shell process、既存 log / cwd / git の終了後観測） |
| プロセス境界 | 2（既存 ai ↔ aibe socket、既存 ai → aish human-shell） |
| 新規基盤機構 | 1（独立 `human_task` tool + 同一接続 gate） |
| 他機能統合 | 3（0055/0057 handoff、0060 briefing、0061 Evidence） |

`scripts/feature-scope.toml` の `0062` entry と一致させる。

## 5. Complexity Gate

- 判定: **Yellow（承認済み）**
- 理由: 新規実行主体、状態機械、永続化、agent loop は追加しないが、既存の二つの process boundary を通り、0055/0057・0060・0061 の3機能を独立 `human_task` tool に統合するため Yellow 閾値に達する
- 分割判断: 新規 novelty を `human_task` tool / gate の一つに限定し、永続化、resume、並列、入れ子、side agent、自動検証、GUI、ownership は Deferred へ送る
- 承認例外: 不要（Red ではない）。`scope_review = "approved"` を registry に記録する

## 6. Complexity budget

| 項目 | 上限 |
|------|------|
| 新規実行主体 | +0 |
| 状態機械 | +0 |
| 永続 aggregate | +0 |
| process boundary / socket | +0（既存2境界・既存socketのみ） |
| novel mechanism | +0（`human_task` tool + gate の1件で固定） |
| agent loop / side agent | +0 |
| external effect | +0（既存 Human Shell と観測のみ） |
| 新クレート | +0 |

## 7. Split triggers

次が必要になったら STOP-THE-LINE し、0062 へ追加せず scope revision と Complexity Gate を再判定して別 spec へ分割する。

- 新しい実行主体、状態機械、agent loop、process boundary、socket
- Human Task の永続化、resume、一覧、schema migration、crash recovery
- 並列、入れ子、side agent、ownership、lease、heartbeat、reconciler、exactly-once
- Human Shell 内 `ai ask`、自動検証、GUI / screenshot 観測
- manual summary または終了後 status / reason 入力
- 0055 / 0060 / 0061 の置換、独自 PTY / observer / command span pairing
- 新クレートを要する大規模分割

## 8. パック構成の適用

**No** — Collaborative Mode は optional 配備や runtime config で脱着する機能ではなく、`Normal` / `Collaborative` を request ごとに明示する core 実行モードである。専用 CLI と tool は持つが、重い依存のリンク除外、Active / Basic Pack の差し替え、独立 RPC bundle は不要であり、既存 tool registry の mode-dependent allowlist で責務を満たす。したがって Pack 境界、Active Pack、Basic Pack、Cargo feature は追加せず、Normal Mode での非公開・拒否テストを fail-closed 契約として置く。

## 9. Acceptance Criteria

| ID | 条件 |
|----|------|
| `collab_cli_selects_collaborative_mode` | `ai collab "依頼内容"` が既存 ask のプロンプト解決を再利用して `ExecutionMode::Collaborative` を選び、`ai "依頼内容"` は `Normal` のままである |
| `collab_legacy_flag_maps_to_same_mode` | 旧 `ai --collaborative "依頼内容"` が同じ Collaborative Mode へ変換され、依頼文や tool call から mode を推測しない |
| `collab_preserves_explicit_tools_without_exec_requirement` | `ai collab` は `--tools @exec` なしで起動でき、`ai collab --tools @exec` 等の明示 tools を保持しつつ `human_task` を追加する |
| `human_task_is_published_only_in_collaborative_mode` | tool registry / definition の共通構成点が `human_task` を Collaborative Mode だけで LLM へ公開し、Normal Mode と forged allowlist は fail-closed になる |
| `human_task_schema_matches_request_contract` | schema が必須非空 objective、任意 reason、任意の string array である instructions / completion_criteria を持ち、空 reason を未指定、array 未指定を空 list へ正規化し、空 element、未知 field、型不一致、64 KiB 超の正規化 briefing を拒否する |
| `human_task_briefing_uses_task_labels_and_omits_empty_sections` | 0060 renderer を再利用し、`Objective:` / `Why this is a Human Task:` / `Suggested actions:` / `Done when:` に request を対応付け、未指定 reason と空 list のセクションは fallback なしで省略する |
| `execute_human_task_uses_existing_human_shell_ports` | fake `HumanShellLauncher` / `EnvironmentObserver` により、request 受付、構造化 briefing 構築、同期開始・待機、結果返却の順序を検証できる。明示 task は versioned JSON 1個で既存起動境界を通り、旧 handoff は従来表示を維持する |
| `human_task_result_reuses_status_and_observation_types` | `HumanTaskResult` が拡張した既存 `HandoffExecutionOutcome` の done/blocked/cancelled、開始時 `HumanTaskRequest`、0061 `PostHandoffObservation` と lifecycle metadata から構築され、status / error の排他不変条件に違反する payload を拒否する |
| `human_task_result_requires_no_manual_summary` | Human Shell 終了後に summary、reason、status、outcome の手入力を要求せず、GUI 作業で Evidence commands が空でも正常結果を返せる |
| `human_task_done_does_not_mean_verified` | done、shell exit code、command Evidence を要求達成または自動検証済みとして扱わず、command / git 状態から status を推定しない |
| `human_task_is_independent_from_shell_exec` | `HumanTaskTool → ExecuteHumanTask → Human Shell` が `ShellExecTool`、`@exec`、command allowlist、shell_exec approval に依存せず、Collaborative Mode の全 shell_exec を Human Shell 化する新規分岐がない |
| `collab_instruction_is_mode_scoped_and_not_in_cli` | Collaborative Mode 専用 LLM instruction を domain/application の builder で既存 system instruction 合成点へ追加し、Normal Mode と CLI parser / dispatch 本文には入れない |
| `human_task_errors_are_structured_and_fail_closed` | invalid schema / mode / gate / wire を安定 error code で拒否し、既知 lifecycle failure は blocked、取消は cancelled、観測失敗は非 fatal とする |
| `collab_human_task_vertical_with_fakes` | `ai collab` の mode / tool 公開から LLM の `human_task` call、fake Human Shell、automatic observation、structured tool result、親 agent 継続までが `shell_exec` なしで通る |
| `collab_human_task_preserves_prior_stage_regressions` | 0055/0057 cleanup、0060 briefing、0061 Evidence、通常 shell_exec、Normal Mode、明示 tools の既存テストが回帰しない |
| `collab_docs_use_official_entrypoint` | CLI help と docs が `ai collab` を正式導線、`--collaborative` を互換導線として示し、`--tools @exec` 必須説明を含まない |

各 row は実装開始時の Scope Lock で `scripts/spec-acceptance.toml` と 1:1 に固定し、対応する ignored test を先に追加する。本設計 step では `status = "draft"` とし、vertical slice AC の governance 用 pending test だけを先行登録する。

## 10. Deferred specs

- Human Task 永続化、resume、一覧、履歴、検索
- 複数 / 並列 / 入れ子 Human Task coordination
- side agent、Human Shell 内 `ai ask`
- completion criteria の自動検証、GUI / screenshot Evidence
- ownership、lease、heartbeat、reconciler、crash recovery
- 旧 collaborative `shell_exec` interception の撤去

## 11. Scope change log

| Revision | 分類 | 変更 | 理由 |
|----------|------|------|------|
| 1 | INITIAL | `ai collab`、独立 `human_task` tool、ExecuteHumanTask、既存 Human Shell / briefing / Evidence 再利用を設計確定 | ShellExec の特殊動作から人間への明示的作業委譲を分離し、正式な Collaborative Mode 導線を提供するため |
| 2 | CONTRACT | instructions / completion criteria を list 契約に修正し、表示ラベルと空 section 省略を固定。gate を tool と一体の同期 callback adapter と明記 | ユーザー要件、0060 の既存表示モデル、One Novelty Rule の整合を回復するため |
| 3 | CONTRACT | 構造化 briefing を既存 Human Shell 起動境界へ運ぶ versioned JSON 1個を固定し、callback 相関 ID と result の排他不変条件を明記 | list field を本番表示へ到達させ、同一接続 callback の取り違えと矛盾 result を元の縦断 AC 内で fail-closed にするため |
| 4 | GOVERNANCE | §9 の全16 ACを Scope Lock し、実装指示書・pending acceptance testを開始 | 設計契約を変更せず、実装開始時の受け入れ範囲を固定するため |
