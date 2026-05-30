# 0020 — P4「日常利用前の仕上げ」正式指示書

> **出典**: `docs/todo/chatgpt-review-4th-gen/concerns.md` §4、`docs/todo/chatgpt-review-4th-gen/implementation-order.md`、`docs/todo/chatgpt-review-4th-gen/strengths.md`、`docs/architecture.md`、`docs/security.md`、`docs/manual/ai-ask-tools.md`、`docs/manual/aish-shell-log.md`、既存実装（`aish/`、`ai/`、`aibe/`、`aibe-client/`、`aibe-protocol/`）。
>
> **状態**: **実装済み**（2026-05-30）。本書は実装前の正式指示書であり、仮実装やサンプル止まりを許可しない。
>
> **重要**: `P4-4` のログ context 改善は **対象外**。本書では `aibe` の request context を拡張しない。

## 目的

P4 は、P3 まででつながった `aish` / `ai` / `aibe` の導線を、日常利用に耐える最後の摩擦まで詰める段階である。ここで直すのは、挙動の微妙なズレ、危険な既定値、transport の重複、そして `shell_exec` の実行前承認である。

本指示書は、次の 4 つを 1 本にまとめる。

1. `aish shell` の session cleanup と CLI 引数の境界を正す
2. `ai ask --session` の session id を厳格にする
3. `aibe-client` に socket transport を集約する
4. `shell_exec` の実行前承認と監査を本番経路に載せる

## スコープ

### 対象

| ID | 対象 | 要点 |
|----|------|------|
| P4-1a | `aish shell` の `max_sessions` off-by-one 修正 | `create_shell_session` の後に prune し、新規セッションを削除対象にしない |
| P4-1b | `aish shell` の余計な引数拒否 | `--format` 以外の未知引数を拒否し、usage を固定する |
| P4-1c | `--session ID` の strict validation | 12 桁小文字 hex 以外を `invalid session id` として拒否する |
| P4-2 | `aibe-client` transport 共通化 | `send_request` / `agent_turn` / `read_response_line` を `aibe-client` 側へ集約する |
| P4-3 | `shell_exec` 実行前承認 | `shell_exec_approval = "never" | "ask" | "always"` を導入し、`ask` は yes/no を要求する |

### 非対象

| ID | 非対象 | 理由 |
|----|--------|------|
| P4-4 | ログ context 改善 | 本書のスコープ外。`shell_log_tail` / `cwd` の現在仕様は維持する |
| P3 の再設計 | `ai` → `aibe` の既存 `agent_turn` コンテキスト | P4 では request context に新しいフィールドを足さない |
| 任意の新規 tool 追加 | `write_file` 等 | 今回は `shell_exec` の承認と監査のみ |

## 確定仕様

### P4-1a: `max_sessions` off-by-one 修正

- `aish shell` はセッションディレクトリを **先に作成してから** 古いセッションを prune する。
- prune は新規に作成した session を削除しない。
- `max_sessions` が `N` のとき、`aish shell` の終了時点で managed session の総数は **高々 N** である。
- prune は managed session 名（12 桁小文字 hex）のみを対象とし、他のディレクトリを削除しない。

### P4-1b: `aish shell` の余計な引数拒否

- `aish shell` の usage は `aish shell [--format tsv|json|env]` に固定する。
- `--format` を strip した後に残る引数が 1 つでもあれば、`aish shell` は usage エラーで終了する。
- `--format` のみは受理するが、`shell` の stdout/stderr の表示契約は変えない。
- `aish session` と同じく、未知引数は黙って無視しない。

### P4-1c: `--session ID` の strict validation

- `--session` は **ちょうど 12 文字**の **小文字 ASCII hex** `0-9a-f` のみを受理する。
- それ以外の値は `invalid session id` として拒否する。
- `AISH_SESSION_DIR` の basename と `--session` の値は一致しなければならない。
- `AISH_SESSION_DIR/current_log` を解決した後は、symlink 先が session 内の通常ファイルとして読めることを確認する。
- `docs/manual/aish-shell-log.md` と `docs/manual/ai-ask-tools.md` は、この strict 仕様と矛盾しない文言に揃える。

### P4-2: `aibe-client` transport 共通化

- socket connect / NDJSON 1 行送信 / 1 行受信 / JSON decode の transport は `aibe-client` に集約する。
- `ai/src/adapters/outbound/aibe_client.rs` は `AskRequest -> ClientRequest` の変換と `aibe-client` 呼び出しだけを持つ。
- `ai` に socket transport の重複実装を残さない。
- `aibe-client` は `ping` / `ensure_running` に加えて、`agent_turn` 用の汎用 request/response transport を提供する。
- 0017 の続きとして位置づけ、wire DTO は `aibe-protocol` を正本に保つ。

### P4-3: `shell_exec` 実行前承認

- `shell_exec` が有効なとき、`shell_exec_approval` が `ask` なら、実行直前に `command` と `args` を表示し、yes/no を求める。
- `always` は承認 UI を出さずに実行する。
- `never` は実行前承認を要求せず、即時拒否する。
- 承認往復は既存の Unix socket 接続上で完結させ、新しい接続の張り直しはしない。
- 承認・拒否・自動実行のすべてを audit に残す。
- `tool_calls` には少なくとも `approval_state` と `approval_source` を残す。
- `approval_state` は既存の `aibe-protocol` の `ExecutedToolCall` を使い、`approval_source` は approval policy の由来を識別できる文字列にする。
- `ask` でユーザーが拒否した場合、tool call は拒否記録として LLM に返し、同一 `agent_turn` の継続を妨げない。
- 拒否記録は `ExecutedToolCall` の `status=Error` として返し、`error` と `message` で明示的に拒否理由を表す。
- `ai` の表示は stdout ではなく stderr に寄せ、承認プロンプトが最終 assistant 出力を汚染しない。

| `shell_exec_approval` | prompt | `approval_state` | `approval_source` の例 |
|----------------------|--------|------------------|------------------------|
| `never` | なし | `NotRequired` | `shell_exec_approval=never` |
| `ask` | yes/no | `ExplicitClientOptIn` | `shell_exec_approval=ask` |
| `always` | なし | `NotRequired` | `shell_exec_approval=always` |

## 受け入れ条件

### P4-1a

- `aish shell` を `max_sessions = N` の状態で起動しても、終了時点の managed session 数は `N` を超えない。
- 新規作成した session が prune 対象にならないことがテストで固定される。
- prune の順序変更により、起動直後だけ `N + 1` になる経路が残らない。

### P4-1b

- `aish shell --format json` は受理される。
- `aish shell --format json --bogus` は usage エラーになる。
- `aish shell bogus` は usage エラーになる。
- `aish shell` の usage 文は `aish shell [--format tsv|json|env]` と一致する。

### P4-1c

- `--session 002f15d02b54` のような 12 桁小文字 hex は受理される。
- `--session 002F15D02B54`、`--session 123`、`--session ../x`、`--session 002f15d02b54extra` は拒否される。
- `AISH_SESSION_DIR` の basename と `--session` が一致しない場合は拒否される。
- `docs/manual/aish-shell-log.md` と `docs/manual/ai-ask-tools.md` の説明がこの strict 仕様と一致する。

### P4-2

- `ai/src/adapters/outbound/aibe_client.rs` に raw socket I/O の重複が残らない。
- `aibe-client` 側の transport が `agent_turn` の正本になる。
- `ai` の adapter は `AskRequest` を `ClientRequest` に変換して transport を呼ぶだけになる。
- `aibe-client` の unit / integration tests で request/response roundtrip が固定される。

### P4-3

- `shell_exec_approval = "ask"` のとき、`shell_exec` 実行前に command/args が表示され、yes/no の結果で実行可否が分かれる。
- `shell_exec_approval = "always"` のとき、承認 UI は出ずに実行される。
- `shell_exec_approval = "never"` のとき、実行されずに拒否記録が残る。
- `tool_calls` に `approval_state` / `approval_source` が残る。
- `approval_source` は `shell_exec_approval=ask|always|never` を識別できる。
- `stdout` には承認プロンプトが混ざらない。
- `docs/security.md` の dangerous tool / audit 方針と矛盾しない。

## レイヤー別タスク分解

### aish

- `aish/src/main.rs` の `shell` 分岐で、`create_shell_session` を先に実行してから prune する。
- `shell` 用の arg parsing を `session` と同等の strict さにする。
- `shell` の usage エラーを testable にする。
- `aish/src/adapters/outbound/session_store.rs` の prune / create の順序を仕様どおりに固定する。

### ai

- `ai/src/domain/shell_log_resolve.rs` の `validate_session_id()` を strict 12 桁小文字 hex にする。
- `ai/src/adapters/outbound/aibe_client.rs` は AskRequest→ClientRequest 変換のみを担当する。
- `ai` の UI は shell_exec 承認 prompt を stderr 側で扱う。

### aibe-client

- `send_request` / `agent_turn` / `read_response_line` の transport をここへ集約する。
- ping / ensure_running と同じ socket 基盤を再利用する。
- request/response の framing と parse error を一元化する。

### aibe

- `aibe/src/adapters/outbound/tools/shell_exec.rs` に承認 policy の判定を入れる。
- `aibe/src/application/tool_round/executor.rs` で audit を付与し、承認経路別に `approval_state` / `approval_source` を埋める。
- 必要なら approval 交渉用の protocol メッセージをサーバ側で解釈する。

### aibe-protocol

- `ExecutedToolCall` の audit roundtrip を維持する。
- P4-3 の承認往復に新しい protocol DTO が必要なら、ここを正本に追加する。
- JSON wire の形は、追加が必要な場合も互換性と検証可能性を優先して定義する。

## テスト計画

### unit

| 対象 | 期待 |
|------|------|
| `aish/src/adapters/outbound/session_store.rs` | `create_shell_session` 後の prune で新規 session が残ること、`max_sessions` を超えないこと |
| `aish/src/main.rs` または同等の testable helper | `aish shell` が余計な引数を拒否すること |
| `ai/src/domain/shell_log_resolve.rs` | `validate_session_id()` が strict 12 桁小文字 hex を強制すること |
| `aibe-protocol/src/executed_tool.rs` | `approval_state` / `approval_source` を含む audit roundtrip |
| `aibe/src/adapters/outbound/tools/shell_exec.rs` | approval policy に応じた拒否・実行の分岐 |

### integration

| 対象 | 期待 |
|------|------|
| `aish/tests/` または `aish/src/main.rs` の既存 test harness | `aish shell` の usage / prune 順序 / session 生成の結合確認 |
| `aibe-client/tests/` | `agent_turn` transport が request/response を正しく往復すること |
| `aibe/tests/agent_turn_loop.rs` | `shell_exec_approval` の `never` / `ask` / `always` 経路 |
| `aibe/tests/socket_protocol.rs` | 必要なら approval 往復の wire 契約 |
| `ai/tests/ask_integration.rs` | `ai` 側の transport 呼び出しと stderr 表示の契約 |

### manual

| 文書 | 期待 |
|------|------|
| `docs/manual/aish-shell-log.md` | `aish shell` の session id / `AISH_SESSION_DIR` / `--session` strict validation を実機で確認できる |
| `docs/manual/ai-ask-tools.md` | `shell_exec` 承認 prompt、`stderr` 表示、`--session` 解決が実機で確認できる |

## docs 更新対象

| ファイル | 変更内容 |
|----------|----------|
| `docs/architecture.md` | `aish shell` の cleanup 順序、`--format` の strict 化、`aibe-client` transport の責務、`shell_exec_approval` の位置づけ |
| `docs/security.md` | `shell_exec` の実行前承認、audit、dangerous tool の扱い |
| `docs/testing.md` | P4 の unit / integration / manual の所在と役割 |
| `docs/manual/ai-ask-tools.md` | `shell_exec` 承認 prompt と `--session` strict validation |
| `docs/manual/aish-shell-log.md` | `AISH_SESSION_DIR` / `--session` の strict validation と session id 表記 |
| `docs/0000_spec-index.md` | 0020 を「進行中」として追加 |

## 未確定・推測・指示外

- `P4-4` のログ context 改善は指示外であり、本書では扱わない。
- `shell_exec_approval` の TOML 上の具体的な配置が `[tools]` 直下か `[tools.shell_exec]` 配下かは、実装時に現行設定モデルへ合わせる必要がある。**推測**としては `shell_exec` 設定に近い場所へ寄せるのが自然だが、本書では配置を固定しない。
- `shell_exec_approval = "ask"` の非対話 stdin 時の振る舞いは、本書では明示していない。実装時に fail-closed を採る場合は、manual と tests を同時に更新する。
- `shell_exec_approval = "ask"` の非対話 stdin 時は fail-closed とし、yes/no を受け取れない場合は拒否記録として返す。
- approval 往復に新しい protocol DTO が必要になるかどうかは、既存 wire で表現できるかの実装判断を要する。
- 承認往復は既存の `serve_connection` の 1 接続で複数 request/response を扱える前提に依存する。新規接続や別チャネルを前提にしない。

## 残リスク

- `shell_exec` の承認 UI は、人間の確認を伴うため manual 検証が不可欠。
- `approval_source` の文字列設計が粗いと、後続の監査やログ検索で再設計が必要になる。
- `aibe-client` の transport 集約は、既存 `ai` / `aibe` のテスト移設を伴うため、網羅漏れがあると回帰しやすい。
