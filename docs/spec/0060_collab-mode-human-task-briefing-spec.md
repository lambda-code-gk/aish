# 0060 Collab Mode Human Task Briefing 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **関連**: [`docs/feature-development-policy.md`](../feature-development-policy.md)、[`0045_pack-composition-spec.md`](0045_pack-composition-spec.md)、[`0055_minimal-human-handoff-spec.md`](0055_minimal-human-handoff-spec.md)、[`0057_pty-process-cleanup-hardening-spec.md`](0057_pty-process-cleanup-hardening-spec.md)

## 0. Core outcome

Collaborative Mode で Human Shell が開いた直後に、ユーザーが目的、handoff の固定理由、最初の候補操作、親エージェントへ戻るタイミングを安全な briefing から即座に理解できる。

## 1. Minimum vertical slice

```text
親 collaborative agent の shell_exec
→ 既存環境変数で aish human-shell を同期起動
→ print_handoff_briefing が環境変数を読み取る
→ render_human_task_briefing が安全な固定形式を生成する
→ stderr に Human Task briefing を表示する
→ ユーザーが操作する、または何も入力せず Ctrl+D / exit する
→ 0055 の既存経路でただちに親 agent へ制御を返す
```

### 1.1 実装境界

主変更は `aish/src/human_shell.rs` に限定する。briefing の文字列生成は I/O と環境変数に依存しない純粋関数 `render_human_task_briefing(parent_request, suggested_command)` に分離する。`print_handoff_briefing` は `AISH_HANDOFF_PARENT_REQUEST` と `AISH_HANDOFF_SUGGESTED_COMMAND` の読み取り、および renderer の返値を stderr へ出力することだけを担う。

既存の `AISH_CONTROL_MODE` と `AISH_HANDOFF_RUNTIME_DIR` は handoff 起動・実行の既存用途のまま維持する。新しい protocol field、shell_exec schema、環境変数、永続 task state は追加しない。

### 1.2 表示契約

表示は次の形式を厳守する。

```text
AISH Collaborative Mode
=======================

Human Task

Objective:
  <parent request>

Why this is a Human Task:
  The parent agent requested a shell operation in Collab Mode.
  AISH has not automatically executed the requested command.

Suggested first action:
  <suggested command>

Done when:
  Return control after you have completed the necessary work,
  or when the parent agent should re-observe the environment
  and decide the next step.

You remain in control:
  Edit, run, replace, or ignore the suggested command.
  Alt+. or Alt+, inserts the suggested command.
  Press Ctrl+D or run `exit` to return control.
```

未設定、または trim 後に空となる Objective は `No parent request summary is available.`、未設定、または trim 後に空となる Suggested first action は `No command was provided.` と表示する。

Objective と Suggested first action の複数行入力は、どちらも論理行ごとに2空白でインデントし、各論理行へ個別に `escape_for_handoff_display` 相当の無害化を適用する。論理改行は構造として保持し、複数行文字列全体へ同関数を適用して改行を `\\n` という文字列へ変換してはならない。各行内の ESC / OSC、CR、TAB、その他 C0 制御文字を terminal 制御として解釈されない形に無害化する。候補コマンドを実行済みまたは完了済みとは扱わない。表示理由は上記の固定された事実だけとし、要求内容からもっともらしい理由を推測しない。

### 1.3 未マージ outcome 案の扱い

origin/main に存在しない終了後の `done` / `blocked` / `cancelled` 選択や summary 入力は導入しない。Ctrl+D または `exit` で Human Shell が終了した後は、追加入力なしで既存の同期 handoff 経路から親エージェントへただちに制御を返す。

新規 protocol field は追加しない。特に `HumanHandoffResult` へ `collab_outcome` field や関連 enum / struct を追加せず、origin/main の schema を維持する。exit code、return marker、表示内容などから推定した status も埋め込まない。

## 2. Fault model

### 2.1 保証対象

標準 Fault Model に従う。加えて、環境変数が未設定または空でも固定 fallback を含む briefing を表示し、表示対象に ANSI / C0 制御文字が含まれても terminal 制御として実行されないことを保証する。stderr 出力失敗は既存の best-effort briefing と同様に Human Shell 起動契約を変更しない。

### 2.2 保証対象外

- briefing 内容の永続化、再送、クラッシュ後の復元
- 親要求または候補コマンドの正しさ・安全性・完了条件の自動検証
- 非同期または複数 Human Task の管理
- terminal の表示幅に応じた折り返し最適化、翻訳、装飾テーマ

## 3. Non-goals

- 新しい `human_task` tool、Human Task domain aggregate、永続 task
- side agent、別 agent loop、ownership、lease、heartbeat、reconciler
- 作業結果 status、summary、理由、履歴の手入力または自動収集
- 候補コマンドの自動実行、実行済み・成功済みという推定
- 親要求から handoff 理由または完了条件を推測すること
- 新 protocol field、新 shell_exec request/response、新環境変数、新設定、外部依存
- 0055 の同期 Human Shell lifecycle または 0057 の cleanup の再設計
- 通常 shell_exec、非 Collaborative Mode の挙動変更

## 4. Complexity inventory

| 項目 | 数・内容 |
|------|----------|
| 実行主体 | 1（既存 `aish human-shell` 同期処理。新規主体なし） |
| 状態機械 | 0 |
| 永続 aggregate | 0 |
| 外部副作用 | 1（既存 stderr への briefing 出力） |
| プロセス境界 | 1（既存 `ai` → `aish human-shell` handoff。新規境界なし） |
| 新規基盤機構 | 0（純粋 renderer の抽出のみ） |
| 他機能統合 | 1（0055/0057 の既存 handoff 経路） |

`scripts/feature-scope.toml` の `0060` entry と一致させる。

## 5. Complexity Gate

- 判定: Green
- 理由: 既存同期 Human Shell の開始時 stderr 表示を純粋 renderer 中心に置換するだけであり、新しい実行主体、状態機械、永続化、protocol、process boundary、外部依存を導入しない
- 分割判断: briefing 表示と終了後の即時 return UX だけを扱い、task 管理・結果収集・非同期協調は Deferred specs へ送る
- 承認例外: なし

## 6. Complexity budget

| 項目 | 上限 |
|------|------|
| 新規実行主体 | +0 |
| 状態機械 | +0 |
| 永続 aggregate | +0 |
| agent loop / side agent | +0 |
| process boundary | +0 |
| external effect | +0（既存 stderr 出力のみ） |
| protocol / schema / env | 新規 +0（既存 schema を変更しない） |
| 新規基盤機構 / 外部依存 | +0 |

## 7. Split triggers

次が必要になったら STOP-THE-LINE し、0060 に追加せず別 spec へ分割する。

- 新しい `human_task` tool または独立 task aggregate
- status、summary、理由、作業履歴の入力または収集
- 永続化、resume、schema migration、crash recovery
- side agent、二つ目の agent loop、複数 task coordination
- lease、heartbeat、reconciler、exactly-once
- 新 protocol field、shell_exec schema、環境変数、設定
- Human Shell lifecycle、PTY cleanup、通常 shell_exec の変更

## 8. パック構成の適用

**No** — 0045 §6 の適用候補条件に該当せず、さらに変更主体の `aish` は同仕様でパック構成の対象外と明記されている。本機能は全 Collaborative Mode handoff で必要な開始時 briefing を単一の純粋関数と既存 stderr adapter に閉じる軽量な core UX であり、runtime toggle、専用 RPC / CLI、重い依存、optional 配備は不要である。Pack 境界、Active Pack、Basic Pack は作らない。

## 9. Acceptance Criteria

| ID | 条件 |
|----|------|
| `human_task_briefing_renders_collaborative_mode_header` | Human Shell 開始時の stderr briefing が厳密な先頭 `AISH Collaborative Mode`、下線、`Human Task` を表示する |
| `human_task_briefing_renders_objective` | `Objective:` に `AISH_HANDOFF_PARENT_REQUEST` の内容を表示し、未設定または trim 後が空なら `No parent request summary is available.` を表示する |
| `human_task_briefing_uses_fixed_reason` | `Why this is a Human Task:` に親 agent が Collab Mode で shell operation を要求したという固定文だけを表示し、要求内容から理由を推測しない |
| `human_task_briefing_renders_suggested_first_action` | `Suggested first action:` に `AISH_HANDOFF_SUGGESTED_COMMAND` の内容を表示し、未設定または trim 後が空なら `No command was provided.` を表示する |
| `human_task_briefing_states_command_not_executed` | AISH が requested command を自動実行していないことを明示し、候補コマンドを実行済み・成功済みとして扱わない |
| `human_task_briefing_preserves_user_control` | `You remain in control:` に suggested command を edit、run、replace、ignore できることと Alt+./Alt+, の挿入操作を厳密に表示する |
| `human_task_briefing_renders_done_when` | `Done when:` に必要作業完了時または親 agent が再観測して次を判断すべき時に返すという固定文を厳密に表示する |
| `human_task_briefing_returns_with_ctrl_d_or_exit` | briefing が Ctrl+D と `exit` の双方を案内し、`exit` 経路で追加入力なしにただちに親 agent へ制御を返す（Ctrl+D の実 PTY 回帰は 0055 E2E が正本） |
| `human_task_briefing_indents_multiline_objective` | 複数行 Objective と Suggested first action を論理行ごとに2空白でインデントし、論理改行を保持して `\\n` 文字列へ変換しない |
| `human_task_briefing_sanitizes_ansi_and_c0` | Objective と Suggested first action の各論理行を個別に escape し、行内の ESC / OSC、CR、TAB、その他 C0 制御文字を terminal 制御として解釈されない形に無害化する |
| `human_task_briefing_renderer_is_pure` | `render_human_task_briefing` は引数だけから文字列を生成し、環境変数読取や I/O を行わない |
| `human_task_briefing_printer_only_reads_env_and_stderr` | `print_handoff_briefing` は既存 parent request / suggested command 環境変数の読取と renderer 結果の stderr 出力だけを担う |
| `human_task_briefing_has_no_outcome_selection` | Human Shell 終了後に `done` / `blocked` / `cancelled` 選択を表示せず要求しない |
| `human_task_briefing_has_no_summary_input` | Human Shell 終了後を含む handoff 全体で summary または理由の手入力を要求しない |
| `human_task_briefing_adds_no_protocol_schema` | `collab_outcome` を含む新しい protocol field、request、response、tool を追加せず、`HumanHandoffResult` の serialize JSON に同 key がない |
| `human_task_briefing_uses_only_existing_env` | `AISH_HANDOFF_PARENT_REQUEST`、`AISH_HANDOFF_SUGGESTED_COMMAND`、`AISH_HANDOFF_RUNTIME_DIR`、`AISH_CONTROL_MODE` 以外の handoff 環境変数を追加しない |
| `human_task_briefing_creates_no_persistent_state` | Human Task、briefing、outcome の永続 task state、履歴、resume 情報を作成しない |
| `human_task_briefing_normal_shell_exec_regression` | 通常 shell_exec と非 Collaborative Mode の実行・承認挙動が変わらない |

各 row は Scope Lock とともに `scripts/spec-acceptance.toml` と 1:1 に固定する。

### 9.1 完了ゲート（製品 AC 外）

- 0055 Minimal Human Handoff と 0057 PTY Process Cleanup Hardening の既存関連テストが成功する。
- workspace の `cargo test -j 1` と最終 `./scripts/verify.sh` が成功する。

## 10. Deferred specs

- 独立 `human_task` tool / Human Task domain model
- task、outcome、summary、履歴の永続化または自動収集
- side agent、複数 task coordination、remote collaboration
- 作業完了条件の自動検証

## 11. Scope change log

| Revision | 分類 | 変更 | 理由 |
|----------|------|------|------|
| 1 | INITIAL | Human Shell 開始時の固定 Human Task briefing と、終了後 outcome / summary 手入力の除去を設計確定 | ユーザーが handoff の目的と返却タイミングを即座に理解し、Ctrl+D / `exit` だけで親へ戻れる同期 UX に固定するため |
| 2 | CONTRACT | 終了後 outcome 手入力を撤回し、trim 空値、両複数行 field の行単位 indent / escape、製品 AC と完了ゲートの分離を明確化。`collab_outcome` など新規 protocol field は追加しない | 手入力を完全に除去しつつ新規 protocol field や推定 status を導入せず、安全な表示契約と Scope Lock を一致させるため |
| 3 | COMPLETE | 全 18 AC が緑、実装指示書を `docs/done/` へ移動し `status = done` | 受け入れ条件と Scope Lock の完了状態を registry と正本で一致させるため |
| 4 | CONTRACT | origin/main にない `collab_outcome` schema を完全削除し、Ctrl+D の疑似 pipe 検証を 0055 実 PTY E2E に委ねる | 未マージ機能を互換契約として固定せず、0060 AC を実際の検証責務に一致させるため |
