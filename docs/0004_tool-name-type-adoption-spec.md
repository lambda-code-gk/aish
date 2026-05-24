# 0004 — ToolName 型の API 全面適用 — 指示書

> **出典**: Codex `review`（2026-05-24）ドメインオブジェクト化候補（優先度: 高）。0003 で型定義のみ実装済み。  
> **レビュー**: Codex `review`（2026-05-24）— **要修正** → 本版で指摘反映済み。  
> **状態**: **実装済み**（2026-05-24、`feature/tool-name-type-adoption`）。正本 API は `aibe::domain::tool_name::ToolName`。

## 目的

組み込みツール名を `String` ではなく **`ToolName` 値オブジェクト** で API 境界まで運び、typo・未知名・`ai` / `aibe` 同期ズレを **コンパイル時またはパース時** に検出する。

0003 で定数正本化（`READ_FILE` / `SHELL_EXEC` / `KNOWN_TOOLS`）は済んでいる。本指示書は **型の全面採用** を定義する。

0003 の wire 互換方針（NDJSON 上は従来どおり文字列、**内部型のみ強化**）を維持したまま、`String` 境界を減らす。

## スコープ

### 対象

#### aibe — domain / application

| ファイル | 変更内容 |
|----------|----------|
| `aibe/src/domain/tool_name.rs` | `Serialize` / `Deserialize` 追加。`FromStr` を正 API として明文化 |
| `aibe/src/domain/tool.rs` | `ToolCall.name` / `ExecutedToolCall.name` → `ToolName`。コンストラクタ引数も同型 |
| `aibe/src/application/agent_turn.rs` | `tools` 引数・検証を `ToolName` 化。入口で一括 `FromStr` |
| `aibe/src/application/tool_defs.rs` | `definitions_for(allowed: &[ToolName])`、`is_known_tool_name` の引数・戻り値 |

#### aibe — ports / adapters

| ファイル | 変更内容 |
|----------|----------|
| `aibe/src/ports/outbound/tool_registry.rs` | `get(&self, name: &ToolName)` |
| `aibe/src/ports/outbound/tools.rs` | `ToolDefinition.name` → `ToolName`。`ToolExecutor::name()` → `ToolName`（または `&ToolName`） |
| `aibe/src/adapters/outbound/tools/registry.rs` | `HashMap<ToolName, _>`（または `ToolName` 比較） |
| `aibe/src/adapters/outbound/tools/read_file.rs` | `ExecutedToolCall::*` 呼び出し、`ToolExecutor::name()` |
| `aibe/src/adapters/outbound/tools/shell_exec.rs` | 同上 |
| `aibe/src/adapters/outbound/openai_compatible.rs` | `parse_tool_calls()` — LLM 返却名を `ToolName::from_str` で正規化。未知名は turn エラーまたは tool 実行前に拒否 |
| `aibe/src/adapters/outbound/mock_llm.rs` | `ToolCall` / `ToolDefinition` 構築箇所 |
| `aibe/src/adapters/outbound/scripted_mock_llm.rs` | 同上 |

#### aibe — protocol

| ファイル | 変更内容 |
|----------|----------|
| `aibe/src/protocol/request.rs` | `AgentTurn.tools` は wire 上 `Vec<String>` のまま。**デシリアライズ直後** に `Vec<ToolName>` へ変換・検証 |
| `aibe/src/protocol/response.rs` | `ExecutedToolCall` の serde 出力が従来どおり snake_case 文字列であること（型変更のみ、wire 不変） |

#### ai — domain / application / adapter

| ファイル | 変更内容 |
|----------|----------|
| `ai/src/domain/tools.rs` | `ToolAllowlist { names: Vec<ToolName> }`。`resolve_tools` 展開結果を `ToolName` 集合に正規化 |
| `ai/src/domain/ask.rs` | `AskInput.tools` / `AskRequest.tools` → `Vec<ToolName>`（内部表現）。`into_request()` は型を維持 |
| `ai/src/application/ask.rs` | 上記 DTO 変更に追従 |
| `ai/src/adapters/outbound/aibe_client.rs` | 送信境界: `AskRequest.tools`（`Vec<ToolName>`）→ `ClientRequest::AgentTurn.tools`（`Vec<String>`）へ `as_str()` で変換 |

#### テスト（型変更に追従）

- `aibe/tests/agent_turn_tools.rs`、`aibe/tests/agent_turn_loop.rs`（存在する場合）
- `ai/tests/ask_integration.rs`
- `ai/tests/tool_names_sync.rs`
- 各 crate の mock / 単体テストで `ToolCall { name: ... }` を構築している箇所

### 対象外

- NDJSON の JSON 形変更（クライアントは引き続き `"read_file"` 等の文字列を送る）
- `ai` カテゴリエイリアス（`@read-only` 等）の **定義・表の変更** → **0009**（0004 では展開結果を `ToolName` に正規化するのみ）
- 動的ツールディスカバリ / `list_tools`
- `ClientRequest` / `ClientResponse` の serde タグ・フィールド名変更

## 0009 との境界

| 0004 | 0009 |
|------|------|
| ツール名の **型化**（`ToolName` 値オブジェクト） | カテゴリ表（`@read-only` 等）と `KNOWN_TOOLS` の **機械同期** |
| `resolve_tools` の展開**後**を `ToolName` 集合にする | カテゴリ定義そのものは `ai` 専有（0002 方針） |

0004 実装時は `resolve_tools` に触れるが、**カテゴリ定数・0002 のカテゴリ表は変更しない**。0009 完了後も `ToolAllowlist` 内部は `Vec<ToolName>` のまま。

## 設計判断（実装前に確定すること）

| 項目 | 推奨案 | 代替案 |
|------|--------|--------|
| 未知名の扱い | `ToolName::from_str(s)` → `Result<_, UnknownToolError>`。**agent_turn 入口**（protocol 変換後）と **ai allowlist 解決**で一括検証。エラーは `invalid_request` / `ToolsResolveError::UnknownTool` | serde カスタム deserializer（wire 失敗メッセージが UX を損ねやすい） |
| `ToolRegistry` キー | `HashMap<ToolName, Arc<dyn ToolExecutor>>`。`ToolExecutor::name()` も `ToolName` を返す | 内部のみ `&'static str`、port 境界で `ToolName` に変換 |
| `ToolDefinition.name` | `ToolName`（LLM API 送信時に `as_str()` で文字列化） | 送信 adapter のみ `String` |
| `ai` allowlist | 内部 `Vec<ToolName>`。**送信境界**（`aibe_client`）でのみ `Vec<String>` に落とす | `AskRequest` まで `String` を維持（型化の効果が薄れる） |
| `AskRequest.tools` | `Vec<ToolName>`（domain 内部表現） | `ToolAllowlist` 型でラップ（0004 では Vec で十分、将来検討可） |
| エラー型 | `UnknownToolError` を domain で定義し、protocol / ai はラップまたは変換 | クレートごとに独立した未知名エラー |
| LLM 未知名 tool call | `parse_tool_calls` で `from_str` 失敗 → `LlmError::UnknownTool` → turn `error`（`tool_not_allowed`） | 実行時に `ToolRegistry::get` が `None` でエラー（遅い） |

### `ToolName` API（確定仕様）

```rust
// aibe/src/domain/tool_name.rs — 0004 完了時点の公開 API
impl ToolName {
    pub fn as_str(&self) -> &str;
    pub fn read_file() -> Self;
    pub fn shell_exec() -> Self;
}
impl FromStr for ToolName { type Err = UnknownToolError; }
impl Display for ToolName { /* snake_case 文字列 */ }
impl Serialize for ToolName { /* JSON string として出力 */ }
impl Deserialize for ToolName { /* JSON string から。未知名は Deserialize エラー */ }
```

- エイリアス `ToolName::parse(s)` は **設けない**。`FromStr` / `str::parse::<ToolName>()` を正とする。
- `ToolCall` / `ExecutedToolCall` に `Deserialize` があるため、`ToolName` の serde は **JSON 文字列**（例: `"read_file"`）として wire と一致させる。

## 受け入れ条件

### 1. 検証

- `KNOWN_TOOLS` 以外の文字列は **`ToolName` 構築前** に拒否される:
  - aibe: `agent_turn` リクエスト（protocol → domain 変換直後）
  - ai: `resolve_tools` / allowlist 解決
- LLM プロバイダ（OpenAI 互換）の tool call 名が未知名の場合、実行前にエラーになる。

### 2. 型

- `ToolCall` / `ExecutedToolCall` / `ToolDefinition` の `name` が `ToolName`。
- `ToolRegistry::get` / `ToolExecutor::name()` が `ToolName` 基準。
- `ai::ToolAllowlist` 内部が `Vec<ToolName>`。`AskInput` / `AskRequest` の `tools` も `Vec<ToolName>`。

### 3. wire 互換（0003 方針維持）

- NDJSON 上の `tools` は引き続き `["read_file", ...]` の **文字列配列**。
- レスポンスの `tool_calls[].name` も **snake_case 文字列**（`ToolName` の serde 出力）。
- 既存統合テストの **JSON 文字列リテラル・アサーションは変更しない**（型変更に伴うコンパイル修正のみ可）。

### 4. 品質

- `cargo fmt --all -- --check`
- `cargo clippy --workspace -- -D warnings`
- `cargo test --workspace`
- `./scripts/check-architecture.sh` 通過（`ai → aibe` 依存方向維持。`ai` は `aibe::ToolName` 等の公開 API を参照してよい）

### 5. 変換経路

- `AskInput` → `AskRequest` → `ClientRequest::AgentTurn.tools`: 内部は `ToolName`、`aibe_client` 送信時のみ `String`。
- protocol 受信: `Vec<String>` デシリアライズ → `ToolName::from_str` 一括 → domain 処理。

## 実装方針

推奨作業順:

1. **`tool_name.rs`** — `Serialize` / `Deserialize`、単体テスト
2. **`tool.rs`** — `ToolCall` / `ExecutedToolCall` の `name` 型変更 + serde roundtrip テスト
3. **ports** — `ToolDefinition`、`ToolRegistry`、`ToolExecutor::name()`
        4. **adapters** — `registry.rs`、各 `ToolExecutor`、`openai_compatible.rs`
5. **protocol** — `request.rs` 入口検証（wire `Vec<String>` 維持）
6. **application** — `agent_turn.rs`、`tool_defs.rs`
7. **ai** — `tools.rs` → `ask.rs` → `aibe_client.rs`（送信変換）
8. **テスト一括修正** — mock / 統合

主要ファイル:

```
aibe/src/domain/tool_name.rs
aibe/src/domain/tool.rs
aibe/src/application/agent_turn.rs
aibe/src/application/tool_defs.rs
aibe/src/ports/outbound/tool_registry.rs
aibe/src/ports/outbound/tools.rs
aibe/src/adapters/outbound/tools/registry.rs
aibe/src/adapters/outbound/openai_compatible.rs
aibe/src/protocol/request.rs
ai/src/domain/tools.rs
ai/src/domain/ask.rs
ai/src/adapters/outbound/aibe_client.rs
```

## テスト

| 種別 | 内容 |
|------|------|
| 単体 | `ToolName::from_str` 成功 / 失敗（`UnknownToolError`） |
| 単体 | `ToolName` の serde roundtrip（JSON 文字列 `"read_file"` ↔ `ToolName`） |
| 単体 | `ToolCall` / `ExecutedToolCall` の serde roundtrip（`name` が JSON 文字列のまま） |
| 単体 | `ai`: `resolve_tools` 展開結果が `ToolName` 集合であること |
| 単体 | `ai`: `AskInput::into_request` が `Vec<ToolName>` を維持すること |
| 統合 | `openai_compatible`: LLM tool call パース → `ToolName` 化 |
| 統合 | `aibe/tests/agent_turn_*.rs`: NDJSON wire 互換（JSON リテラル不変） |
| 統合 | `ai/tests/ask_integration.rs`: allowlist 送信が従来 JSON 形 |
| 回帰 | 0001 / 0002 / 0003 の既存 agent_turn・ask 統合 |

## 0003 との関係

0003 で **見送り** と明記（型定義と定数集約のみ）。0004 完了後は `domain/tool_name.rs` の `ToolName` が正本 API となる。

0003 の確定判断との対応:

| 0003 判断 | 0004 での扱い |
|-----------|---------------|
| wire JSON は文字列のまま | 維持。`ToolName` は serde で JSON string |
| `ToolName` 全面置換は見送り | 0004 で実施 |
| ツール名正本は `aibe::domain::tool_name` | 維持。`ai` は `aibe` 公開名 / `ToolName` を参照 |
| cwd 検証は tool 名より先 | 維持（0004 は検証順序を変えない） |

## 優先度・実施タイミング

現状は組み込みツール **2 件**（`read_file` / `shell_exec`）のため、0004 単体の ROI は限定的。

**実施を推奨するトリガー**（いずれか）:

- 組み込みツールが **3 件以上** になる見込み
- **aibe 以外のクライアント**（`ai` 以外）が protocol で `agent_turn` を送る
- ツール名 typo による **本番バグ** または review での指摘
- **0009** 着手前（allowlist 型化でカテゴリ展開後の型が固定される）

トリガーが無い間は 0005 以降と並行可能だが、新ツール追加 PR では 0004 先行を推奨。

## 未確定・残リスク

- serde `Deserialize` 失敗時のメッセージを `invalid_request` に寄せるか、serde エラーをそのまま返すか — 実装時に UX を確認（推奨: agent_turn 入口で明示メッセージ）。
- `ToolExecutor::name()` を `ToolName` にすると各 executor 実装の boilerplate が増える — `const` 正本（`READ_FILE` 等）から生成する helper で軽減可。
- 0004 と **0008**（`ChatMessage` 型化）を同一 PR に含めると diff が膨らむ — 別 PR 推奨。
