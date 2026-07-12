# 0059 Collaborative Mode Outcome（第1段階）実装指示書

設計書: [`docs/spec/0059_collab-outcome-status-spec.md`](../spec/0059_collab-outcome-status-spec.md)

> **Scope revision 3**: `summary` 手入力経路を削除し、status 選択のみとする（設計書 §11）。

## 0. 目的

0055 の Collaborative Mode で Human Shell が正常に制御を返した後、親 terminal の termios を復元してから、ユーザーに `done` / `blocked` / `cancelled` を明示選択させる。選択時点で追加入力なく親へ戻し、既存 `HumanHandoffResult.collab_outcome.status`、`ShellExecApproval.handoff_result`、aibe の JSON synthetic tool result を通して構造化 outcome を返す。正本は設計書であり、summary 手入力、Human Task 化、永続化、複数 task 管理へ拡張しない。

## 0.1 Scope Lock

- Feature scope registry: `scripts/feature-scope.toml`
- Scope revision: `3`
- Complexity class: Green
- Vertical slice AC ID: `collab_outcome_returns_structured_to_parent`
- Locked AC IDs:
  - `collab_outcome_returns_structured_to_parent`
  - `collab_outcome_status_accepts_documented_forms`
  - `collab_outcome_status_rejects_invalid_forms`
  - `collab_outcome_domain_creation_preserves_invariants`
  - `collab_outcome_serializes_all_statuses`
  - `collab_outcome_is_independent_from_shell_exit_code`
  - `collab_outcome_launch_failure_skips_prompt`
  - `collab_outcome_noninteractive_stdin_fails_explicitly`
  - `collab_outcome_io_is_unit_testable`
  - `non_collaborative_shell_exec_remains_unchanged`

実装開始時、上記 ID を `scripts/feature-scope.toml` の `locked_ac_ids` に同順で固定し、feature status を同 registry の既存命名に従う実装中状態へ変更する。Scope Lock 後に AC、実行主体、状態機械、外部 effect、process boundary、integration を追加しない。必要になった場合は実装を止め、設計書 §7 と `docs/feature-development-policy.md` に従って scope revision と Complexity Gate を再審査する。

## 1. 調査済みの既存経路

| 層 / 経路 | 既存ファイル・型・関数 | 0059 での扱い |
|-----------|------------------------|---------------|
| composition root | `ai/src/main.rs` の collaborative `shell_exec` approval handler | `create_runtime_handoff_dir()`、`RuntimeHandoffDirGuard`、`ParentTermiosGuard::save()`、`RunSynchronousHumanHandoff::execute()` の既存順を基点にする。service 成功後に guard を明示 drop してから collector を呼ぶ |
| Application | `ai/src/application/human_handoff.rs` の `RunSynchronousHumanHandoff::execute`、`HumanHandoffRequest`、`HumanHandoffError` | Human Shell 起動、normal return marker 確認、`EnvironmentObserver::observe` までの責務と返値を維持する。outcome I/O を入れない |
| Handoff port | `ai/src/ports/outbound/human_handoff.rs` の `HumanShellLauncher::launch_and_wait`、`HumanShellReturn`、`EnvironmentObserver` | launcher / observer の責務を変更しない。別の outcome collector port を追加する |
| Handoff adapter | `ai/src/adapters/outbound/human_handoff.rs` の `AishHumanShellLauncher`、`ProcessEnvironmentObserver`、`ParentTermiosGuard`、`RuntimeHandoffDirGuard` | collector は Human Shell process 制御と分離した terminal I/O adapter として配置する |
| Protocol DTO | `aibe-protocol/src/collaborative_handoff.rs` の `HumanHandoffResult`、`HandoffExecutionOutcome`、`RequestedCommandCompletion` | wire 用 outcome enum / struct と必須 field を追加する。`aibe-protocol/src/lib.rs` から既存方式で re-export する |
| Approval transport | `aibe-protocol/src/request.rs` の `ClientRequest::ShellExecApproval { handoff_result, handoff_error, ... }`、`aibe-client` の `ShellExecApprovalDecision` | request variant、新 tool、新 RPC は追加せず、既存 `handoff_result` を使用する |
| aibe inbound | `aibe/src/adapters/inbound/connection_approval.rs` の approval decision 変換 | DTO field 追加に伴う compile/test 同期だけとし、新分岐を作らない |
| Synthetic result | `aibe/src/adapters/outbound/tools/shell_exec.rs` の `decision.handoff_result` 分岐と `serde_json::to_string` | DTO 全体の既存 JSON 化を維持し、全 status の snake_case serialization を検証する |
| Regression | `ai/tests/0055_collaborative_handoff_vertical_e2e.rs`、`ai/tests/normal_shell_exec_regression.rs`、`ai/tests/0055_minimal_human_handoff.rs` | 0055 fixture / mock aibe / watchdog を再利用し、通常経路と service 責務の regression を守る |
| Pending AC | `ai/tests/0059_collab_outcome_status_pending.rs`、`scripts/spec-acceptance.toml` の 0059 rows | 登録済み 11 skeleton と関数名を正本にし、実テストへ置換する |

既存命名の `HumanHandoff*`、`CollabOutcome*`、`RunSynchronousHumanHandoff`、`*Collector`、`*Error` に合わせる。adapter から `aibe_protocol` 型を直接返さず、domain outcome と wire DTO を application mapper で分離する。

## 2. Phase 分割と解除順

0059 は単一 Phase とする。domain、stream 差し替え単体、protocol serialization、composition root、縦断 E2E、regression を一つの vertical slice として完結させる。

| Phase | 内容 | ゲート（`scripts/spec-acceptance.toml`） |
|-------|------|------------------------------------------|
| 1 | domain invariant、outcome collector port / terminal adapter、application mapper、必須 protocol field、termios 復元後の収集、既存 synthetic result、通常経路 regression を実装する | 0059 の全 11 AC。縦断ゲートは `collab_outcome_returns_structured_to_parent` |

解除順は次のとおりとする。

1. 登録済み `ai/tests/0059_collab_outcome_status_pending.rs` の ignored skeleton を、同名 test の実装へ置換する。必要に応じて domain / adapter 内 unit test を追加するが、registry test の代替にしない。
2. domain parse / invariant、collector の差し替えストリーム単体、protocol serialization を先に緑にする。
3. composition root を接続し、launch failure / noninteractive / exit-code independence / normal regression を緑にする。
4. 最小の collaborative PTY E2E で termios 復元後の入力と structured synthetic result を確認する。
5. 全 11 AC が本番経路で成功した同じ変更で、Rust test の `#[ignore]` を外し、0059 の全 registry row を `pending = false` にする。部分的な `pending=false` や部分完了報告は行わない。

Vertical Slice Gate: 縦断 AC が成功する前に Human Task model、永続化、resume、複数 task、side agent、汎用対話 framework、別 protocol を追加しない。

## 3. 変更対象ファイル（予想）

| crate / 区分 | ファイル | 具体的な変更 |
|--------------|----------|--------------|
| `ai` Domain | `ai/src/domain/collab_outcome.rs`（新規）、`ai/src/domain/mod.rs` | `CollabOutcomeStatus`、`CollabOutcome`、parse error / creation error と invariant を追加し re-export する |
| `ai` Port | `ai/src/ports/outbound/collab_outcome.rs`（新規）、`ai/src/ports/outbound/mod.rs` | outcome 収集境界と収集 error を追加し re-export する |
| `ai` Adapter | `ai/src/adapters/outbound/collab_outcome.rs`（新規）、`ai/src/adapters/outbound/mod.rs` | stdin interactivity 判定、stderr prompt、status 再入力を実装し re-export する。reader / writer を差し替え可能にする。summary 入力は実装しない |
| `ai` Application | `ai/src/application/collab_outcome.rs`（新規）、`ai/src/application/mod.rs` | domain outcome を protocol DTO へ変換し、既存 `HumanHandoffResult` に統合する mapper を追加する |
| `ai` composition | `ai/src/main.rs` | handoff service 成功後、termios guard drop 後だけ collector を呼び、成功時だけ approved result を返す |
| Protocol | `aibe-protocol/src/collaborative_handoff.rs`、`aibe-protocol/src/lib.rs` | wire `CollabOutcomeStatus` / `CollabOutcome` と `HumanHandoffResult.collab_outcome` 必須 fieldを追加・export する |
| Protocol consumers | `aibe/src/adapters/outbound/tools/shell_exec.rs`、`aibe/src/adapters/inbound/connection_approval.rs`、関連する `HumanHandoffResult` literal | compile 同期、全 status JSON test、既存 synthetic result assertion を更新する |
| Tests | `ai/tests/0059_collab_outcome_status_pending.rs`（実装時に既存命名との整合を保ったまま必要なら `_pending` を外す）、`ai/tests/0055_collaborative_handoff_vertical_e2e.rs`、`ai/tests/normal_shell_exec_regression.rs` | 11 AC、最小 PTY E2E、通常経路 regression。registry の `file_glob` は実ファイル名変更と同時に更新する |
| Registry | `scripts/feature-scope.toml`、`scripts/spec-acceptance.toml` | Scope Lock と、全 AC 成功後の pending 解除 |
| Docs | `docs/architecture.md`、`docs/manual/0059_collab-outcome-status.md`（新規）、`docs/manual/README.md`、完了時 `docs/0000_spec-index.md` | protocol / layer / termios 順序、手動確認手順、完了状態を同期する |

既存 literal の compile 修正では `collab_outcome` を省略可能にするための serde default を足さない。本番契約上必須なので、fixture ごとに意味のある outcome を明示する。

## 4. レイヤー責務と API 方針

### 4.1 Domain (`ai`)

- `CollabOutcomeStatus` は `Done` / `Blocked` / `Cancelled` の三値だけを持つ。`Failed` を追加しない。
- `parse_status(input: &str)` 相当は trim 後に ASCII 大文字小文字を無視し、`d|done`、`b|blocked`、`c|cancelled` だけを受理する。I/O と protocol serde を持ち込まない。
- `CollabOutcome` は `status` のみを持つ。`summary` フィールド・constructor 引数・必須判定は持たない。
- `human_shell_exit_code` を constructor 引数にせず、status 推論 API を作らない。

### 4.2 Port / Adapter (`ai`)

- outbound port は `collect() -> Result<CollabOutcome, CollabOutcomeCollectionError>` 相当の一責務にする。戻り値は domain 型とし protocol DTO を返さない。
- terminal adapter の production entry は `std::io::stdin().is_terminal()` を最初に検査する。false なら prompt を一切出さず、`Cannot collect Collaborative Mode result because stdin is not interactive.` を識別できる error を返す。
- testable core は `BufRead` と `Write`（または同等の小さい抽象）を引数に取り、入力と stderr 出力を差し替え可能にする。global stdin/stderr を unit test から触らない。
- status prompt は設計書 §1.2 の文面と選択肢を stderr に出し、flush 後に一行読む。invalid / 空 / `complete` / `failed` は error 文を出して再入力する。
- **有効な status を得た時点で即座に `CollabOutcome` を返し、summary / 理由の追加入力は一切行わない。**
- EOF / read / write / flush error は収集 error とし、暗黙 outcome を返さない。
- 対話 loop は adapter 内の同期 loop に限定し、状態機械、background task、新 dependency を作らない。

### 4.3 Application / composition root (`ai`)

- `RunSynchronousHumanHandoff::execute()` は Human Shell 正常 return marker と再観測までを担当し、collector を注入しない。これにより launch failure / marker 不足時に prompt が起動しないことを構造で保証する。
- application mapper は `HumanHandoffResult` と domain `CollabOutcome` を受け、wire `CollabOutcome` を設定した完成 DTO を返す。domain enum と wire enum の全 variant を明示 match し、文字列 round-trip を使わない。
- `ai/src/main.rs` の順序は次のコード構造を崩さない。

```text
ParentTermiosGuard::save()
→ RunSynchronousHumanHandoff::execute()（launch + normal marker + 再観測）
→ drop(parent_termios_guard)（親 terminal の termios 復元）
→ execute 成功時だけ outcome_collector.collect()
→ mapper で HumanHandoffResult.collab_outcome を設定
→ approved=true + handoff_result=Some
```

- `drop` は collector 呼出より前の独立 statement にし、変数名を `_termios_guard` のままにして暗黙 scope drop に依存しない。execute が失敗した場合もまず drop し、collector を呼ばず既存 `human_handoff_failed` を返す。
- collector の noninteractive / EOF / I/O error も `code = "human_handoff_failed"` の既存 `ShellExecApprovalDecision.handoff_error` に変換し、`approved=false`、`handoff_result=None` とする。収集成功時だけ `approved=true` にする。
- runtime guard は outcome 収集完了または失敗まで保持し、既存 cleanup 契約を維持する。

### 4.4 Protocol / aibe

- `aibe_protocol::CollabOutcomeStatus` は `#[serde(rename_all = "snake_case")]` で `done` / `blocked` / `cancelled` を固定する。
- wire `CollabOutcome` は `status` のみを持つ。`summary` フィールドは追加しない。
- `HumanHandoffResult.collab_outcome` は optional にせず必須にする。0059 後の collaborative success は必ず設定し、非 collaborative 経路は `HumanHandoffResult` 自体を作らない。
- `ClientRequest::ShellExecApproval`、`ShellExecApprovalDecision`、validation invariant、新規 tool 定義を増やさない。aibe は既存 `serde_json::to_string(&handoff_result)` の synthetic result 経路を使う。
- serialization fallback の変更は 0059 の非対象とし、通常 serialization が成功することを test する。

## 5. AC とテストマッピング

registry test の関数名は AC ID と完全一致させる。対話 E2E は最小 1 本とし、prompt / retry はストリーム差し替え単体を優先する。

| AC ID | test 関数 | 主ファイル / 種別 | 検証内容 |
|-------|-----------|-------------------|----------|
| `collab_outcome_returns_structured_to_parent` | 同名 | `ai/tests/0055_collaborative_handoff_vertical_e2e.rs` / 最小 PTY E2E | normal return と再観測後、termios 復元状態で status 入力でき、mock aibe が `collab_outcome.status` を含む synthetic JSON を受ける |
| `collab_outcome_status_accepts_documented_forms` | 同名 | `ai/tests/0059_collab_outcome_status.rs` / domain unit | 6 documented forms と mixed/upper case を三 status に parse |
| `collab_outcome_status_rejects_invalid_forms` | 同名 | 同上 / stream unit | 空、`x`、`complete`、`failed` を拒否し再入力後に成功。summary 入力がない |
| `collab_outcome_domain_creation_preserves_invariants` | 同名 | 同上 / domain unit | 全 status の constructor を I/O なしで検証 |
| `collab_outcome_serializes_all_statuses` | 同名 | 同上 | 三 status の snake_case、`summary` キーなし、`HumanHandoffResult` 内の必須 field |
| `collab_outcome_is_independent_from_shell_exit_code` | 同名 | 同上 / application integration | exit 0 + blocked、non-zero + done 等を明示選択し、選択値が維持される |
| `collab_outcome_launch_failure_skips_prompt` | 同名 | 同上 / application unit | launcher error と missing marker で collector call count 0、prompt buffer 空、既存 error |
| `collab_outcome_noninteractive_stdin_fails_explicitly` | 同名 | 同上 / adapter/application unit | noninteractive flag で明示 error、outcome / handoff_result なし、approved false |
| `collab_outcome_io_is_unit_testable` | 同名 | 同上 / stream unit | in-memory reader/writer だけで正確な prompt、retry を検証。実 PTY 不要 |
| `non_collaborative_shell_exec_remains_unchanged` | 同名 | `ai/tests/normal_shell_exec_regression.rs` / regression | 通常 approval request に outcome prompt がなく、handoff_result なし。CLI / tool 定義不変 |

補助 test は `ai/src/domain/collab_outcome.rs`、`ai/src/adapters/outbound/collab_outcome.rs`、`aibe-protocol/src/collaborative_handoff.rs`、`aibe/src/adapters/outbound/tools/shell_exec.rs` に追加してよい。ただし registry の 11 test は薄いダミーにせず、対応する本番 API を直接または縦断経路で検証する。

## 6. `spec-acceptance.toml` 更新手順

1. 実装開始時点では、既存 0059 rows の `pending = true` と ignored skeleton を維持する。`id` / `test` は変更しない。
2. pending test ファイルを rename する場合だけ、同じ変更で全 0059 `file_glob` を実在 path に更新する。
3. `./scripts/check-feature-scope.py` で Scope Lock と registry の AC 集合が一致することを確認する。
4. 全 11 test が本番経路で成功した後、同じ変更で全 0059 rows を `pending = false` にし、全 test の `#[ignore]` を外す。
5. `./scripts/check-spec-acceptance.py` と対象 test を再実行する。一部だけ解除して Phase 完了扱いにしない。

## 7. ドキュメント更新

- `docs/architecture.md`: Collaborative handoff の既存節へ、domain / adapter / application / protocol の分離、`HumanHandoffResult.collab_outcome` schema、`human_shell_exit_code` と status の独立、`execute → termios drop → collect → map` の順序を最小追記する。
- `docs/manual/0059_collab-outcome-status.md`: 実端末で三 status の選択のみ（追加入力なし）、Human Shell launch failure 時に prompt が出ないことを確認する手順を書く。実 API key を必要としない mock aibe 手順を優先する。
- `docs/manual/README.md`: 上記 manual へのリンクを追加する。
- 完了時のみ `docs/0000_spec-index.md`: 0059 task row を削除し、spec を「設計確定（実装済み）」、本書を `docs/done/` の実装済み row に移す。

## 8. 非対象・触ってはいけないもの

- `human_task` tool、独立 Human Task domain / aggregate
- outcome / handoff の永続化、履歴、resume、crash recovery、exactly-once delivery
- 複数 task、side agent、二つ目の agent loop、ownership、lease、heartbeat、reconciler
- Human Shell 内の `ai ask` / `done`、command / file change 履歴、完了条件の自動検証
- 非対話 stdin の代替 protocol、remote / network collaboration、別 UI
- exit code、return marker、表示文言からの status 推論
- Human Shell lifecycle / PTY cleanup / termios guard の再設計
- 通常 `shell_exec`、非 Collaborative Mode、既存 CLI / config / LLM tool definition の変更
- 新 crate、外部 dependency、Pack 境界、Active/Basic Pack、動的ロード機構
- `aish` crate の変更。outcome の収集・解釈・送信を `aish` に置かない

## 9. 実装・検証順

1. Scope Lock を固定し、ignored AC test を実 test fixture に置換する。
2. domain と protocol DTO、pure mapper を実装し、domain / serialization test を `cargo test -p ai -j 1` と `cargo test -p aibe-protocol -j 1` の対象指定で通す。
3. collector port / terminal adapter を実装し、in-memory stream 単体を通す。
4. composition root を `execute → termios drop → collect → map` の順で接続し、failure / regression test を通す。
5. 0055 の helper と外側 watchdog を再利用した最小 PTY E2E を 1 本通す。prompt matrix ごとに PTY E2E を増やさない。
6. `./scripts/verify-targeted.sh` または対象 crate の直列 test を通す。
7. 全 11 AC の `#[ignore]` / pending を同時解除し、scope / acceptance checker を通す。
8. docs を同期し、完了直前に `./scripts/verify.sh` を 1 回実行する。失敗箇所だけ修正して最後に再実行し、`.verify-timing-last` の summary を報告する。

## 10. 完了条件

- [ ] `collab_outcome_returns_structured_to_parent`: Human Shell 正常終了・再観測後、親 termios 復元後に収集した status が既存 synthetic result で同じ親へ返る
- [ ] `collab_outcome_status_accepts_documented_forms`: documented 6 forms を大文字小文字無視で受理する
- [ ] `collab_outcome_status_rejects_invalid_forms`: 空・未定義・`complete`・`failed` を拒否して再入力する
- [ ] `collab_outcome_domain_creation_preserves_invariants`: domain が status のみを I/O なしで表現する
- [ ] `collab_outcome_serializes_all_statuses`: 三 status が安定した snake_case JSON になり `summary` キーを含まない
- [ ] `collab_outcome_is_independent_from_shell_exit_code`: exit code から status を決めない
- [ ] `collab_outcome_launch_failure_skips_prompt`: launch / marker failure では prompt と outcome を作らず既存 error を返す
- [ ] `collab_outcome_noninteractive_stdin_fails_explicitly`: 非対話 stdin で暗黙 outcome を作らず明示 error を返す
- [ ] `collab_outcome_io_is_unit_testable`: 差し替え stream で prompt / retry を単体検証できる
- [ ] `non_collaborative_shell_exec_remains_unchanged`: 通常 / 非 Collaborative Mode の挙動、CLI、tool definition が不変
- [ ] 11 AC の Rust test から `#[ignore]` が外れ、`scripts/spec-acceptance.toml` が全て `pending = false`
- [ ] `docs/architecture.md` と manual が実装に同期している
- [ ] `./scripts/check-feature-scope.py`、`./scripts/check-spec-acceptance.py`、`./scripts/verify.sh` が成功している
- [ ] 手動検証の実施結果、または未実施を残リスクとして報告している
- [ ] 全条件達成後だけ本書を `docs/done/` へ移し、`docs/0000_spec-index.md` を実装済みに更新している

## 11. STOP-THE-LINE 条件

新しい実行主体、状態機械、永続 aggregate、外部 effect、process boundary、integration、新 crate / dependency が必要になった場合、または §8 の非対象が必要になった場合は実装を停止する。0059 に混ぜず、設計書・feature scope revision・Complexity Gate を更新し、必要なら別 spec へ分割する。

## 12. 仕様との差分

なし。0059 設計書 revision 3（status 選択のみ、summary 手入力なし）と、修正済み順序（Human Shell 正常 return と再観測の完了後、親 termios を復元し、その後に outcome を収集）を厳守する。
