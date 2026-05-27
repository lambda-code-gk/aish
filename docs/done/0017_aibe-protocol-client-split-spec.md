# 0017 — `aibe-protocol` / `aibe-client` 分離指示書 — 仕様ドラフト

> **出典**: `docs/todo/chatgpt-review-4th-gen/p1-protocol-split.md`、`implementation-order.md`、`concerns.md` §3。  
> **レビュー**: Codex 仕様レビュー 1 回（2026-05-28）。初回「要修正後着手」指摘を本版に反映済み。  
> **状態**: **実装済み**（2026-05-28）

## 目的

Sprint 2（P1）は、別フロントエンドや将来の GUI / TUI / MCP クライアントを見据えて、`aibe` 本体に閉じていた wire 契約と socket 通信を切り出す段階である。ここで境界を先に固める理由は次の 3 点。

1. `ai` が `aibe` 本体へ直依存したままだと、将来のフロントエンド追加時に実装・テスト・依存の全てが `aibe` に吸い寄せられる。
2. wire DTO と transport を先に独立させると、`aibe` の server / LLM / agent loop / tools を保守しながら、クライアント実装を別クレートで増やせる。
3. 依存方向を明確にしておくと、`check-architecture.sh` で「クライアントが本体へ戻る」逆流を機械的に防げる。

## 背景（現状）

workspace は `aibe` / `aish` / `ai` の 3 クレートのみ（`Cargo.toml` の `members`）。`aibe` が wire・transport・server を同居させ、`lib.rs` で `client` / `protocol` / `domain` を公開している。

### wire DTO（`aibe/src/protocol/`）

| モジュール | 内容 |
|------------|------|
| `protocol/mod.rs` | `ClientRequest` / `ClientResponse` / `ErrorCode` / `AgentTurnStatus` / `ProtocolMessageOut` の re-export |
| `protocol/request.rs` | `ClientRequest`、`ProtocolMessage`、`RequestContext`（wire 文字列）。**`TryFrom<RequestContext> for AgentTurnContext` は domain 変換のため `aibe` 側に残す**（移設時は `aibe-protocol` に domain 依存を持ち込まない） |
| `protocol/response.rs` | `ClientResponse::AgentTurnResult` 等。**フィールド型として `ExecutedToolCall` を参照するが、型定義の正本は `aibe/src/domain/tool.rs`**（`protocol` は domain を import している） |

### domain・契約再 export（`aibe/src/domain/` + `lib.rs`）

| 型・定数 | 正本（現状） | 備考 |
|----------|--------------|------|
| `ToolName`, `KNOWN_TOOLS`, `READ_FILE`, `SHELL_EXEC`, `is_known_tool` | `domain/tool_name.rs` | `lib.rs` から再 export |
| `ExecutedToolCall`, `ExecutedToolStatus` | `domain/tool.rs` | wire JSON にも載るが domain モジュール定義 |
| `ShellLogTail`, `ShellLogTail::MAX_BYTES`（16 KiB） | `domain/shell_log_tail.rs` | `RequestContext.shell_log_tail` の truncate 規約 |
| `AgentTurnContext`, `ChatMessage`, `ClientCwd`, `MessageRole` | `domain/*` | server 内部のみ（クライアント非公開） |

### transport（`aibe/src/client/mod.rs`）

- `ping` / `ping_result` / `ensure_running` — `crate::protocol::{ClientRequest, ClientResponse}` を使用。
- `default_socket_path()` は **`lib.rs`** にあり、`ai` の `toml_config` が参照。

### config 定数（`aibe/src/ports/outbound/config.rs`）

- `DEFAULT_MAX_TOOL_OUTPUT_BYTES`（32 KiB）— server の `ToolsConfig` 既定。`ai` の `stdout_presenter` も表示 truncate に利用。

### `ai` が `aibe` から参照しているもの（移行対象）

| 区分 | 参照箇所 | 現状の import |
|------|----------|----------------|
| transport | `main.rs` | `aibe::client`（`ensure_running`） |
| wire | `aibe_client.rs`, `agent_client.rs`, `presenter.rs`, `application/ask.rs` | `aibe::protocol::*` |
| wire 契約型 | `stdout_presenter.rs`, `domain/tools.rs`, `domain/ask.rs`, `toml_config.rs` | `aibe::ToolName`, `aibe::domain::{ExecutedToolCall, …}` |
| 契約定数 | `stdout_presenter.rs` | `aibe::ports::outbound::DEFAULT_MAX_TOOL_OUTPUT_BYTES` |
| wire 付加 | `application/ask.rs` | `aibe::ShellLogTail` |
| パス規約 | `toml_config.rs` | `aibe::default_socket_path` |
| テスト | `tool_names_sync.rs`, `tool_catalog_sync.rs` | `aibe::KNOWN_TOOLS` 等 |
| テスト | `shell_log_tail_max_bytes.rs` | `aibe::ShellLogTail::MAX_BYTES` |
| テスト | `ask_integration.rs` | **`aibe::application::server`**, `MockLlm`, `ProfileRegistry` 等（server 起動 E2E） |

`ai/Cargo.toml` は `aibe = { path = "../aibe" }` のみ。`scripts/check-architecture.sh` は 3 クレート前提で split crate 未認識。

**結論**: P1 完了時点で `ai` の path 依存は **`aibe-protocol` + `aibe-client` のみ**。`ai` → `aibe`（dev-dependencies 含む）は禁止。

## 目標依存関係

```text
aish          （変更なし。aibe 禁止）

aibe-protocol （leaf: serde のみ。domain / tokio / aibe 禁止）

aibe-client   → aibe-protocol のみ（Unix socket, ping, ensure_running, default_socket_path）

aibe          → aibe-protocol, aibe-client（server / LLM / agent / tools。wire 変換 TryFrom は aibe 内）

ai            → aibe-protocol, aibe-client のみ（aibe / aish 禁止）
```

## 型・定数の所在（移行後・確定）

| 名前 | 移行後の正本 | `aibe` | `ai` |
|------|--------------|--------|------|
| `ClientRequest`, `ClientResponse`, `ErrorCode`, `AgentTurnStatus`, `ProtocolMessage`, `ProtocolMessageOut`, `RequestContext` | `aibe-protocol` | 利用 | 利用 |
| `ToolName`, `KNOWN_TOOLS`, `READ_FILE`, `SHELL_EXEC`, `is_known_tool`, `UnknownToolError` | `aibe-protocol` | 利用（re-export しない） | 利用 |
| `ExecutedToolCall`, `ExecutedToolStatus` | `aibe-protocol` | 利用 | 利用（presenter 等） |
| `SHELL_LOG_TAIL_MAX_BYTES`（= 16 KiB） | `aibe-protocol` | `ShellLogTail::from_wire_opt` が参照 | `shell_log_tail_max_bytes` テストが参照 |
| `MAX_TOOL_OUTPUT_BYTES`（= 32 KiB） | `aibe-protocol` | `ToolsConfig` 既定が参照 | `stdout_presenter` が参照 |
| `ShellLogTail`（振る舞い型） | `aibe` domain | server のみ | **参照しない**（`ai` は定数のみ） |
| `AgentTurnContext`, `ChatMessage`, `ClientCwd`, `MessageRole` | `aibe` domain | server のみ | 参照しない |
| `TryFrom<RequestContext> for AgentTurnContext` | `aibe`（`application` または `adapters`） | 実装 | — |
| `TryFrom<ProtocolMessage> for ChatMessage` | `aibe` | 実装 | — |
| `default_socket_path` | `aibe-client` | 利用可 | `toml_config` が利用 |
| `ping`, `ensure_running` | `aibe-client` | `lib.rs` 起動チェックは `aibe-client` 経由 | `main` が利用 |

**依存の鉄則**: `aibe-protocol` は **`aibe` / `aish` / `ai` / `aibe-client` を一切参照しない**。`aibe-client` は **`aibe` を参照しない**。

## スコープ

### 対象

- `aibe-protocol` / `aibe-client` 新規クレート
- `aibe` の public API 再編成（`pub mod client` / `pub mod protocol` を非公開正本にしない）
- `ai` の依存切替・import 置換・テスト移設
- `docs/architecture.md`、`docs/testing.md` 更新
- `scripts/check-architecture.sh` の split crate ルール追加
- `scripts/check-hexagonal.sh` の対象範囲整理（leaf は対象外の明記）
- 実装完了後 `docs/0000_spec-index.md` の 0017 状態を **実装済み** に更新

### 対象外

- `aish` の変更
- LLM プロバイダ・tool 実装の変更
- wire JSON の形そのものの変更
- `aibe` の agent loop / server ロジックの再設計
- P2 / P3、0008 phase 2
- API キー・本番設定の扱い変更

## 確定した設計判断

| 項目 | 判断 | 理由 |
|------|------|------|
| **クレート分割** | 上記「目標依存関係」どおり | 逆流と secret 漏れを防ぐ |
| **wire JSON** | 破壊的変更禁止（`serde(tag)`、フィールド名、enum 値、NDJSON 1 行 1 JSON） | 既存 socket 契約維持 |
| **`aibe` の公開 API** | split 後、`client` / `protocol` をクライアント向け正本として公開しない | `ai` が `aibe` を経由しない最終状態 |
| **`aibe` の re-export** | `ToolName` 等を `aibe` クレート根から再 export **しない**（server 内部は `aibe_protocol::` / `use` で十分） | `ai` が誤って `aibe::ToolName` に戻るのを防ぐ |
| **PR 順** | `aibe-protocol` → `aibe` 内部差し替え → `aibe-client` → `ai` → docs → `check-architecture.sh` | wire 固定後に transport・クライアント |
| **hexagonal** | `aibe-protocol` / `aibe-client` は leaf。`check-hexagonal.sh` は `aibe` / `aish` / `ai` のみ | レイヤーを無理に増やさない |
| **セキュリティ** | secret・API キーは `aibe` 設定のみ。protocol/client に持たない | 優先順位 1 |
| **`ensure_running`** | 実装は `aibe-client`。`aibe` バイナリの「既に起動中」判定も `aibe_client::ping` を使う | spawn はクライアント責務。`aibe` の `run()` は server 起動に専念 |
| **`ask_integration` の server E2E** | `ask_reaches_mock_aibe` は **`aibe/tests/` へ移設**（`ai` に `aibe` dev-dep は置かない） | AC「`ai` は aibe のみ依存禁止」と両立 |

## 受け入れ条件

### クレート・依存

- workspace に `aibe-protocol` と `aibe-client` が追加されている。
- `ai/Cargo.toml` の path 依存は **`aibe-protocol` と `aibe-client` のみ**（`[dependencies]` / `[dev-dependencies]` とも `aibe` 禁止）。
- `aibe-protocol/Cargo.toml` に `aibe` / `aish` / `ai` / `aibe-client` の path 依存がない。
- `aibe-client/Cargo.toml` に `aibe` / `aish` / `ai` の path 依存がない（`aibe-protocol` のみ可）。

### wire・型

- `aibe-protocol` の serde roundtrip テストが、現行と同じ JSON 形で `ClientRequest` / `ClientResponse` / `ToolName` / `ErrorCode` を検証している。
- `SHELL_LOG_TAIL_MAX_BYTES` と `MAX_TOOL_OUTPUT_BYTES` が `aibe-protocol` にあり、`ai` / `aibe` がそれぞれ参照している（値は 16 KiB / 32 KiB のまま）。

### `ai` から消える参照

- `ai/src/**` および `ai/tests/**` に `use aibe::` / `aibe::` が **1 件もない**。
- 代替 import 例: `aibe_protocol::…`, `aibe_client::…`（クレート名は実装時の snake_case に合わせる）。

### `aibe`・テスト

- `aibe/src/**` に `aibe/src/client/` モジュール実装が残っていない（`aibe-client` へ移設）。
- `aibe-client/tests/client_ping.rs`、`ensure_running_*` が存在し、`aibe/tests/` には残らない。
- `aibe/tests/socket_protocol.rs` は **`aibe` に残る**（MockLlm + `server::run` の server 統合。serde 単体は `aibe-protocol` 側へ抽出）。
- `ai/tests/ask_integration.rs` の `ask_reaches_mock_aibe` 相当は `aibe/tests/` にあり、`ai` 側は Mock クライアントのみのテストに整理されている。

### 機械検査・docs

- `./scripts/check-architecture.sh` が下記「追加する検査」を満たす実装になっている。
- `./scripts/check-hexagonal.sh` が `aibe` / `aish` / `ai` で引き続き成功する。
- `docs/architecture.md`、`docs/testing.md`、`docs/0000_spec-index.md`（実装済み）が一致している。

## `scripts/check-architecture.sh` に追加する検査（実装タスクに含める）

| 対象 | 禁止依存（Cargo.toml の path / 名前） |
|------|----------------------------------------|
| `aibe-protocol` | `aibe`, `aibe-client`, `aish`, `ai` |
| `aibe-client` | `aibe`, `aish`, `ai` |
| `ai` | `aibe`, `aish`（既存どおり） |
| `aish` | `aibe`（既存どおり） |

追加で推奨（実装可能なら）:

- `ai/src` / `ai/tests` に `\baibe::` または `use aibe` が無いことを `rg` で検査（path 依存削除の漏れ防止）。

## 実装タスク分解

1. `aibe-protocol` 新設: wire DTO、`ToolName`、`ExecutedToolCall`、`KNOWN_TOOLS`、契約定数、serde 単体テスト（`protocol/request.rs` の既存 `#[cfg(test)]` を移す）。
2. `aibe` の `protocol` モジュールを削除し、`aibe-protocol` 参照 + **`TryFrom` 変換**（`RequestContext` → `AgentTurnContext` 等）を `aibe` 内に残す。
3. `aibe-client` 新設: `ping` / `ensure_running` / `default_socket_path`、クライアント統合テスト移設。
4. `aibe` の `lib.rs` から `pub mod client` / `pub mod protocol` と domain 契約の再 export を削除。`run()` は `aibe_client::ping` を使用。
5. `ai` の `Cargo.toml` と全 import を `aibe-protocol` / `aibe-client` に切替。
6. `ai/tests/ask_integration.rs` から server 起動テストを `aibe/tests/` へ移す。残りは `aibe_protocol` のみで完結させる。
7. `docs/architecture.md`、`docs/testing.md` 更新。
8. `check-architecture.sh` に上表の検査を追加。

## docs 更新一覧

| ファイル | 内容 |
|----------|------|
| `docs/architecture.md` | 依存図・protocol 節・クレート表を split 後に更新 |
| `docs/testing.md` | **必須**: クレート別テストの所在（protocol 単体 / client 単体 / server 統合 / ai 単体） |
| `docs/0000_spec-index.md` | 完了時 **実装済み** |
| `docs/manual/` | 手動確認が増えた場合のみ |

## テスト方針

| 種別 | 所在 | 具体例 |
|------|------|--------|
| **protocol 単体** | `aibe-protocol/tests/` または crate 内 `#[cfg(test)]` | `ClientRequest` / `ClientResponse` / `ToolName` / `ErrorCode` の serde roundtrip。`RequestContext` の JSON 形 |
| **client 単体** | `aibe-client/tests/` | `client_ping.rs`、`ensure_running_env.rs`、`ensure_running_spawn.rs`（`tests/common/mod.rs` で mock `aibe` バイナリ起動） |
| **server 統合** | `aibe/tests/` に残す | `socket_protocol.rs`（**server + socket E2E**。protocol 単体とは別ファイル）、`agent_turn_*`、`request_tool_validation.rs`、`llm_profiles_socket.rs` |
| **ai 単体** | `ai/tests/` | `resolve_tools`、`RecordingClient`、`plan_ask_launch` 等（**`aibe` 起動なし**） |
| **ai↔aibe E2E** | `aibe/tests/` へ移設 | 旧 `ask_reaches_mock_aibe` |

**禁止**: `socket_protocol.rs` を `aibe-protocol` と `aibe` の両方に「同内容」で置くこと。server 統合は `aibe` のみ、serde 単体は `aibe-protocol` のみ。

## ブランチ名

- `feature/aibe-protocol-split`

## 未確定・残リスク

- `ensure_running` の `AIBE_BIN` 解決と `aibe` バイナリ配置は現状どおり。split 後もドキュメントに 1 行追記する。
- import 置換漏れは `check-architecture.sh` の `rg` 検査と `cargo test --workspace` で潰す。
- クレート追加に伴い CI のビルド時間がわずかに増える。

## 関連

- [0008](done/0008_chat-message-and-protocol-typing-spec.md) phase 2 とは別件。
- [concerns.md](todo/chatgpt-review-4th-gen/concerns.md) §3 の P1 前倒し。
- P2 / P3 とは切り離す。
