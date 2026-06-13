# 0036 — `shell_exec` 承認 UX 拡張 実装指示書

> **設計書**: [spec/0036_shell-exec-approval-ux-spec.md](../spec/0036_shell-exec-approval-ux-spec.md)  
> **状態**: 実装指示書  
> **注意**: この文書は実装手順・検証・受け入れ条件を定義する。コード実装は行わない。  
> **作成日**: 2026-06-13

## 要約

`shell_exec` の承認 UX を、`y/n/a/c` の 4 選択・session 限定記憶・tier 分岐・pattern auto-approve まで含めて実用化する。  
実装の芯は次の 4 点。

1. `ShellExecApproval` wire に `approval_origin` を追加し、`aibe` 側で `approval_source` を監査可能にする
2. `ai` 側で `read_only` / `mutating` / `destructive` の tier classifier を持ち、session shell 許可と `YesExecCache` を分離する
3. `[tools.shell_exec.auto_approve_patterns]` を `AIBE_CONFIG` から読み、session 許可後のみ有効にする
4. `ai` / `aibe` / `aibe-client` / `aibe-protocol` / docs を同時に更新し、`./scripts/verify.sh` と関連 smoke / integration を通す

## 変更ファイル一覧

### `aibe-protocol`

- `aibe-protocol/src/request.rs`
- `aibe-protocol/src/executed_tool.rs`
- `aibe-protocol/src/lib.rs`

### `aibe-client`

- `aibe-client/src/transport.rs`
- `aibe-client/src/lib.rs`
- `aibe-client/tests/agent_turn_approval.rs`

### `aibe`

- `aibe/src/ports/outbound/shell_exec_approval.rs`
- `aibe/src/adapters/inbound/connection_approval.rs`
- `aibe/src/adapters/inbound/unix_socket_server.rs`
- `aibe/src/adapters/outbound/tools/shell_exec.rs`
- `aibe/src/adapters/outbound/toml_config.rs`
- `aibe/src/ports/outbound/config.rs`
- `aibe/src/application/request_service.rs`
- `aibe/tests/shell_exec_approval_socket.rs`
- `aibe/tests/agent_turn_tools.rs` または既存 shell_exec 監査系テスト

### `ai`

- `ai/src/adapters/outbound/shell_exec_approval_ui.rs`
- `ai/src/adapters/outbound/yes_exec_cache.rs`
- `ai/src/adapters/outbound/aibe_client.rs`
- `ai/src/adapters/outbound/aibe_config.rs`
- `ai/src/main.rs`
- `ai/src/domain/shell_exec_approval.rs` または同等の domain モジュールを新設
- `ai/src/domain/mod.rs`
- `ai/tests/yes_exec_integration.rs`
- `ai/tests/shell_exec_approval_ux.rs` など新規 integration

### `docs`

- `docs/security.md`
- `docs/architecture.md`
- `docs/testing.md`
- `docs/manual/ai-ask-tools.md`
- `docs/aibe.config.example.toml`

> 注: 要件中の `aibe.config.example.toml` は実ファイル名 `docs/aibe.config.example.toml` を指す。

## 実装手順

### Phase 1: UI + session cache + wire plumbing

#### 1.1 `approval_origin` wire を追加する

対象:

- `aibe-protocol/src/request.rs`
- `aibe-protocol/src/executed_tool.rs`
- `aibe-protocol/src/lib.rs`
- `aibe-client/src/transport.rs`
- `aibe-client/src/lib.rs`
- `aibe/src/ports/outbound/shell_exec_approval.rs`
- `aibe/src/adapters/inbound/connection_approval.rs`

作業内容:

- `ClientRequest::ShellExecApproval` に `approval_origin` を追加する
- `approval_origin` を表す enum / DTO を `aibe-protocol` に置く
- `aibe-client::agent_turn_*` が approval callback の結果として `approved + approval_origin` を送れるようにする
- `ShellExecApprovalGate` は bool だけでなく provenance 付きの decision を受け取れる形に変える
- `aibe` 側は `approval_origin` を tool_call ごとに保持し、後続の audit で使えるようにする
- `ExecutedToolCall::with_shell_exec_audit` は `approval_origin` から `approval_source` を再構成できるようにする

想定関数・責務:

- `aibe-client::transport::agent_turn_with_events_on_stream`
- `aibe-client::transport::agent_turn_with_events`
- `aibe-client::transport::agent_turn_on_stream`
- `aibe-client::transport::agent_turn`
- `aibe::ports::outbound::shell_exec_approval::ShellExecApprovalGate`
- `aibe::adapters::inbound::connection_approval::ConnectionApprovalGate::request_shell_exec_approval`
- `aibe::adapters::outbound::tools::shell_exec::finish_shell_exec`

#### 1.2 `y/n/a/c` UI を実装する

対象:

- `ai/src/adapters/outbound/shell_exec_approval_ui.rs`
- `ai/src/adapters/outbound/aibe_client.rs`
- `ai/src/main.rs`

作業内容:

- 承認 UI を `y / n / a / c` の 4 選択に拡張する
- `approval_prompt_stderr_lines` は escape 表示を維持しつつ、選択肢と tier を出せるようにする
- `prompt_shell_exec_approval` は `bool` ではなく、少なくとも `approved` と `approval_origin` を返す decision 型にする
- `stdin_ready_for_shell_exec_approval()` の fail-closed を維持する
- `run_agent_turn_core` の approval callback で session cache と UI をつなぐ

想定関数:

- `approval_prompt_stderr_lines`
- `stdin_ready_for_shell_exec_approval`
- `parse_approval_yes`
- `prompt_shell_exec_approval`
- `run_agent_turn_core`

#### 1.3 `YesExecCache` を拡張する

対象:

- `ai/src/adapters/outbound/yes_exec_cache.rs`
- `ai/tests/yes_exec_integration.rs`

作業内容:

- `YesExecCache` のキーを `exact_invocation` と `command_name` に拡張する
- session 限定であることを維持し、永続化は session scope のみとする
- 既存 cache ファイルの読み込みは壊さない方向で移行する
- `remember` は `y` / `a` / `c` の選択に応じた scope を保存する
- `should_auto_approve` は tier 判定結果と合わせて参照する

想定関数:

- `YesExecCache::load`
- `YesExecCache::should_auto_approve`
- `YesExecCache::remember`
- `approval_key`
- `cache_path`

#### 1.4 session shell 許可を `ai` に持たせる

対象:

- `ai/src/main.rs`
- `ai/src/adapters/outbound/aibe_client.rs`
- `ai/src/adapters/outbound/aibe_config.rs`

作業内容:

- `ai chat` の session 初回だけ `shell_exec` の session 許可を要求する状態を `run_agent_turn_core` 側で保持する
- `session_shell_allowed = false` の間は cache / pattern による自動承認をしない
- `AIBE_CONFIG` から読み込んだ shell_exec policy を `ai` 側でも利用できるようにする
- `--yes-exec` は `ask` のみ有効、`never` を越えない現在の優先順位を維持する

想定関数:

- `run_agent_turn_core`
- `load_shell_exec_approval`
- `AibeShellExecApproval`

### Phase 2: tier classifier + command-only 記憶

#### 2.1 tier classifier を導入する

対象:

- `ai/src/domain/shell_exec_approval.rs` などの新規 domain モジュール
- `ai/src/domain/mod.rs`
- `ai/src/adapters/outbound/shell_exec_approval_ui.rs`
- `ai/src/adapters/outbound/yes_exec_cache.rs`
- `ai/src/main.rs`

作業内容:

- `read_only` / `mutating` / `destructive` を structured argv で分類する
- `shell` 文字列の見た目ではなく `command + args` で判定する
- 曖昧なものは上位 tier に倒す
- `c` で記憶した場合でも tier を再評価し、`mutating` / `destructive` へ横断しないようにする
- `read_only` は session 許可後に自動承認しやすく、`mutating` は初回 prompt を維持できるようにする

想定関数:

- `classify_shell_exec_tier`
- `canonical_shell_exec_invocation`
- `normalize_shell_exec_invocation`
- `shell_exec_approval_origin_for_decision`

#### 2.2 session cache decision を tier と結びつける

対象:

- `ai/src/adapters/outbound/yes_exec_cache.rs`
- `ai/src/main.rs`
- `ai/tests/yes_exec_integration.rs`

作業内容:

- `a` は `command + args` の完全一致のみ再利用する
- `c` は command 名ベースで再利用するが、同一 tier 内に閉じる
- `read_only` / `mutating` / `destructive` の decision が audit できるよう、`approval_origin` を返す経路を統一する
- non-TTY では拒否し、`y` を入力しても通らない挙動を維持する

### Phase 3: pattern config + audit 文字列

#### 3.1 `[tools.shell_exec.auto_approve_patterns]` を実装する

対象:

- `aibe/src/ports/outbound/config.rs`
- `aibe/src/adapters/outbound/toml_config.rs`
- `ai/src/adapters/outbound/aibe_config.rs`
- `docs/aibe.config.example.toml`

作業内容:

- `auto_approve_patterns` の TOML を追加する
- `read_only` と `mutating` の 2 系統を扱えるようにする
- pattern は structured argv の canonical form に対して評価する
- `session_shell_allowed = true` の場合のみ pattern を見る
- destructive tier は pattern でも自動承認しない
- 解析失敗時は自動承認に倒さず、安全側に倒す

想定関数:

- `AibeShellExecApproval::load`
- `parse_tools`
- `parse_shell_exec_auto_approve_patterns`
- `match_shell_exec_auto_approve_pattern`

#### 3.2 `approval_source` / `decision` の監査を整える

対象:

- `aibe/src/adapters/outbound/tools/shell_exec.rs`
- `aibe-protocol/src/executed_tool.rs`
- `aibe/tests/shell_exec_approval_socket.rs`
- `aibe/tests/external_commands.rs`

作業内容:

- `approval_source` に `ui=y|a|c`、`cache=session`、`pattern=<name>` の provenance を残す
- `decision` は `executed` / `rejected_by_user` / `rejected_by_policy` / `rejected_by_tier` / `auto_approved_session` / `auto_approved_pattern` / `approval_unavailable` を使う
- `external_command=` 付き既存監査と衝突しないことを確認する

## テスト追加一覧

### `aibe-protocol`

- `aibe-protocol/src/request.rs` の `ShellExecApproval` 追加フィールド roundtrip
- `aibe-protocol/src/executed_tool.rs` の `approval_origin` / `approval_source` roundtrip

### `ai`

- `ai/src/adapters/outbound/shell_exec_approval_ui.rs`
  - `y/n/a/c` の表示
  - non-TTY fail-closed
  - escape 表示維持
- `ai/src/adapters/outbound/yes_exec_cache.rs`
  - `exact_invocation`
  - `command_name`
  - legacy cache 読み込み
- `ai/src/adapters/outbound/aibe_config.rs`
  - `auto_approve_patterns` 読み込み
- `ai/tests/yes_exec_integration.rs`
  - seeded cache の session 再利用
  - `never` 優先
  - non-TTY deny
- 新規 `ai/tests/shell_exec_approval_ux.rs`
  - `y / n / a / c` の分岐
  - tier 別自動承認
  - pattern 自動承認

### `aibe-client`

- `aibe-client/tests/agent_turn_approval.rs`
  - `approval_origin` を含む approval 往復
  - approved / denied 両系統

### `aibe`

- `aibe/tests/shell_exec_approval_socket.rs`
  - `approval_origin` が tool audit に反映されること
  - user deny / session allow / pattern allow の差分
- `aibe/tests/agent_turn_tools.rs` または同等の shell_exec 監査系テスト
- `aibe/src/adapters/outbound/toml_config.rs` の config parse unit
- `aibe/src/adapters/outbound/tools/shell_exec.rs` の `approval_source` unit

## docs 更新

以下を同じ変更で更新する。

- `docs/security.md`
  - session shell 許可の境界
  - `approval_origin` / `approval_source`
  - pattern auto-approve の安全条件
- `docs/architecture.md`
  - shell_exec 承認の責務分離
  - `ai` / `aibe` / `aibe-client` の役割
  - `approval_origin` wire の位置づけ
- `docs/testing.md`
  - unit / integration / manual の追加表
  - 新しい shell_exec 承認テストの正本
- `docs/manual/ai-ask-tools.md`
  - `y/n/a/c` の見え方
  - session 許可
  - pattern auto-approve
  - non-TTY fail-closed
- `docs/aibe.config.example.toml`
  - `[tools.shell_exec.auto_approve_patterns]`
  - 既存 `shell_exec_approval` の例示維持

## 受け入れ条件チェックリスト

- [ ] `shell_exec_approval = "ask"` で `y / n / a / c` が動く
- [ ] `approval_origin` が wire に載る
- [ ] `approval_source` が `ui` / `cache` / `pattern` を識別できる
- [ ] `read_only` tier は session 許可後に自動承認される
- [ ] `mutating` tier は初回 prompt を維持できる
- [ ] `destructive` tier は毎回 prompt され、自動承認しない
- [ ] pattern auto-approve は session 許可後のみ有効
- [ ] `--yes-exec` は `never` を越えない
- [ ] non-TTY は fail-closed
- [ ] `./scripts/verify.sh` が通る

## Step 6 で実行するコマンド

### 必須

```bash
./scripts/verify.sh
./scripts/smoke-mock.sh
```

### 追加 integration

```bash
cargo test -p ai --test yes_exec_integration -- --test-threads=1
cargo test -p ai --test shell_exec_approval_ux -- --test-threads=1
cargo test -p aibe --test shell_exec_approval_socket -- --test-threads=1
cargo test -p aibe-client --test agent_turn_approval -- --test-threads=1
```

### 追加で落とすべき unit / protocol

```bash
cargo test -p aibe-protocol --lib executed_tool -- --nocapture
cargo test -p aibe-protocol --lib request -- --nocapture
```

## 実装順の補足

1. wire と transport を先に揃える
2. `ai` の UI / cache / session state を固める
3. tier classifier を入れる
4. pattern config と audit 文字列を仕上げる
5. docs と test を同期してから verify / smoke を回す

## 未確定・推測・指示外

- `auto_approve_patterns` の TOML の最終スキーマは設計書で厳密固定されていないため、実装時に `read_only` / `mutating` の 2 系統配列に寄せる前提で進める
- `approval_origin` の enum 名・wire 名は既存 naming に合わせて最終調整が必要
- `ai/src/domain/shell_exec_approval.rs` は新規作成を想定しているが、既存 domain モジュールへ統合してもよい

## 残リスク

- pattern は広すぎると誤承認の温床になるため、手動検証で境界確認が必要
- TTY / non-TTY の切り替えは自動テストだけでは取り切れないため、manual 手順の確認が必要
- `approval_origin` 追加後は `aibe-client` / `aibe` / `ai` の往復契約が増えるので、回帰時は wire roundtrip を優先して見る
