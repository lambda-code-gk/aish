# 0010 — Gemini プロバイダ指示書 — 仕様ドラフト

> **出典**: 本タスク（2026-05-24）— Gemini プロバイダ実装の前段仕様化。Google 公式 Gemini API / function calling 仕様を参照。レビュー反映（2026-05-24、Codex `review` 再レビュー反映）。  
> **状態**: **実装済み**

## 目的

aibe に **Gemini 専用の LLM アダプタ** を追加し、Google AI Studio の Gemini API だけでツール付き会話を処理できるようにする。OpenAI 互換アダプタの流用はしない。

この指示書は、実装前に以下を固定する。

- Google AI Studio (`generativelanguage.googleapis.com`) の **`generateContent` REST v1beta のみ** を使う
- `GeminiLlm` という **専用 adapter** を導入する
- `MessageRole` / `ChatMessage` / **クライアント wire** は **0008 のまま**（`ToolCall.provider_extras` は aibe 内部のみ）
- `TerminationCapability` は安全側の初期値から始める

## スコープ

### 対象

- Gemini 専用 `LlmProvider` 実装
- `llm_factory` / `toml_config` / `AppConfig` への組み込み
- `functionDeclarations` / `functionCall` / `functionResponse` の変換
- `systemInstruction` への system 集約
- `ToolCall.id` と Gemini `functionCall.id` の対応
- **`ToolCall.provider_extras`**（`thought_signature` 等のマルチターン保持）
- `wiremock` を使った HTTP テスト
- 手動検証手順

### 対象外

- Vertex AI
- streaming
- thinking / reasoning の有効化や最適化
- OpenAI 互換ゲートウェイ経由の Gemini 利用
- Gemini の組み込みツール（Google Search など）
- **クライアント protocol** の変更（`provider_extras` は wire に載せない）
- `ai` / `aish` 側での LLM 直呼び

## 確定した設計判断

| 項目 | 方針 |
|------|------|
| **API** | Google AI Studio の `generativelanguage.googleapis.com` を使う。`POST /v1beta/models/{model}:generateContent` のみを実装対象にする。 |
| **provider 名** | 設定値は **`gemini` のみ**（`google` 等のエイリアスは設けない）。 |
| **adapter** | 専用 `GeminiLlm` を追加する。`openai_compatible` の実装を流用しない。 |
| **設定** | `[llm] provider = "gemini"`、`api_key`、`model`、任意 `base_url`。`base_url` 省略時は `https://generativelanguage.googleapis.com/v1beta` を既定にする。 |
| **環境変数** | `openai_compatible` と共用する: `AIBE_LLM_PROVIDER`, `AIBE_API_KEY`, `AIBE_MODEL`, `AIBE_BASE_URL`。Gemini 利用時は `AIBE_LLM_PROVIDER=gemini` を使う。 |
| **既定モデル** | `gemini-3.5-flash`。設定省略時の例、`docs/` の例示、本文中の例をこれに揃える。 |
| **system** | `MessageRole::System` は `systemInstruction` に集約する。複数 system メッセージは **出現順で `\n\n` 連結**して 1 本にする。 |
| **tool 結果** | 連続する `role: tool` は同一 user ターンにまとめ、`functionResponse` parts として 1 本の `Content` にする。 |
| **functionCall.id** | `ToolCall.id` と 1 対 1 で対応させる。Gemini 応答の `functionCall.id` が欠落または空なら **synthetic id**（後述）を生成し、その turn 内で一貫して再利用する。 |
| **provider メタデータ** | Gemini の `thoughtSignature` 等は **part 単位**で `ToolCall.provider_extras` に保持し、次ラウンドで **同一 part 位置**の `functionCall` に復元する（0008 wire には載せない）。call 全体のメタデータとして集約しない。 |
| **termination** | `TerminationCapability.plain_complete_accepts_tool_role` の初期値は **`false`**。0006 の安全側から始める。 |
| **境界** | LLM HTTP クライアントは **aibe のみ** に置く。`ai` / `aish` は Gemini API を直接呼ばない。 |
| **安全性** | API キーは aibe 設定に限定し、ログとリポジトリに残さない。`../security.md` の `AIza` マスク方針に従う。 |

## `ToolCall.provider_extras`（0010 で domain 拡張）

Gemini 3 系の function calling では、マルチターン時に **`thought_signature` を前ターンの `functionCall` part とともに再送**する必要がある（Google 公式。実 API では実測で確認）。現行 `ToolCall` は `id` / `name` / `arguments` のみでは不足する。

### 追加フィールド

```rust
// aibe/src/domain/tool.rs — 0010 で追加
pub struct ToolCall {
    pub id: String,
    pub name: ToolName,
    pub arguments: Value,
    /// Gemini 等の provider 固有 part。wire / protocol には載せない。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_extras: Option<Value>,
}
```

### 格納内容と再送ルール（Gemini）

Google 公式 function calling では、`thoughtSignature` は **受け取った part と同じ part でそのまま返す**必要がある。複数 `functionCall` part がある場合、**署名が付くのは先頭 part のみ**など part ごとに差があり、**part をマージすると壊れる**。

`provider_extras` は **call 全体のメタデータではなく、元の Gemini part 位置を再現するための保存領域** とする。

| キー | 値 | 備考 |
|------|-----|------|
| `gemini_part_index` | 整数 | 同一 `model` ターン内での part 順序（0 始まり）。再送時に part 順を復元する。 |
| `thoughtSignature` | API が返した値を **そのまま** `Value` で保持 | 受信 part にのみ付与。無い part には載せない。 |

```json
{
  "gemini_part_index": 0,
  "thoughtSignature": "<API が返した値>"
}
```

**禁止**:

- 複数 `functionCall` の `provider_extras` を 1 つの `ToolCall` に **マージ**しない（1 `ToolCall` ↔ 1 `functionCall` part）
- 署名のある part と無い part を **1 part に合成**しない
- `thoughtSignature` を別 part や call 全体のメタとして **付け替え**ない

**再送**:

- aibe → Gemini で `Assistant` + tool calls を `model` part に戻すとき、各 `ToolCall` の `provider_extras` から **対応 part のみ** `thoughtSignature` 等を復元する
- part 順は `gemini_part_index` と `ChatMessage.tool_calls` の並びで再現する

- adapter が `functionCall` part を domain に載せるとき、**その part 由来**の `provider_extras` を設定する
- **0008 の wire / `ProtocolMessage` は変更しない**（`provider_extras` は serde で省略され、クライアントからは見えない）

## メッセージ変換表

### aibe → Gemini

| aibe 側 | Gemini 側 | 補足 |
|---------|-----------|------|
| `MessageRole::System` | `systemInstruction` | 1 つに集約（`\n\n` 連結）。`contents` には載せない。 |
| `MessageRole::User` | `contents[].role = "user"` + `parts[].text` | 一般テキストメッセージ。 |
| `MessageRole::Assistant` | `contents[].role = "model"` + `parts[].text` | 通常の応答。本文が空のときは text part を省略してよい。 |
| `MessageRole::Assistant` + tool call | `contents[].role = "model"` + `parts[].functionCall` | 1 `ToolCall` ↔ 1 part。`provider_extras` から **その part のみ** `thoughtSignature` 等を復元（part マージ禁止）。 |
| `MessageRole::Tool` | `contents[].role = "user"` + `parts[].functionResponse` | 連続する tool message は同一 `Content` に束ねる（後述 JSON）。 |

### Gemini → aibe

| Gemini 側 | aibe 側 | 補足 |
|-----------|---------|------|
| `parts[].text` | assistant content | テキストを assistant 本文として扱う。 |
| `parts[].functionCall` | assistant tool call | `ToolCall` に変換。`id` 欠落時は synthetic id。**その part** の signature 類のみ `provider_extras` へ（part 位置も保存）。 |
| `parts[].functionResponse` | 受信対象外 | モデル応答では想定しない。 |

### 変換ルール

- part の順序は保持する
- 1 回の Gemini 応答に **複数** `functionCall` part がある場合、**すべて** `LlmStepResult.tool_calls` に載せる（0001 の逐次実行は `ToolRoundExecutor` が担当）
- 1 応答に text と functionCall が混在した場合、text は assistant 本文、functionCall は tool call として分離する
- `functionCall.args` は JSON object として `ToolCall.arguments` に写す（OpenAI の arguments 文字列とは異なる）
- `functionCall.id` が欠落した応答は synthetic id を付与したうえで domain に載せる

### synthetic id

- 形式: `call_{turn_index}_{part_index}`（例: `call_0_0`, `call_0_1`）
- **同一 `agent_turn` 内**で一意かつ再現可能であること
- 以降の `functionResponse.id` はこの id と一致させる

## `functionResponse` / `functionCall` の正規 JSON

### tool 結果（aibe → Gemini）

連続 `MessageRole::Tool` を 1 user ターンにまとめる。各 tool ごとに 1 `functionResponse` part。

> **注**: 以下の `response.content` は **aibe 実装上の tool 結果文字列の格納方法**（0010 の adapter 契約）である。Gemini API の `functionResponse.response` は **任意 JSON** を受け付ける（Google 公式）。将来 structured / multimodal tool result を扱う場合は本節を拡張する。

```json
{
  "role": "user",
  "parts": [
    {
      "functionResponse": {
        "id": "call_0_0",
        "name": "read_file",
        "response": {
          "content": "file body or [tool error]\n..."
        }
      }
    }
  ]
}
```

| フィールド | 規則 |
|-----------|------|
| `id` | 必須。対応する `ToolCall.id` と一致。 |
| `name` | 必須。`ToolName` の wire 名（`read_file` 等）。 |
| `response.content` | aibe 契約。`ChatMessage::tool` の `content` 文字列を **そのまま**入れる（`[tool error]` 前置き含む）。0001 の tool result 方針を維持。Gemini API 上の `response` 全体の固定形ではない。 |

### モデル応答の functionCall（Gemini → aibe）

```json
{
  "functionCall": {
    "id": "abc123",
    "name": "read_file",
    "args": { "path": "README.md" }
  }
}
```

- `name` は LLM が返した文字列をそのまま `ToolCall.name` に保持する（組み込み外の名前も可）

## ツール名エラーと 0001 の区別

| 状況 | 挙動 |
|------|------|
| モデルが **組み込みに存在しない** ツール名を返す | 0001 どおり executor が tool result（`tool_not_implemented`）を LLM へ返して **ループ継続** |
| モデルが **allowlist 外の既知ツール** を返す | 0001 どおり executor が tool result（`tool_not_allowed`）を LLM へ返して **ループ継続** |

## 設定・環境変数

### TOML

```toml
[llm]
provider = "gemini"
api_key = "YOUR_API_KEY"
model = "gemini-3.5-flash"
# base_url = "https://generativelanguage.googleapis.com/v1beta"
```

### 環境変数

> **0011 との関係**: 以下の `AIBE_LLM_*` は **legacy フラット `[llm]`** 設定でのみ有効。新形式（`[llm.<name>]` + `[profiles.<name>]`）では無視される — [0011_llm-profiles-spec.md](0011_llm-profiles-spec.md)。

| 変数 | 意味 |
|------|------|
| `AIBE_LLM_PROVIDER` | `gemini` を選ぶ。 |
| `AIBE_API_KEY` | Gemini API key。設定ファイルの `api_key` がなければ参照する。 |
| `AIBE_MODEL` | 省略時の model。Gemini では `gemini-3.5-flash` を既定にする。 |
| `AIBE_BASE_URL` | 省略時の API ベース URL。Gemini では `https://generativelanguage.googleapis.com/v1beta` を既定にする。**OpenAI 互換用に設定した URL が env に残っていると誤接続する**ため、Gemini 利用時は TOML で明示するか env を Gemini 用に上書き・削除する（[manual](../manual/gemini-provider.md) 参照）。 |

### 読み込み規則

- 既存 `parse_llm` と同様、**フィールドごと**に「TOML の `[llm]` に値があればそれを使い、なければ env、なければ provider 既定」
- 例: TOML に `provider = "mock"` があるとき、`AIBE_LLM_PROVIDER=gemini` では **上書きしない**
- **`AIBE_BASE_URL` が OpenAI 互換用（例: `http://127.0.0.1:8080/v1`）のまま残り、`provider = "gemini"` だけ切り替えると誤接続する**。Gemini では TOML の `base_url` を明示するか、env を Gemini 既定に合わせる
- `provider = "gemini"` でも API key が空なら設定エラーにする
- `base_url` は末尾の `/` を `trim_end_matches('/')` で正規化する

## HTTP

### エンドポイント

```text
POST {base_url}/models/{model}:generateContent
```

`base_url` の既定は `https://generativelanguage.googleapis.com/v1beta`。Vertex AI の URL は使わない。

### 認証

- `x-goog-api-key` ヘッダを使う
- API key を URL クエリに入れない
- リクエスト/レスポンスのログに key を残さない

### リクエスト本文 — `complete_with_tools`

```json
{
  "contents": [
    { "role": "user", "parts": [{ "text": "..." }] }
  ],
  "systemInstruction": {
    "parts": [{ "text": "..." }]
  },
  "tools": [
    {
      "functionDeclarations": [
        {
          "name": "read_file",
          "description": "...",
          "parametersJsonSchema": { "type": "object", "properties": {} }
        }
      ]
    }
  ],
  "toolConfig": {
    "functionCallingConfig": {
      "mode": "AUTO"
    }
  }
}
```

`ToolDefinition.parameters` は provider-neutral な JSON Schema である。Gemini v1beta の制限付き `parameters` は `additionalProperties` を受理しないため、adapter は完全な JSON Schema を受理する `parametersJsonSchema` へ変換する。

### リクエスト本文 — `complete`（tools なし）

テキストのみ終端・`finish_text_only`・SummaryPrompt の `llm.complete()` では **`tools` と `toolConfig` を送らない**（`openai_compatible` の `tools: None` と同趣旨）。

```json
{
  "contents": [ "..." ],
  "systemInstruction": { "parts": [{ "text": "..." }] }
}
```

### 受信本文

- `candidates[0].content.parts[*]` を主な解析対象にする
- **すべて**の `functionCall` part を tool call として収集する
- `text` part は assistant 本文に連結する（複数 text がある場合は出現順で連結）
- `candidates` が空、JSON パース失敗、HTTP 非 2xx → `LlmError::Provider`
- `finishReason` が `SAFETY` / `RECITATION` 等で有用な parts が無い、`promptFeedback.blockReason` がある → `LlmError::Provider`（理由文字列に code / reason を含める）

### 参考ルール

- `functionCall.id` と `functionResponse.id` は一致させる
- client-provided tool は `functionCall.name` と `functionResponse.name` の両方を同じ provider-safe 名へ変換する（例: `aish.replay_show` → `aish_replay_show`）
- streaming は別 API のため本仕様では使わない

## TerminationCapability

| provider | `plain_complete_accepts_tool_role` | 備考 |
|----------|------------------------------------|------|
| Gemini | `false` | 安全側。実測で更新するまで Replay は選ばない。 |

max-tool-rounds 終端では 0006 の既定どおり `SummaryPrompt` が使われる（`complete()` は tools なし）。

## 受け入れ条件

1. `provider = "gemini"` で `GeminiLlm` が生成できる（`gemini` 以外の provider 別名は不要）。
2. `api_key` / `model` / `base_url` を TOML と env から解決できる。既定モデルは `gemini-3.5-flash`。
3. `generateContent` は `v1beta` のみを呼び、Vertex AI や streaming を呼ばない。
4. `MessageRole::System` は `systemInstruction` に集約される（`\n\n` 連結）。
5. 連続 `role: tool` は 1 user ターンにまとめ、上記 **正規 `functionResponse` JSON** で送る。
6. `functionCall.id` は `ToolCall.id` に写像され、欠落時は `call_{turn}_{part}` 形式の synthetic id が働く。
7. `thoughtSignature`（等）が付く part では `ToolCall.provider_extras` に **part 単位**で保持し、次ラウンドで **同一 part 位置**に復元される（part マージ禁止）。
8. 1 応答に複数 `functionCall` がある場合、すべて `LlmStepResult.tool_calls` に載る。
9. `complete()` は `tools` / `toolConfig` を送らない。
10. 組み込みに存在しないツール名の `functionCall` → tool result（`tool_not_implemented`）で **ループ継続**（`openai_compatible` テストと同型）。
11. `TerminationCapability.plain_complete_accepts_tool_role` の初期値は `false`。
12. `wiremock` の `aibe/tests/gemini_llm.rs` に HTTP・変換・id fallback・**`agent_turn` 未知ツール** のテストがある。
13. `../security.md` の `AIza` マスク方針と矛盾しない。

## テスト

### 統合（`aibe/tests/gemini_llm.rs`）

| テスト（想定名） | 観点 |
|-----------------|------|
| `gemini_complete_calls_generate_content` | HTTP: mock `generateContent`、`complete` が assistant 本文を返す |
| `complete_with_tools_sends_function_declarations` | HTTP: `tools` / `toolConfig` を body に含める |
| `parse_multiple_function_calls` | 1 応答に複数 `functionCall` part → すべて `tool_calls` |
| `synthetic_id_when_function_call_id_missing` | id 欠落時 `call_{turn}_{part}` |
| `provider_extras_preserves_thought_signature_on_resend` | 受信 part の `thoughtSignature` を `provider_extras` に保存し、2 ラウンド目 request body で **同一 part** に復元（part マージなし） |
| `agent_turn_unknown_tool_from_llm_returns_tool_result_and_continues` | `openai_compatible_llm.rs` と同型 |

### 手動

- [../manual/gemini-provider.md](../manual/gemini-provider.md)

## 実装マップ

| ファイル | 変更 |
|----------|------|
| `aibe/src/domain/tool.rs` | `ToolCall.provider_extras` 追加 |
| `aibe/src/ports/outbound/config.rs` | `LlmConfig::Gemini` |
| `aibe/src/adapters/outbound/toml_config.rs` | `provider = "gemini"` |
| `aibe/src/adapters/outbound/llm_factory.rs` | `build_llm` / `termination_capability` |
| `aibe/src/adapters/outbound/gemini.rs` | adapter 本体 |
| `aibe/src/adapters/outbound/mod.rs` | `GeminiLlm` export |
| `aibe/tests/gemini_llm.rs` | wiremock 統合 |
| `../aibe.config.example.toml` | gemini 設定例（コメント） |
| `../manual/gemini-provider.md` | 手動検証（実装後に cwd 注記済み） |
| `AGENTS.md` | 0010 行追加 |

## 0001 / 0006 / 0008 との関係

| ドキュメント | 関係 |
|-------------|------|
| **0001** | エージェントループは不変。allowlist 外・組み込み未知名とも executor が tool result で継続。 |
| **0006** | capability 初期 `false` → SummaryPrompt 既定。`complete()` は tools なし。 |
| **0008** | wire / `MessageRole` は不変。`provider_extras` は serde 省略で **aibe 内部のみ**。 |

## docs 同期（実装 PR に含める）

- `../architecture.md` — Gemini provider、`generateContent`、`provider_extras`
- `../security.md` — 変更なし想定（`AIza` マスク既存）
- `../aibe.config.example.toml` — gemini 例
- `../manual/README.md` — 索引（済なら維持）
- `AGENTS.md` — 0010 状態を実装済みに更新
- `../0000_spec-index.md` — 0010 を実装済みに更新
- `0006_max-tool-rounds-terminator-spec.md` — Gemini 行の「推測」は manual 実測後に更新可

## 未確定・残リスク

- `gemini-3.5-flash` で `thoughtSignature` が **常に** 必須かは **実 API 実測**で確定する。0010 では part 単位の `provider_extras` により、必須でも非必須でも対応可能
- `provider_extras` を `Value` のままにするか、将来 part 専用 struct にするかは実装時に Google 公式の実測に寄せて判断可（wire 不変）
- `functionResponse.response.content` は現行 aibe の文字列 tool result 用契約。structured / multimodal payload は将来本節を拡張
- 既存 env の **`AIBE_BASE_URL` 残留**による誤接続（OpenAI 互換 URL のまま Gemini を選ぶ）
- streaming / Vertex / built-in tools は本仕様の外
