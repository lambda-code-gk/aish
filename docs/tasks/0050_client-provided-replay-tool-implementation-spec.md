# 0050 — Client-Provided Replay Tool 実装指示書

> **種別**: 実装指示書（`docs/tasks/`）  
> **設計の正本**: [0050_client-provided-replay-tool-spec.md](../spec/0050_client-provided-replay-tool-spec.md)  
> **状態**: 実装指示書  
> **起票**: 2026-06-24  
> **関連**: [architecture.md](../architecture.md)、[security.md](../security.md)、[testing.md](../testing.md)、[aish-command-output-replay.md](../manual/aish-command-output-replay.md)、[`scripts/spec-acceptance.toml`](../../scripts/spec-acceptance.toml)、[`docs/0000_spec-index.md`](../0000_spec-index.md)

## 0. 目的

`docs/spec/0050_client-provided-replay-tool-spec.md` を満たすために、`ai` が turn-local の read-only client tool `aish.replay_show` を広告・実行し、`aibe` は validation と audit のみを担う実装を追加する。  
`aibe` は `AISH_SESSION_DIR` を直接読まず、`ai` 側で構築した replay manifest と `shell_log_tail` の hybrid 互換を維持する。  
`aish replay` と `ai` の replay 解釈は共有 parser を使い、二重実装を避ける。`ai/src/main.rs` は肥大化させず、replay manifest / client tool executor / validation を分割モジュールへ逃がす。

## 1. パック構成の適用

**部分適用**。

理由は設計書どおり 2 点ある。

1. `aibe` と `aish` は pack 化して差し替える対象ではない。`aibe` は protocol と turn orchestration の拡張で足り、`aish` は replay parser の共有元として core のまま残す。
2. `ai` には turn-local replay provider と no-op fallback が必要なので、client 層に限って optional boundary を切るのは妥当である。

したがって、pack-like boundary は `ai` の client tool provider までに限定し、`aibe` / `aish` へ一般化した pack 機構は持ち込まない。

## 2. Phase 分割

| Phase | 内容 | ゲート（`scripts/spec-acceptance.toml`） |
|-------|------|------------------------------------------|
| 1 | protocol DTO 追加、shared replay parser 抽出、replay manifest 生成、client tool executor、aibe の tool validation、`ai` の wiring 分割、tests と docs 同期をまとめて実施する。`shell_log_tail` 互換を壊さず、Phase 1 だけで完結させる。 | Phase 1 の AC がすべて `pending = false` になるまで完了扱いにしない |

## 3. 変更ファイル一覧

### 3.1 `aibe-protocol`

| パス | 役割 |
|------|------|
| `aibe-protocol/src/request.rs` | `ClientProvidedToolSpec`、`AgentTurn.client_tools`、`ClientToolResult` を追加する。`RequestContext` / `ClientRequest` の serde 後方互換を維持する。 |
| `aibe-protocol/src/response.rs` | `ClientToolCallRequested`、`ClientToolResultStatus`、`ClientToolErrorKind`、必要なら client tool audit 用の enum を追加する。`AgentTurnResult.tool_calls` の既存表現を壊さない。 |
| `aibe-protocol/src/executed_tool.rs` | read-only client tool の audit 文字列と decision 文字列を追加し、shell approval と同じ監査表現へ寄せる。 |
| `aibe-protocol/src/lib.rs` | 新 DTO / enum の re-export と共通定数の追加。 |

### 3.2 `aibe-client`

| パス | 役割 |
|------|------|
| `aibe-client/src/transport.rs` | `ClientToolCallRequested` を受けて `ClientToolResult` を返す turn-local 往復を追加する。`ShellExecApproval` と同じ request/response パターンを使う。 |
| `aibe-client/src/lib.rs` | 新しい transport API の re-export と、client tool 用の補助型を公開する。 |
| `aibe-client/tests/client_tool_roundtrip.rs` | turn-local client tool 往復の回帰を固定する新規テスト。 |

### 3.3 `aibe`

| パス | 役割 |
|------|------|
| `aibe/src/application/agent_turn.rs` | `client_tools` を受け取り、LLM tool request を validation して `ClientToolCallRequested` を emit する。namespace / risk / schema を fail-closed で判定する。 |
| `aibe/src/application/tool_defs.rs` | `aish.replay_show` の schema 定義を client tool 版へ切り出す。既存 builtin tool 定義とは分離する。 |
| `aibe/src/application/protocol_convert.rs` | `RequestContext` の変換に replay manifest 由来の turn-local 情報を載せる際の境界を整える。 |
| `aibe/src/application/server.rs` | `client_tools` を受ける composition root の配線を追加する。 |
| `aibe/src/adapters/inbound/connection_approval.rs` | shell approval と同型の socket 往復を参考に、client tool 往復の読み書き順を壊さないようにする。 |
| `aibe/src/adapters/outbound/tools/registry.rs` | read-only client tool を builtin tool と混ぜず、allowlist 側で拒否・監査しやすくする。 |
| `aibe/tests/client_tool_socket.rs` | namespace なし / `shell_exec` / write 系の reject、read-only `aish.replay_show` の accept を固定する。 |
| `aibe/tests/request_tool_validation.rs` | `client_tools` が turn に入ったときの validation / fallback の回帰を追加する。 |

### 3.4 `ai`

| パス | 役割 |
|------|------|
| `ai/src/main.rs` | `client_tools` の広告・実行・fallback を細い配線に留め、実際のロジックは `application/client_tools/` と `application/replay_manifest/` に逃がす。 |
| `ai/src/application/client_tools/mod.rs` | client tool boundary の入口。turn-local tool の可否判定と executor 生成をまとめる。 |
| `ai/src/application/client_tools/replay_show.rs` | `aish.replay_show` の request 構築、入力検証、wrapper / metadata 組み立てを担当する。 |
| `ai/src/application/replay_manifest.rs` | `shell_log_mode` 解決、manifest block 生成、canonical path 検証、fallback 判定を担当する。 |
| `ai/src/application/replay_source.rs` | shared parser へ流す raw span source の読み出しと session ルールをまとめる。 |
| `ai/src/domain/client_tools.rs` | turn-local client tool の型、error kind、mode 解決結果を domain に閉じる。 |
| `ai/src/adapters/outbound/aibe_client.rs` | `ClientToolCallRequested` / `ClientToolResult` を扱う transport hook を追加する。 |
| `ai/src/adapters/outbound/stdout_presenter.rs` | 必要最小限の表示更新のみ行う。client tool の内容は誇張表示しない。 |
| `ai/tests/client_tools_replay.rs` | manifest / wrapper / error kind / fallback の統合寄り回帰を置く。 |
| `ai/tests/history_cli.rs` | `shell_log_tail` の互換性回帰を維持する。 |
| `ai/Cargo.toml` | 共有 parser の依存解決が必要ならここで調整する。 |

### 3.5 `aish`

| パス | 役割 |
|------|------|
| `aish/src/domain/replay_parser.rs`（新規） | `aish replay` と `ai` が共通利用する shared replay parser の core。span grouping / index resolution / stdout・stderr の扱いを切り出す。 |
| `aish/src/application/replay.rs` | 既存 replay 実装を shared parser 利用へ寄せ、`show/list/pick` の正本を維持する。 |
| `aish/src/application/mod.rs` | replay parser の公開を追加する。 |
| `aish/src/lib.rs` | shared parser の re-export を追加する。 |
| `aish/src/main.rs` | replay 関連 CLI の既存挙動を変えずに parser 参照先を差し替える。 |
| `aish/tests/exec_log.rs` | shared parser の span 解釈と `replay_show` 契約の回帰を追加する。 |
| `aish/tests/shell_interactive.rs` | `aish shell` から作られる replay への影響を確認する。 |

### 3.6 docs / index / manual

| パス | 役割 |
|------|------|
| `docs/architecture.md` | protocol、dependency rule、`shell_log_mode`、client tool 往復、shared parser の境界を更新する。 |
| `docs/manual/aish-command-output-replay.md` | 既存 replay manual に `client_tools` 互換と replay source の前提を追記する。 |
| `docs/manual/ai-client-provided-replay-tool.md`（新規） | `shell_log_mode` と `aish.replay_show` の手動確認手順を分離して書く。 |
| `docs/0000_spec-index.md` | `docs/tasks/` の 0050 を追加する。 |
| `scripts/spec-acceptance.toml` | 実装時に 0050 の AC を 1:1 で登録する。 |

## 4. 実装順序

### 4.1 protocol

1. `aibe-protocol` に `ClientProvidedToolSpec`、`AgentTurn.client_tools`、`ClientToolCallRequested`、`ClientToolResult`、`ClientToolResultStatus`、`ClientToolErrorKind` を追加する。
2. `ShellExecApproval` と同様に serde 後方互換を保つ。既存クライアントは `client_tools` を送らなくても動き、既存 `aibe` は新 field を無視できる形にする。
3. `aibe-protocol/src/lib.rs` で新型を re-export する。
4. `aibe-client/src/transport.rs` に turn-local 往復を追加し、`ShellExecApproval` と同じ NDJSON request/response パターンをそのまま再利用する。

### 4.2 shared replay parser

1. `aish/src/application/replay.rs` から span grouping / index resolution / `stderr` 制約 / prompt echo prefix stripping を抽出する。
2. 抽出先は `aish/src/domain/replay_parser.rs` を正本にし、`aish` と `ai` が同じ関数群を使う。
3. ここでは raw replay output を再サニタイズしない。`aish replay` の現在の意味を変えないことを最優先にする。
4. `ai` 側は parser を自前で再実装せず、`aish` 側の shared leaf を呼ぶだけにする。

### 4.3 manifest

1. `ai/src/application/replay_manifest.rs` で `shell_log_mode = off|tail|manifest|hybrid` を解決する。
2. current session の replay source は canonicalize し、session dir の外側と symlink escape を拒否する。
3. manifest は turn-local の制御情報であり、model にそのまま渡す raw log ではない。`#N exit=... command="..."` の短い block に落とす。
4. `manifest` / `hybrid` で manifest が有効なときだけ `client_tools` に `aish.replay_show` を広告する。
5. manifest が失効・欠落したときは `shell_log_tail` fallback を使うか、`manifest` mode なら `ClientToolResult.status=Error` に倒す。

### 4.4 client tool executor

1. `ai/src/application/client_tools/` に turn-local executor を分割する。
2. `ClientToolCallRequested` を受けたら、`aish.replay_show` だけを処理する。
3. namespace なし、`aish.` 以外、`shell_exec`、write 系、read-only 以外の risk は即 reject する。
4. arguments は固定 schema に合わせて validate し、`index / stream / tail_bytes` 以外は受けない。
5. 成功時は `[untrusted terminal output]` wrapper と metadata を必ず付ける。
6. 失敗時は短い error message と `ClientToolErrorKind` を返し、任意 path や機密情報を error 文字列に混ぜない。
7. 実行は `ShellExecApproval` と同型の socket 往復パターンに揃え、turn を一時停止して client 応答を待つ。

### 4.5 aibe loop

1. `aibe/src/application/agent_turn.rs` で `client_tools` を turn 入力として受け取る。
2. tool 要求が `aish.replay_show` 以外なら fail-closed で拒否する。
3. `ToolRiskClass::ReadOnly` 以外は拒否する。read-only でも schema 不一致なら拒否する。
4. `ExecutedToolCall` の audit は既存 shell_exec のパターンを参照しつつ、client tool 由来であることが分かる文字列を残す。
5. `aibe` は `AISH_SESSION_DIR` を読まない。replay source の解決は client 側責務であり、server で補完しない。

### 4.6 ai wiring

1. `ai/src/main.rs` から replay manifest 生成、client tool 可否判定、tool result 組み立てを外へ出す。
2. `ai/src/application/replay_manifest.rs` と `ai/src/application/client_tools/` が主導し、`main.rs` は Option の結線だけにする。
3. `ai/src/adapters/outbound/aibe_client.rs` は transport と executor のアダプタに徹し、判定ロジックを持たない。
4. `shell_log_tail` の既存経路は残し、manifest が使えない turn でも ask/chat/retry/rerun は継続できるようにする。

### 4.7 tests

1. `aibe-protocol` の serde roundtrip を unit で固定する。
2. `aibe-client` は turn-local client tool 往復の unit / stream テストを追加する。
3. `aibe` は socket / validation の integration-ish テストで reject / accept を固定する。
4. `ai` は manifest 生成、wrapper 文字列、error kind、`shell_log_tail` fallback を integration-ish で固定する。
5. `aish` は shared parser の span 解釈と旧 replay の互換を unit / integration で固定する。

## 5. 受け入れ条件

設計書 §14 をそのままテストに落とす。各 AC は 1:1 で `scripts/spec-acceptance.toml` に登録する。

| ID | 条件 | テスト関数 | pending |
|----|------|------------|---------|
| AC-01 | `ClientProvidedToolSpec` が `aibe-protocol` に追加され、`AgentTurn.client_tools` が optional field として roundtrip できる | `agent_turn_client_tools_roundtrip` | true |
| AC-02 | `ClientToolCallRequested` / `ClientToolResult` が wire に追加され、turn-local の往復が成立する | `client_tool_call_and_result_roundtrip` | true |
| AC-03 | `aibe` は `client_tools` を受けても `AISH_SESSION_DIR` を直接読まない | `aibe_does_not_read_aish_session_dir_for_client_tools` | true |
| AC-04 | `shell_log_mode` が `off|tail|manifest|hybrid` で解決できる | `shell_log_mode_resolves_off_tail_manifest_hybrid` | true |
| AC-05 | `ai` は current turn の replay manifest から `aish.replay_show` を広告できる | `replay_manifest_advertises_aish_replay_show` | true |
| AC-06 | `aish.replay_show` は `index/stream/tail_bytes` を受け取り、recorded output を wrapper 付きで返す | `replay_show_returns_untrusted_terminal_output_wrapper` | true |
| AC-07 | `stream=stderr` は `exec` span に限って成功し、shell span では `InvalidArguments` で reject される | `replay_show_rejects_shell_span_stderr` | true |
| AC-08 | replay manifest が無い turn でも `shell_log_tail` だけで turn を継続できる | `shell_log_tail_fallback_keeps_turn_running_without_manifest` | true |
| AC-09 | `aish replay` と `ai` の span 解釈は shared parser により一致する | `shared_replay_parser_matches_aish_replay_output` | true |
| AC-10 | namespace なしまたは `aish.` 以外、`shell_exec` など危険な client tool は reject される | `client_tool_validation_rejects_non_namespace_and_dangerous_names` | true |
| AC-11 | tool result は常に `[untrusted terminal output]` wrapper と metadata header を持つ | `replay_tool_result_always_includes_untrusted_terminal_output_header` | true |
| AC-12 | `./scripts/verify.sh` が成功する | （運用ゲート。`spec-acceptance.toml` には登録しない） | — |

## 6. `scripts/spec-acceptance.toml` 登録案

`spec = "0050"` として追加し、初期値は **すべて `pending = true`** とする。  
未到達の AC は `#[ignore]` 付きテストを先に置き、実装後に `pending = false` へ切り替える。

| Phase | id | description | test | file_glob | pending |
|------|----|-------------|------|-----------|---------|
| 1 | `protocol_client_tools_roundtrip` | `AgentTurn.client_tools` が roundtrip する | `agent_turn_client_tools_roundtrip` | `aibe-protocol/src/request.rs` | true |
| 1 | `client_tool_call_result_roundtrip` | `ClientToolCallRequested` / `ClientToolResult` が roundtrip する | `client_tool_call_and_result_roundtrip` | `aibe-protocol/src/response.rs` | true |
| 1 | `aibe_no_session_dir_read` | `aibe` が `AISH_SESSION_DIR` を直接読まない | `aibe_does_not_read_aish_session_dir_for_client_tools` | `aibe/src/application/agent_turn.rs` | true |
| 1 | `shell_log_mode_matrix` | `shell_log_mode` が `off|tail|manifest|hybrid` で解決する | `shell_log_mode_resolves_off_tail_manifest_hybrid` | `ai/src/application/replay_manifest.rs` | true |
| 1 | `manifest_advertisement` | manifest から `aish.replay_show` を広告する | `replay_manifest_advertises_aish_replay_show` | `ai/src/application/replay_manifest.rs` | true |
| 1 | `replay_wrapper_metadata` | wrapper + metadata header が付く | `replay_tool_result_always_includes_untrusted_terminal_output_header` | `ai/src/application/client_tools/replay_show.rs` | true |
| 1 | `stderr_rejection` | shell span の `stderr` 要求を拒否する | `replay_show_rejects_shell_span_stderr` | `aish/src/application/replay.rs` | true |
| 1 | `tail_fallback` | manifest 無しでも `shell_log_tail` で継続する | `shell_log_tail_fallback_keeps_turn_running_without_manifest` | `ai/src/adapters/outbound/aibe_client.rs` | true |
| 1 | `shared_parser_alignment` | `aish` と `ai` の span 解釈が一致する | `shared_replay_parser_matches_aish_replay_output` | `aish/src/domain/replay_parser.rs` | true |
| 1 | `tool_rejection` | namespace なし / 危険な tool 名を reject する | `client_tool_validation_rejects_non_namespace_and_dangerous_names` | `aibe/src/application/agent_turn.rs` | true |
| 1 | `error_kind_matrix` | client tool の error kind を区別する | `client_tool_error_kinds_cover_missing_session_and_span_states` | `ai/src/application/client_tools/replay_show.rs` | true |

## 7. `docs/architecture.md` 更新箇所

実装と同じ PR で次を更新する。

1. **依存ルール**
   - `ai` の依存に shared replay parser を追加するか、parser-only の narrow dependency を許可する記述に更新する。
   - 既存の `ai → aibe-protocol, aibe-client のみ` という表現は、client tool replay については例外を明示する。
2. **プロトコル節**
   - `ClientProvidedToolSpec`、`ClientToolCallRequested`、`ClientToolResult`、`client_tools` の wire 形状を追加する。
   - `ShellExecApproval` と同じく turn-local request/response 往復であることを明記する。
3. **`shell_log_tail` / replay 節**
   - `shell_log_mode = off|tail|manifest|hybrid` を追加し、manifest と fallback の関係を明文化する。
   - `aibe` は `AISH_SESSION_DIR` を直接読まないことを再掲する。
4. **aish 章**
   - shared replay parser の正本と `aish replay` の関係を追記する。

## 8. 手動検証手順の草案

新規 manual `docs/manual/ai-client-provided-replay-tool.md` を追加し、次の手順を載せる。

### 前提

- `cargo build -p aish -p ai -p aibe`
- `aish shell` の手動確認は TTY が必要
- `AISH_SESSION_DIR` があるときとないときの両方を試す

### 手順 1: replay manifest の確認

1. `aish shell` で session を作る。
2. `echo hello` と `cargo test -j 1` のような span を作る。
3. `ai ask` を実行し、`shell_log_mode=manifest` か `hybrid` のときに manifest block が表示されることを確認する。
4. manifest に `#N exit=... command="..."` 形式が含まれることを確認する。

### 手順 2: client tool 往復の確認

1. `ai ask` で `aish.replay_show` が広告される条件を作る。
2. `ClientToolCallRequested` が来たら `aish.replay_show` を実行し、wrapper 付きの出力がモデルへ戻ることを確認する。
3. `stream=stderr` を shell span に対して要求したとき、`InvalidArguments` 相当で拒否されることを確認する。

### 手順 3: fallback の確認

1. `AISH_SESSION_DIR` を外した状態で `ai ask` を実行する。
2. manifest が無い場合でも `shell_log_tail` だけで turn が継続することを確認する。
3. `manifest` mode では fallback ではなく error 経路になることを確認する。

### 手順 4: shared parser の確認

1. `aish replay list/show` と `ai` 側の manifest 解釈が同じ span index を指すことを確認する。
2. shell span の stdout で prompt echo prefix の扱いが一致することを確認する。

## 9. 仕様との差分

- なし。設計書の AC をそのまま実装指示へ落とす。
