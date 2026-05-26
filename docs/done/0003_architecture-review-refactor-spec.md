# 0003 — アーキテクチャレビュー反映 — 指示書

> **出典**: Codex `review`（2026-05-24）。Cursor 親が `feat/architecture-review-refactor` で実装。  
> **状態**: 実装済み（2026-05-24）。正本の要約は `../architecture.md`（ツール cwd・プロトコル型）。本ファイルはレビュー指摘への対応方針と受け入れ条件を定義する。

## 確定した設計判断（ユーザー / レビュー反映）

| 項目 | 方針 |
|------|------|
| **`context.cwd` とツール** | `tools` が空でない `agent_turn` では **`context.cwd`（絶対パス）必須**。未送信・相対パスは turn `error`（`invalid_request`）。aibe プロセス cwd への **フォールバック禁止** |
| **ツール名の正本** | `aibe::domain::tool_name`（`READ_FILE` / `SHELL_EXEC` / `KNOWN_TOOLS`）。`ai` は `aibe` 公開名を参照し、ローカル定数の二重管理をやめる |
| **プロトコル JSON** | NDJSON 上のフィールド形は **0001 / 0002 と互換**（`status` は `"ok"` / `"max_tool_rounds"` 等の snake_case 文字列）。内部型のみ enum / struct で強化 |
| **レイヤー分離（ai）** | allowlist 解決は **domain**、起動時 `stderr` 表示は **Presenter（adapter）** |
| **送信 payload** | CLI 収集（`AskInput`）と aibe 送信（`AskRequest`）を分離。ツール有効時 cwd 検証は送信直前 |
| **max-round 終端** | 要約生成は **domain**（`ToolExecutionSummary`）、終端ユースケースは **application**（`tool_round_terminator`） |
| **`ToolName` 全面置換** | **見送り**（MVP）。型定義と定数集約のみ。プロトocol / allowlist は引き続き `String` |

## 目的

Codex アーキテクチャレビュー（ヘキサゴナル適合・ドメインオブジェクト化・拡張性・境界分割）の指摘を、**本番経路**に反映する。

- 依存方向と cwd 方針の **隠れ依存** を除去する
- プリミティブ偏重の **热点**（status、tool 記録、allowlist、送信 context）を型で固定する
- 将来のツール / クライアント追加に耐える **モジュール分割** を入れる
- `./scripts/check-architecture.sh`（内包: `check-hexagonal.sh`）を維持する

## スコープ

### 対象

- **aibe**: `ClientCwd`, `ToolExecutionContext`, `ToolExecutionSummary`, `tool_round_terminator`, プロトコル内部型
- **ai**: `ToolAllowlist`, `AskInput` / `AskRequest`, Presenter 拡張、ツール名参照の一本化
- **docs**: `architecture.md`, `security.md`, `.cursor/rules/30-architecture.mdc`
- **テスト**: 上記の単体・統合・回帰

### 対象外

- `ToolName` の API 全体への置換（`Vec<ToolName>` 等）
- 非 `ai` クライアントの新規実装
- `max_tool_rounds` 時の LLM プロンプト戦略の再設計（構造化要約まで。プロバイダ別最適化は将来）
- `aish` クレート変更
- プロトocol の breaking change（フィールド名・JSON 形の変更）

## 受け入れ条件

### 1. ヘキサゴナル静的検査

- `cargo test --workspace` 成功
- `cargo clippy --workspace -- -D warnings` 成功
- `./scripts/check-architecture.sh` 成功（`application → adapters` 逆依存なし）

### 2. cwd 方針（aibe）

- `ToolExecutionContext::base_dir` は **`ClientCwd` のみ** を参照する。`std::env::current_dir()` を **使わない**
- `tools: []` のときは cwd 未送信を許容する
- `tools` 非空かつ `context.cwd` 未送信 / 相対パス → `ClientResponse::Error { code: invalid_request, ... }`
- **検証順序**: `tools` 非空のときは **cwd を tool 名検証より先** に行う（未送信 cwd + 未知名が同時でも `invalid_request`）
- `read_file` / `shell_exec` の相対パス・`.` 付き `allowed_roots` は `context.cwd` 基準のまま動作する

### 3. ドメイン型（aibe）

| 型 | 配置 | 責務 |
|----|------|------|
| `ClientCwd` | `domain/client_cwd.rs` | 絶対パス必須。`RequestContext::require_client_cwd()` から構築 |
| `ToolExecutionSummary` | `domain/tool_execution_summary.rs` | 実行済み tool 記録のプレーンテキスト要約 |
| `ExecutedToolStatus` | `domain/tool.rs` | `ok` / `error`（serde snake_case） |
| `AgentTurnStatus` | `protocol/response.rs` | `ok` / `max_tool_rounds` |
| `ToolName` | `domain/tool_name.rs` | 定数・`is_known_tool` の正本（API 置換は未実施） |

### 4. モジュール分割（aibe）

- `application/agent_turn.rs` — ループ制御
- `application/tool_round_terminator.rs` — max-round 到達時の最終 `complete()` と `AgentTurnStatus::MaxToolRounds`
- `ExecutedToolCall`（監査・レスポンス）と LLM 向け `ToolResult` の **二重持ちは維持**（レビュー推奨どおり冗長性を意図的に残す）

### 5. ai クライアント

| 型 / モジュール | 配置 | 責務 |
|-----------------|------|------|
| `ToolAllowlist` | `domain/tools.rs` | 展開済みツール名集合 |
| `ResolvedTools` | 同上 | `allowlist` + 起動時メタ（`ToolsStartupLine`） |
| `AskInput` | `domain/ask.rs` | CLI / ユースケース入力 |
| `AskRequest` | 同上 | `AgentClient` 経由の送信 payload |
| `Presenter::show_tools_startup` | `ports` + `stdout_presenter` | 0002 の起動時 1 行（domain に `eprintln` しない） |

- ツール名は `aibe::{READ_FILE, SHELL_EXEC, is_known_tool, KNOWN_TOOLS}` を参照
- `ai/tests/tool_names_sync.rs` は **aibe 公開名を ai が受け付ける** ことのみ検証（定数の二重 assert は削除）

### 6. プロトocol 互換

- **wire JSON** は 0001 / 0002 と同一視できること（既存クライアント・テストの JSON 文字列検査を維持）
- 内部では `AgentTurnResult.tool_calls: Vec<ExecutedToolCall>`（serde 出力は従来どおり object 配列）
- `AgentTurnResult.status` は enum だが JSON では `"ok"` / `"max_tool_rounds"` 文字列

### 7. 表示契約（0002 維持）

- `stdout` / `stderr` / `--verbose-tools` / `max_tool_rounds` warning の契約は **0002 を変更しない**
- Presenter は `ExecutedToolCall` 型で verbose 行を組み立てる（`serde_json::Value` 直読みをやめる）

## 実装マップ（正本パス）

### aibe

```
aibe/src/domain/client_cwd.rs          # ClientCwd, ClientCwdError
aibe/src/domain/tool_name.rs           # READ_FILE, SHELL_EXEC, KNOWN_TOOLS, ToolName
aibe/src/domain/tool_execution_summary.rs
aibe/src/domain/tool.rs                # ExecutedToolStatus
aibe/src/ports/outbound/tool_context.rs # ToolExecutionContext（ClientCwd のみ）
aibe/src/protocol/request.rs           # RequestContext::require_client_cwd
aibe/src/protocol/response.rs          # AgentTurnStatus, tool_calls: Vec<ExecutedToolCall>
aibe/src/application/agent_turn.rs
aibe/src/application/tool_round_terminator.rs
```

### ai

```
ai/src/domain/tools.rs       # ToolAllowlist, resolve_tools（aibe 名参照）
ai/src/domain/ask.rs         # AskInput, AskRequest, AskRequestError
ai/src/application/ask.rs    # into_request → AgentClient
ai/src/adapters/outbound/stdout_presenter.rs  # show_tools_startup, format_tool_call_line(ExecutedToolCall)
ai/src/ports/outbound/presenter.rs
ai/src/ports/outbound/agent_client.rs         # AskRequest
```

## テスト

| 種別 | 内容 |
|------|------|
| 単体 | `ClientCwd` パース、`require_client_cwd`、cwd 未送信 + tools → `invalid_request` |
| 単体 | `ToolExecutionSummary`、AskInput → AskRequest（tools 有無） |
| 単体 | Presenter / startup 行フォーマット |
| 統合 | `agent_turn_loop`: cwd 必須、max_tool_rounds、`ExecutedToolStatus` |
| 統合 | `agent_turn_tools`: socket リクエストに `context.cwd` |
| 統合 | `ask_integration`: allowlist、`AgentTurnStatus` presenter 契約 |
| 回帰 | `tools: []` 単発 LLM、既存 ping / socket protocol |

手動: `../manual/ai-ask-tools.md`（cwd 節は architecture 正本に従う）。

## 影響クレート

| クレート | 変更 |
|---------|------|
| **aibe** | domain / application / protocol / ports / テスト |
| **ai** | domain / application / adapters / ports / テスト |
| **aish** | 変更なし |
| **docs** | 本ファイル、`architecture.md`, `security.md`, `AGENTS.md`, `.cursor/rules/30-architecture.mdc` |

## 0001 / 0002 との関係

| ドキュメント | 関係 |
|-------------|------|
| **0001** | エージェントループ・tool result 方針は不変。0003 は cwd 必須化と内部型強化を **上書き補足** |
| **0002** | allowlist・表示契約は不変。0003 は domain/presentation 分離と `AskRequest` を **追加** |
| **architecture.md** | 運用上の正本。0003 実装後の cwd 表を優先 |

**0001 記載の訂正（0003 以降）**

- ~~「`context.cwd` 未送信時のみ aibe cwd にフォールバック」~~ → **禁止**
- 内部 `tool_calls` は `Vec<serde_json::Value>` ではなく `Vec<ExecutedToolCall>`（**wire JSON は同等**）

## 意図的に見送ったもの（フォロー指示書）

| 項目 | 理由 | 指示書 |
|------|------|--------|
| `ToolName` の全 API 置換 | JSON / config との境界で過剰。定数正本化で同期リスクは低減済み | [0004](0004_tool-name-type-adoption-spec.md) |
| `RequestContext` のドメイン化 | 0003 は `ClientCwd` のみ。tail 上限・拡張フィールドは未着手 | [0005](0005_request-context-domain-spec.md) |
| `max_tool_rounds` の会話履歴そのまま再送 | 0001 採用の要約経路を維持。構造化は `ToolExecutionSummary` まで | [0006](0006_max-tool-rounds-terminator-spec.md) |
| agent_turn ループ本体の分割 | 0003 は terminator のみ分離 | [0007](0007_agent-turn-loop-modularization-spec.md) |
| `ChatMessage.role` の enum 化 | 0003 は status / tool_calls のみ | [0008](0008_chat-message-and-protocol-typing-spec.md) |
| カテゴリ表の機械同期 | 0003 は aibe 定数正本 + 受け入れテストのみ | [0009](0009_ai-tool-category-sync-spec.md) |
| cwd 必須の「ツール実行時のみ」緩和 | クライアント実装のばらつきを招く。リクエスト時点で統一 | 0005 / architecture で再議論可 |
| 0001 / 0002 ドラフト本文の全面改稿 | 0003 が差分の指示書。詳細は本ファイル + architecture | — |

## 未確定・推測

| 種別 | 内容 |
|------|------|
| **推測** | 将来の非 `ai` クライアントは `context.cwd` 必須を守ればよい。緩和ポリシーは未議論 |
| **推測** | `ToolName` 全面適用はツール数増加時に再検討 |
| **未確定** | 実 LLM 各プロバイダでの `max_tool_rounds` 要約品質 |

## 残リスク

- 手動検証（`../manual/ai-ask-tools.md`）は指示書作成時点で未再実施
- 静的 hexagonal チェックは依存方向のみ。ランタイムのプロバイダ差は別途
- `ToolName` 未適用のため、将来ツール追加時は `domain/tool_name.rs` と `ai` カテゴリ表の同期が必要

## 完了確認コマンド

```bash
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
./scripts/check-architecture.sh
```
