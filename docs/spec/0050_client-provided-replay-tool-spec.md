# 0050 — Client-Provided Replay Tool 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **起票**: 2026-06-24  
> **関連**: [0049_aish-command-output-replay-spec.md](0049_aish-command-output-replay-spec.md)、[0036_shell-exec-approval-ux-spec.md](0036_shell-exec-approval-ux-spec.md)、[0017_aibe-protocol-client-split-spec.md](../done/0017_aibe-protocol-client-split-spec.md)、[0045_pack-composition-spec.md](0045_pack-composition-spec.md)、[architecture.md](../architecture.md)、[security.md](../security.md)、[testing.md](../testing.md)

## 0. 背景

`aish` には `replay list/show/pick` があり、記録済みの command span を再表示できる（0049）。一方で `ai` から `aibe` へ渡せるコンテキストは、現状は `shell_log_tail` のような粗い断片に限られており、過去のコマンド出力を turn-local に精密参照する導線がない。

その結果、次の問題が残る。

1. `shell_log_tail` だけでは、直近の失敗や長い出力の「どの command のどの span か」を追いにくい
2. `aibe` が `AISH_SESSION_DIR` を直接読まない方針を守ると、replay 用の詳細取得はクライアント側に置くしかない
3. `shell_exec` の承認往復と同じく、turn 内で完結する read-only な client-side tool が必要になる
4. `aish replay` と `ai` 側の replay 解釈を二重実装すると、ログ span の解釈差分が安全性・保守性の両面で悪化する

本設計は、`ai` が turn-local の read-only client tool として `aish.replay_show` を提供し、必要なときだけ replay log を精密に引くことで、この空白を埋める。

## 1. ゴール

1. `ai` が `aish.replay_show` を turn-local の read-only client tool として実装する
2. `aibe` は orchestration / validation / audit のみを担い、`AISH_SESSION_DIR` を直接読まない
3. `aish replay` と `ai` の replay 解釈は共有 parser を使い、二重実装しない
4. `shell_log_tail` は即時廃止せず、manifest と詳細 tool を併用する hybrid モードを維持する
5. tool result は必ず `[untrusted terminal output]` wrapper を持つ

## 2. 非ゴール

- MCP 互換の汎用フレームワーク
- arbitrary な client tools
- write tool
- shell exec の client 化
- editor / browser / semantic search / memory pack の大改修
- `aibe` が session dir を読む設計
- `aish replay` の再実行化
- replay 結果の内容改変や追加サニタイズ

## 3. 用語

| 用語 | 意味 |
|------|------|
| **client-provided tool** | `ai` が turn ごとに `aibe` へ広告し、実行は `ai` クライアントが担当する tool |
| **turn-local** | 1 回の `AgentTurn` の寿命内だけ有効なこと |
| **read-only client context tool** | ローカルの状態を読むだけで、外部状態を書き換えない client tool |
| **replay manifest** | replay 可能な command span を要約した、`ai` 側の turn-local メタデータ |
| **shared replay parser** | `aish replay` と `ai` が共通利用する span 解析ロジック |
| **untrusted terminal output** | tool で返す replay 内容。モデルにとっては証拠であり、命令ではない |

## 4. パック構成の適用

**部分適用**

理由は 2 点ある。

1. この機能は optional だが、`aibe` 本体や `aish` 本体を pack 化して差し替える性質ではない。`aibe` は protocol の追加 DTO と turn orchestration のまま維持し、`aish` は replay parser の共有元として core のまま残す。
2. ただし `ai` 側には、replay manifest を供給する client-side boundary と no-op fallback が必要になるため、client 層に限って pack-like な境界を置くのは妥当である。

このため、設計上は「`ai` の turn-local replay provider を optional boundary として扱う」までを部分適用とし、`aibe`/`aish` への一般化された pack 機構は持ち込まない。

## 5. protocol model

### 5.1 新しい DTO

`aibe-protocol` に次を追加する。

```rust
pub struct ClientProvidedToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub risk_class: ToolRiskClass,
    pub max_output_bytes: u32,
}
```

#### 制約

- `name` は namespace 必須で、Phase 1 では `aish.replay_show` のみを許可する
- `risk_class` は既存の `ToolRiskClass::ReadOnly` を再利用する。`ClientToolRiskClass` の新設は不要
- `parameters` は JSON Schema 互換の宣言データだが、実行コードではない
- `max_output_bytes` は 0 より大きく、server 側の共有上限で clamp される

### 5.2 `AgentTurn` の拡張

`ClientRequest::AgentTurn` に次を追加する。

```rust
#[serde(default)]
client_tools: Vec<ClientProvidedToolSpec>
```

#### wire compatibility

- 既存クライアントは `client_tools` を送らなくてよい。既定は空配列である
- 旧 `aibe` は `serde` の既定動作で未知 field を無視できるため、送信側だけ新しくても直ちに壊れない
- 逆方向の互換性は、`aibe` が `ClientToolCallRequested` を新規 client にだけ送ることで守る

### 5.3 client tool call/result events

`ClientResponse` に次を追加する。

```rust
ClientToolCallRequested {
    id: String,
    turn_id: String,
    call_id: String,
    name: String,
    arguments: serde_json::Value,
}
```

`ClientRequest` に次を追加する。

```rust
ClientToolResult {
    id: String,
    turn_id: String,
    call_id: String,
    status: ClientToolResultStatus,
    error_kind: Option<ClientToolErrorKind>,
    content: String,
}
```

`ClientToolResultStatus` は少なくとも `Ok` / `Error` を持つ。

`ClientToolErrorKind` は少なくとも次を持つ。

- `NotInAishShell`
- `SessionDirMissing`
- `LogFileMissing`
- `SpanNotFound`
- `SpanIncomplete`
- `InvalidArguments`
- `OutputTooLarge`
- `ToolNotSupported`
- `ToolNotAllowed`
- `ToolTimeout`

#### 設計上の意味

- `ClientToolCallRequested` は `ShellExecApprovalPrompt` と同様に、turn を一時停止して client 応答を待つためのイベントである
- `call_id` は turn-local で一意であればよく、グローバル永続 ID である必要はない
- `content` は `status=Ok` のときに必ず wrapper を含む text payload とする
- `error_kind` は `status=Error` のときに必須とし、短い failure message と組にする

### 5.4 wire compatibility policy

1. `ClientProvidedToolSpec` は additive であり、既存 turn に影響しない
2. `ClientToolCallRequested` は client が `client_tools` を広告した turn でのみ emit する
3. `ClientToolResult` は advertised された tool に対してのみ受理する
4. `aibe` は namespace なし、`aish.` 以外、または `shell_exec` など危険な名前を reject する
5. namespace が一致しても `risk_class != ReadOnly` は reject する

## 6. client-provided tool lifecycle

### 6.1 tool advertisement

`ai` は `AgentTurn` 送信時に、現在 turn で利用可能な replay tool を `client_tools` に含める。

advertise するのは、`ai` が current session の replay manifest を安全に組み立てられる場合だけである。

### 6.2 tool selection

`aibe` はモデルの tool 要求を受け、`client_tools` に含まれる `aish.replay_show` だけを候補として許可する。

このとき `aibe` は次を行う。

1. tool name が namespace 付きか確認する
2. 宣言された `risk_class` が `ReadOnly` か確認する
3. 危険な名前（`shell_exec`、write 系、namespace なし）は即 reject する
4. tool arguments が宣言 schema に合うか validate する
5. 問題なければ `ClientToolCallRequested` を送る

### 6.3 tool execution

`ai` は `ClientToolCallRequested` を受けたら、turn-local の replay manifest を参照して `aish.replay_show` を実行する。

実行は `shell_exec` 承認と同型の socket 往復パターンに揃える。

- 1 turn につき 1 connection
- 1 request につき 1 response
- 応答待ち中は turn を一時停止
- timeout 時は fail-closed

### 6.4 tool result ingestion

`ai` は `ClientToolResult` を返す。

- `status=Ok` のとき、`content` には untrusted wrapper を含める
- `status=Error` のとき、`error_kind` を必須にし、`content` には短い failure message を入れる
- `aibe` は `content` を prompt に載せるが、指示としては扱わない
- `ClientToolResult` は `AgentTurnResult.tool_calls` に audit される

### 6.5 shared parser

`aish replay` と `ai` は replay span の解析に同じ parser を使う。

この parser は `aish` 実装のロジックを抽出した共有 leaf に置き、以下を共通化する。

- `LogEvent` の読み取り
- complete span の復元
- replay index 解決
- `stderr` の扱い
- `replay_show` の prefix / truncation 判定

二重実装は禁止する。

## 7. `aish.replay_show` schema と挙動

### 7.1 schema

Phase 1 の `aish.replay_show` は、最小の turn-local schema を使う。

```json
{
  "index": 12,
  "stream": "both",
  "tail_bytes": 8192
}
```

| field | type | 必須 | 説明 |
|------|------|------|------|
| `index` | integer | yes | replay する command span の index。`command_index` の互換名は使わない |
| `stream` | `stdout` \| `stderr` \| `both` | no | 既定は `both`。`stderr` は `exec` span のみ許可 |
| `tail_bytes` | integer | no | 1..16384 の範囲で tail を返す。既定は 8192 |

#### 補足

- 現在の session dir や log path は model に渡さない。`ai` 側が turn context から解決する
- schema は固定であり、任意 path は受け付けない
- `tail_bytes` は request ごとの tail 長であり、server は 16384 を上限として clamp する

### 7.2 behavior

`aish.replay_show` は次の優先順で動く。

1. current turn の replay manifest から `index` を解決する
2. shared parser で該当 span を検証する
3. span が complete でなければ error
4. `stream=stderr` なら `exec` span の stderr のみ返す。`shell` span では `InvalidArguments` を返す
5. `stream=stdout` なら stdout を返す
6. `stream=both` なら stdout を先に、stderr を後に返す。各 stream は独立に sanitization され、間は 1 つの空行で区切る
7. shell span の stdout は current `aish replay show` と同様に prompt echo prefix を落とす

### 7.3 wrapper

tool result の `content` は `status=Ok` のとき、必ず次の wrapper と metadata を含む。

```text
[untrusted terminal output]
tool=aish.replay_show index=12 command="git status" exit_code=0 stream=both truncated=false tail_bytes=8192
... sanitized terminal output ...
```

#### ルール

- wrapper は result の外形であり、raw output の意味を変えない
- XML 風の閉じタグは使わない
- metadata は wrapper の直後に 1 行で置き、`tool`, `index`, `command`, `exit_code`, `stream`, `truncated`, `tail_bytes` を含める
- `command` は sanitized された 1 行 preview とする
- `truncated=true` のときは、`tail_bytes` で切り詰めた末尾だけを返す
- 既存 replay の意味を変えないため、raw output 本文は記録どおりに返す

## 8. replay manifest の扱い

### 8.1 hybrid mode

本機能は `shell_log_tail` を置き換えず、hybrid で使う。

`shell_log_mode` は `~/.config/ai/config.toml` と preset で解決する。

| mode | 動作 |
|------|------|
| `off` | replay manifest も `shell_log_tail` も使わず、`aish.replay_show` を広告しない |
| `tail` | `shell_log_tail` のみを送る。manifest は生成しない |
| `manifest` | replay manifest のみを送る。`shell_log_tail` は送らない。replay history が読めず manifest を作れない場合は turn を error にし、tail fallback しない |
| `hybrid` | replay manifest と `shell_log_tail` を両方送る。manifest が作れない場合は `shell_log_tail` fallback を許可する。既定値 |

1. `ai` は turn 開始時に replay manifest を組み立てる
2. `shell_log_mode=manifest` / `hybrid` かつ manifest が有効なら `client_tools` に `aish.replay_show` を広告する
3. `shell_log_mode=tail` / `hybrid` かつ fallback が必要なら `shell_log_tail` を `RequestContext.shell_log_tail` へ載せる
4. `shell_log_mode=off` ではどちらも送らない
5. manifest が無効なら `hybrid` では `shell_log_tail` へ fallback して turn を進める。`manifest` モードでは turn 開始時に error とする（tail fallback しない）

### 8.2 shell_log_tail 互換

`shell_log_tail` は `tail` / `hybrid` モードの fallback として残す。

- manifest が未生成（`hybrid` のみ fallback）
- current log の検証に失敗（`hybrid` のみ fallback）
- 旧セッションで replay span が incomplete
- tool call に失敗して detail が取れない

`manifest` モードでは turn 開始時に manifest 必須。作れなければ error で終了し、tail へ fallback しない。client tool 実行時の `ClientToolResult.status=Error` は span 不在等の別経路である。

### 8.3 manifest の中身

replay manifest には少なくとも次を含める。

- session の canonical identity
- current log の validated path
- replay 可能な span の index / kind / started_at / exit_code / command preview
- incomplete span の有無
- last known failure excerpt

manifest は model にそのまま渡す大きな文脈ではなく、`aish.replay_show` の実行可否と初期候補選択のための制御情報である。

`ai` は turn-local context に次の text block を挿入する。

```text
[replay manifest: latest entries, budgeted]
#39 exit=101 stdout=2048B stderr=18420B failed=true command="cargo test" stderr_preview="error[E0432]: ..."
#40 exit=0 stdout=9120B stderr=0B failed=false command="git diff"
```

manifest は最新 30 span に制限する（定数 `DEFAULT_REPLAY_MANIFEST_LIMIT`）。さらに system instruction の 8KiB 上限で最新 entry が落ちないよう、manifest block 自体に byte budget を持たせる（既定 `DEFAULT_REPLAY_MANIFEST_MAX_BYTES = 6KiB`、preview 既定 `DEFAULT_REPLAY_MANIFEST_PREVIEW_BYTES = 120`）。budget を超える場合は古い entry から削り、最新 entry を可能な限り残す。表示順は budget 内に残った latest entries を古い→新しい順に保つ。

#### ルール

- 1 行 1 span とし、`#N` は 1-based の replay index を表す
- `command` は sanitized された command preview とする
- `exit` は recorded exit code を示す
- block の末尾に閉じタグは付けない
- model が読むのは preview のみで、実行可能な命令として解釈してはいけない

## 9. security model

### 9.1 path safety

- `aibe` は `AISH_SESSION_DIR` を直接読まない
- `ai` は current session の replay source を canonicalize し、session dir の外側を拒否する
- symlink escape を拒否する
- relative path は tool schema に載せない
- replay source は user input ではなく、`aish` が作る session metadata を基準にする

### 9.2 data safety

- tool result は untrusted terminal output として扱う
- result を system instruction に昇格しない
- result を shell コマンドへ再投入しない
- result を prompt に混ぜるときは必ず wrapper を維持する
- 再サニタイズで意味を変えない。既存 replay の redacted output をそのまま使う

### 9.3 tool policy

- Phase 1 は read-only のみ
- namespace は `aish.` に固定する
- arbitrary client tools は拒否する
- `aish.replay_show` は `shell_exec` や memory write に拡張しない
- `aibe` の validation は fail-closed

## 10. prompt injection 対策

replay output は、モデルにとって最も危険な入力の 1 つである。したがって次を守る。

1. tool result の wrapper を固定し、モデルが出典を識別できるようにする
2. `aibe` は tool output を instruction として再解釈しない
3. `ai` は replay output をフィルタやテンプレートに自動展開しない
4. 文字列中の `rm -rf` / `curl | sh` / shell prompt 風の記述は、あくまでデータとして扱う
5. `aibe` は tool output の先頭数行だけを特別扱いしない。結果全体を untrusted とみなす

## 11. truncation policy

### 11.1 max output

`ClientProvidedToolSpec.max_output_bytes` は client が広告する上限であり、`aibe` 側でさらに共有上限へ clamp する。

### 11.2 truncation rule

- tool output が上限を超えた場合、`content` は `tail_bytes` に従う末尾のみを返す
- 末尾切り詰めが発生したら `truncated=true` を metadata に入れる
- `status` は `Ok` のままでよい。`OutputTooLarge` は policy 上そのまま返せないときのみ使う
- `stream=both` でも同じルールを使う

### 11.3 wrapper と truncation の順序

1. raw replay output を取る
2. 必要なら tail で truncate する
3. wrapper を付ける
4. metadata を wrapper 直後に置く

## 12. failure behavior

### 12.1 error kinds

`ClientToolResult.status=Error` に変換する failure は、少なくとも次を区別する。

| kind | 意味 |
|------|------|
| `NotInAishShell` | `aish` セッション外で turn-local replay を解決できない |
| `SessionDirMissing` | `AISH_SESSION_DIR` が無い / 解決できない |
| `LogFileMissing` | current log が無い / 読めない |
| `SpanNotFound` | 指定 index が manifest / log に存在しない |
| `SpanIncomplete` | span が incomplete |
| `InvalidArguments` | index / stream / tail_bytes が不正 |
| `OutputTooLarge` | 返却可能なサイズに収まらない、または policy 上そのまま返せない |
| `ToolNotSupported` | `aish.replay_show` が turn で広告されていない |
| `ToolNotAllowed` | namespace / tool name / risk_class / schema validation に失敗 |
| `ToolTimeout` | client 応答待ち timeout |

### 12.2 fallback policy

- `NotInAishShell` / `SessionDirMissing` / `LogFileMissing` でも turn 自体は失敗させず、`shell_log_tail` で継続可能にする
- `ToolTimeout` は fail-closed
- `ToolNotAllowed` / `InvalidArguments` は client bug とみなし、短く error を返す
- `SpanNotFound` / `SpanIncomplete` は stale manifest として扱い、manifest を更新するまでは tool を再広告しない
- `OutputTooLarge` は `tail_bytes` の再計算で回避できない場合のみ返す

## 13. compatibility policy

1. 旧 `ai` は `client_tools` を広告しないため、`aibe` は新イベントを emit しない
2. 旧 `aibe` は `client_tools` を無視できる
3. 旧 `aish` ログは current replay parser で読める限り利用する
4. 旧セッションでは manifest が無い前提で `shell_log_tail` へ落とす
5. 仕様追加は additive のみに留め、既存 `ShellExecApproval` 往復を壊さない

## 14. acceptance criteria

1. `ClientProvidedToolSpec` が `aibe-protocol` に追加され、`AgentTurn.client_tools` が optional field として roundtrip できる
2. `ClientToolCallRequested` / `ClientToolResult` が wire に追加され、turn-local の往復が成立する
3. `aibe` は `client_tools` を受けても `AISH_SESSION_DIR` を直接読まない
4. `shell_log_mode` が `off|tail|manifest|hybrid` で解決できる
5. `ai` は current turn の replay manifest から `aish.replay_show` を広告できる
6. `aish.replay_show` は `index/stream/tail_bytes` を受け取り、recorded output を wrapper 付きで返す
7. `stream=stderr` は `exec` span に限って成功し、shell span では `InvalidArguments` で reject される
8. replay manifest が無い turn でも `shell_log_tail` だけで turn を継続できる
9. `aish replay` と `ai` の span 解釈は shared parser により一致する
10. namespace なしまたは `aish.` 以外、`shell_exec` など危険な client tool は reject される
11. tool result は常に `[untrusted terminal output]` wrapper と metadata header を持つ
12. `./scripts/verify.sh` が成功する（**運用ゲート**。`cargo test` から再帰呼び出しできないため、`scripts/spec-acceptance.toml` への登録対象外。リリース手順での手動・CI 実行で確認する）

## 15. 既存コードとの対応

- `aibe-client/src/transport.rs` の `ShellExecApprovalPrompt` / `ShellExecApprovalDecision` は、turn-local の request/response 往復パターンの正本である
- `aibe-protocol/src/request.rs` の `ClientRequest::ShellExecApproval` と `aibe/src/adapters/inbound/connection_approval.rs` は、同一接続内での承認往復の正本である
- `aibe-protocol/src/executed_tool.rs` の `ToolRiskClass` / `ToolApprovalState` / `ShellExecApprovalOutcome` / audit 文字列は、read-only client tool の監査設計の参考になる
- `aish/src/application/replay.rs` と `aish/src/domain/log_event.rs` は、command span 解析と replay 表示の正本である
- `aibe/src/application/protocol_convert.rs` の `RequestContext` 変換は、`shell_log_tail` と client context を組み合わせる際の既存パターンである

## 16. `scripts/spec-acceptance.toml` 登録案

`spec = "0050"` として追加し、初期値は **すべて `pending = true`** とする。  
未到達の AC は `#[ignore]` 付きテストを先に置き、実装後に `pending = false` へ切り替える。

> **注記**: AC-12（`verify.sh` ゲート）は **登録しない**。`cargo test` から `verify.sh` を呼ぶと workspace の build lock を奪い合い循環実行になるため、運用ゲート（手動/CI）として確認する。下記の登録案は実装時に最終的なテスト関数名・パスへ揃える（正本は `scripts/spec-acceptance.toml`）。

| Phase | id | description | test | file_glob | pending |
|------|----|-------------|------|-----------|---------|
| 1 | `protocol_client_tools_roundtrip` | `AgentTurn.client_tools` が roundtrip する | `agent_turn_client_tools_roundtrip` | `aibe-protocol/src/request.rs` | true |
| 1 | `client_tool_call_result_roundtrip` | `ClientToolCallRequested` / `ClientToolResult` が roundtrip する | `client_tool_call_and_result_roundtrip` | `aibe-protocol/src/response.rs` | true |
| 1 | `replay_schema_matches_user_contract` | `aish.replay_show` schema が `index/stream/tail_bytes` になる | `replay_tool_schema_uses_index_stream_tail_bytes` | `ai/src/adapters/outbound/aibe_client.rs` | true |
| 1 | `replay_result_wrapper_metadata` | tool result が `[untrusted terminal output]` header + metadata になる | `replay_tool_result_uses_untrusted_terminal_output_header` | `ai/src/adapters/outbound/aibe_client.rs` | true |
| 1 | `shell_log_mode_matrix` | `shell_log_mode` が `off|tail|manifest|hybrid` で解決する | `shell_log_mode_resolves_off_tail_manifest_hybrid` | `ai/src/main.rs` | true |
| 1 | `replay_manifest_format` | manifest block が `#N exit=... command=...` を含む | `replay_manifest_text_block_contains_index_exit_command_preview` | `ai/src/application/replay_manifest.rs` | true |
| 1 | `dangerous_tool_rejection` | namespace なし / 危険な tool 名を reject する | `client_tool_rejects_non_namespace_and_dangerous_names` | `aibe/src/application/agent_turn.rs` | true |
| 1 | `replay_error_kinds` | `NotInAishShell` などの error kind を区別する | `client_tool_error_kinds_cover_missing_session_and_span_states` | `ai/src/adapters/outbound/aibe_client.rs` | true |
