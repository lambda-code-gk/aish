# 0059 Collaborative Mode Outcome（第1段階）設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **関連**: [`docs/feature-development-policy.md`](../feature-development-policy.md)、[`0045_pack-composition-spec.md`](0045_pack-composition-spec.md)、[`0055_minimal-human-handoff-spec.md`](0055_minimal-human-handoff-spec.md)、[`0057_pty-process-cleanup-hardening-spec.md`](0057_pty-process-cleanup-hardening-spec.md)

> **0060 による置換**: 本書は 0059 実装時点の契約を記録する。0060 は終了後の対話収集 UX を撤回し、既存 `HumanHandoffResult.collab_outcome` を optional 化して成功 handoff では省略する。新規 field や推定 status は追加しない。対話収集・必須 DTO 前提の AC は 0060 実装時に pending 化、削除、または 0060 AC へ差し替える。

## 0. Core outcome

Collaborative Mode の Human Shell 終了後に、ユーザーが `done` / `blocked` / `cancelled` を明示選択し、親エージェントが構造化された Collab Outcome（status のみ）として受け取れる。

## 1. Minimum vertical slice

```text
親 collaborative agent の shell_exec
→ 0055 の Human Shell を起動
→ ユーザーが Human Shell を終了
→ 親 terminal の termios を復元
→ status を選択（追加入力なし）
→ ai application が CollabOutcome を HumanHandoffResult に統合
→ ShellExecApproval.handoff_result
→ aibe ShellExecTool の JSON synthetic tool result
→ 同じ親 agent が status を認識して継続
```

### 1.1 0055 経路への接続点

既存経路は `ai/src/main.rs` の collaborative `shell_exec` handler から `RunSynchronousHumanHandoff::execute()` を呼び、`ai/src/application/human_handoff.rs` が再観測までを行い、`ShellExecApprovalDecision.handoff_result` と `aibe-protocol::ClientRequest::ShellExecApproval.handoff_result` を経て、`aibe/src/adapters/outbound/tools/shell_exec.rs` が DTO 全体を JSON 化した synthetic tool result を返す。

0059 では次の薄い拡張だけを行う。

- Domain: `ai` の純粋 domain に `CollabOutcomeStatus { Done, Blocked, Cancelled }` と `CollabOutcome { status }` を置く。status parse に I/O を含めない。**summary フィールドは持たない。**
- Port / Adapter: `ai` の outbound port に outcome 収集境界を追加し、terminal adapter が表示、対話 stdin 判定、status 再入力を担う。選択完了時点で即座に domain `CollabOutcome` を返す。summary の追加入力は行わない。
- Application / composition root: 既存 `RunSynchronousHumanHandoff` は責務と返値を変えず、正常 return marker 確認後の再観測までを行う。`ai/src/main.rs` の collaborative handler は service 成功後に `ParentTermiosGuard` を先に drop し、その後だけ collector を呼ぶ。薄い application mapper が domain outcome を protocol DTO へ変換し、既存 `HumanHandoffResult` に統合する。収集成功時のみ approval decision を success にする。
- Protocol / tool result: `aibe_protocol` に wire 用 `CollabOutcomeStatus` / `CollabOutcome { status }` を置き、`HumanHandoffResult` に必須の `collab_outcome` field を追加する。0059 実装後の collaborative success では常に存在し、非 collaborative 経路は `HumanHandoffResult` 自体を生成しない。`ShellExecApproval` の新しい分岐や独立 tool は作らず、既存 `handoff_result` と既存 JSON synthetic result 経路をそのまま利用する。

`ai` domain 型と `aibe_protocol` wire DTO は責務を分け、adapter から protocol 型を直接返さない。`aish` は outcome を収集・解釈・送信せず、変更対象としない。

`human_shell_exit_code` と `collab_outcome.status` は独立である。exit code から status を導出せず、親も表示文言や exit code から結果を推測しない。

### 1.2 対話 UX

Human Shell の正常終了後に terminal adapter は次を表示する。

```text
Human Shellを終了しました。
作業結果を選択してください。

  [d] done       作業を完了した
  [b] blocked    作業を完了できなかった
  [c] cancelled  作業を中止した

> 
```

status は `d` / `done`、`b` / `blocked`、`c` / `cancelled` を大文字小文字を無視して受け付ける。空入力およびその他の値（`complete`、`failed` を含む）は拒否して再入力する。**有効な status を選択した時点で追加入力なく親エージェントへ戻る。** summary / 理由の手入力欄は設けない（第1段階の責務はシェル終了と作業完了の分離のみ）。

## 2. Fault model

### 2.1 保証対象

標準 Fault Model に従う。加えて、単一の対話 stdin が利用可能で Human Shell が正常に制御を返した場合、ユーザーが有効な outcome を明示するまで結果を返さない。

- Human Shell 起動失敗は outcome 入力を表示せず、0055 の既存 `human_handoff_failed` tool error とする。`blocked` へ変換しない。
- stdin が非対話の場合は outcome を推定せず、`Cannot collect Collaborative Mode result because stdin is not interactive.` 相当の明示エラーを返す。
- 親中断時は 0057 を含む既存終了処理に従い、outcome の永続化や復旧を行わない。
- terminal adapter の EOF / 入力エラーは handoff outcome 収集失敗として扱い、暗黙の `done` / `blocked` / `cancelled` を生成しない。この失敗は `ShellExecApprovalDecision.handoff_error` の既存 `human_handoff_failed` 経路で親へ返し、`handoff_result` と同時に返さない。

### 2.2 保証対象外

- プロセスクラッシュまたは OS 再起動後の outcome 復旧
- 非対話 stdin 向けの代替入力 protocol
- outcome の exactly-once 保存・配送
- ユーザー申告内容の真偽または完了条件の自動検証
- 複数 Human Task の競合・並列管理

## 3. Non-goals

- `human_task` tool または独立 Human Task model
- outcome / handoff の永続化、中断再開、履歴管理
- 複数タスク管理、side agent、ownership、lease、heartbeat、reconciler、ネットワーク協調
- Human Shell 内の `ai ask` / `done`、コマンド履歴・ファイル変更履歴の収集
- summary / 理由の手入力、完了条件の自動検証
- exit code、return marker、表示文言からの status 推論
- Human Shell の全面再設計
- 通常 `shell_exec`、非 Collaborative Mode、既存 CLI / 設定 / LLM tool 定義の変更
- 新 crate または外部依存の追加

## 4. Complexity inventory

| 項目 | 数・内容 |
|------|----------|
| 実行主体 | 1（既存 `ai` collaborative handoff 内の同期処理。新規主体なし） |
| 状態機械 | 0（永続状態なし。入力再試行は adapter 内の同期 loop） |
| 永続 aggregate | 0 |
| 外部副作用 | 1（terminal stdin / stderr の対話 I/O） |
| プロセス境界 | 1（既存 `ai` → `aibe` の `ShellExecApproval.handoff_result`） |
| 新規基盤機構 | 0（既存 handoff port / DTO / synthetic result の拡張） |
| 他機能統合 | 1（0055 collaborative human handoff） |

`scripts/feature-scope.toml` の `0059` entry と一致させる。

## 5. Complexity Gate

- 判定: Green
- 理由: 新しい実行主体、状態機械、永続化、agent loop、process boundary を導入せず、0055 の同期 handoff に純粋 domain 型、terminal input adapter、既存 DTO field を一つずつ追加する薄い変更である
- 分割判断: 第1段階は単一 Human Shell の明示 outcome 返却だけに固定し、Human Task 化・永続化・協調機構は Deferred specs へ送る
- 承認例外: なし

## 6. Complexity budget

| 項目 | 上限 |
|------|------|
| 新規実行主体 | +0 |
| 状態機械 | +0 |
| 永続 aggregate | +0 |
| agent loop / side agent | +0 |
| process boundary | +0 |
| 外部依存 / crate | +0 |
| protocol DTO | 既存 `HumanHandoffResult` の optional 構造化 field +1 のみ |
| terminal adapter | outcome collector +1 のみ |

## 7. Split triggers

次が必要になったら STOP-THE-LINE し、0059 に追加せず別 spec へ分割する。

- `human_task` tool または独立 task aggregate
- outcome / handoff の永続化、resume、schema migration
- side agent または二つ目の agent loop
- 複数タスク管理、ownership、lease、heartbeat、reconciler
- 非対話 stdin 向け protocol または別 UI
- コマンド / ファイル変更履歴、完了条件検証
- network coordination または exactly-once delivery
- Human Shell lifecycle / PTY cleanup の再設計

## 8. パック構成の適用

**No** — 0045 §6 の候補条件に該当しない。0059 は optional 配備する機能束ではなく、0055 の Collaborative Mode が正常に親へ返す結果契約の一部であり、単一 application service / terminal adapter / 既存 protocol DTO に閉じる軽量機能である。runtime toggle、専用 RPC / CLI、重い依存、basic build からの除外は不要なため、Pack 境界 / Active Pack / Basic Pack は作らない。

## 9. Acceptance Criteria

| ID | 条件 |
|----|------|
| `collab_outcome_returns_structured_to_parent` | Human Shell の正常終了と再観測の後、親 terminal の termios を復元してから status を収集し、`HumanHandoffResult.collab_outcome.status` が既存 JSON synthetic tool result を通じて同じ親 agent へ構造化されて返る |
| `collab_outcome_status_accepts_documented_forms` | `d` / `done`、`b` / `blocked`、`c` / `cancelled` を大文字小文字を無視して対応 status に parse する |
| `collab_outcome_status_rejects_invalid_forms` | 空、`x`、`complete`、`failed` その他未定義入力を拒否して再入力し、`failed` status を持たない |
| `collab_outcome_domain_creation_preserves_invariants` | 純粋 domain の `CollabOutcomeStatus` / `CollabOutcome { status }` が全 status を I/O なしで表現する（summary フィールドなし） |
| `collab_outcome_serializes_all_statuses` | `done` / `blocked` / `cancelled` が安定した snake_case の構造化 JSON に serialize され、`summary` キーを含まない |
| `collab_outcome_is_independent_from_shell_exit_code` | exit 0 / non-zero のいずれからも status を自動決定せず、ユーザーの明示選択だけを採用する |
| `collab_outcome_launch_failure_skips_prompt` | Human Shell 起動失敗または正常 return marker 不足時は outcome prompt を表示せず既存 tool error を返し、`blocked` outcome を生成しない |
| `collab_outcome_noninteractive_stdin_fails_explicitly` | stdin が非対話なら暗黙 outcome を生成せず、収集不能を示す明示エラーを返す |
| `collab_outcome_io_is_unit_testable` | outcome collector の入出力ストリームを差し替えて status prompt・再入力を単体テストでき、実 PTY E2E を不必要に追加しない |
| `non_collaborative_shell_exec_remains_unchanged` | 通常 `shell_exec`、非 Collaborative Mode、既存 CLI / 設定 / LLM tool 定義の挙動が変わらない |

各 row は Scope Lock とともに `scripts/spec-acceptance.toml` と 1:1 に固定する。

## 10. Deferred specs

- 独立 `human_task` tool / Human Task domain model
- outcome と handoff の永続化、履歴、resume / crash recovery
- 複数 Human Task の管理、side agent、ownership、lease / heartbeat / reconciler
- Human Shell 内 `ai ask` / `done`
- **第3段階**: 実行コマンド・終了コード・標準出力/エラー・最後に失敗したコマンドなどからの自動結果情報生成
- Human Shell 終了フローへの summary / 理由の手入力欄（第1段階では意図的に採用しない。自動収集や会話フォローアップで代替する）
- コマンド履歴・ファイル変更履歴の収集と完了条件検証
- 非対話・remote / network collaboration

## 11. Scope change log

| Revision | 分類 | 変更 | 理由 |
|----------|------|------|------|
| 1 | INITIAL | 0055 の Human Shell 正常終了後に明示 outcome を収集し、既存 structured handoff result で親へ返す第1段階を設計確定 | Collaborative Mode 全体を再設計せず、ユーザー申告結果だけを最小 vertical slice として追加するため |
| 3 | REDUCE | `summary` フィールドと終了後の手入力経路を削除し、status 選択のみに縮小。AC `collab_outcome_summary_rules_are_enforced` を削除 | 将来の自動ログ収集追加時に手入力 UX が固定化・互換維持で残り続けるのを防ぐ。第1段階の責務はシェル終了と作業完了の分離のみ |
| 4 | SUPERSEDED | 0060 により終了後の対話 outcome 収集と必須 `collab_outcome` 契約を撤回する方針を記録 | Human Task briefing から追加入力なしで親へ戻る UX を正とし、推定 status を返さないため |
