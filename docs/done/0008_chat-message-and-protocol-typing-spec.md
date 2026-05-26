# 0008 — ChatMessage / プロトocol メッセージの型強化 — 指示書

> **出典**: Codex `review`（2026-05-24）低優先度（プロトocol DTO の弱い型付けの残存）。0003 で `AgentTurnStatus` / `ExecutedToolCall` は強化済み。  
> **レビュー**: Codex `review`（2026-05-24）— 本書に反映済み（責務固定・受け入れ条件拡張・実装マップ・段階 PR 境界）。  
> **状態**: **実装済み**（PR 1: role enum）。

## 目的

会話メッセージの `role: String` や、system / user / assistant / tool の **区別を型で表現** し、不正 role 混入・終端要約用 user 判定の文字列 prefix 依存を減らす。

0003 残存例:

- `ChatMessage.role` が `String`
- `initial_user_request()` が `starts_with("[shell log tail]")` 等でフィルタ（`aibe/src/adapters/outbound/terminator/summary.rs`）
- `ProtocolMessage` → `ChatMessage` 変換が無検証（`From` による暗黙変換、`aibe/src/protocol/request.rs`）

## スコープ

### 対象

- `MessageRole` enum（`user`, `assistant`, `tool`, `system`）
- `ChatMessage` の role フィールド型変更とコンストラクタ / 比較 helper
- `ProtocolMessage` → `ChatMessage` の **`TryFrom` 化** と未知 role 拒否
- `initial_user_request` を **型 + 明示タグ**（例: `InjectedContent`）へ段階移行
- **aibe 全体**の role 文字列リテラル比較の排除（terminator / mock / adapter 含む）

### 対象外

- OpenAI / Gemini 固有メッセージ形の adapter 内完結（既存方針）
- マルチモーダル content
- wire JSON の role 文字列変更（引き続き `"user"` 等）

## 確定した設計判断（レビュー反映）

| 項目 | 方針 |
|------|------|
| **wire 互換** | NDJSON の `messages[].role` は **従来どおり JSON 文字列**。変更は **aibe 内部表現** のみ |
| **未知 role** | wire 上の未知 role は **拒否**（無視しない）。`RequestService` が **`invalid_request`** を返す（0003 / architecture のエラーコード表に準拠） |
| **変換経路** | 0005 と同様、protocol → domain は **`TryFrom` のみ**。`From<ProtocolMessage> for ChatMessage` は **削除** |
| **検証の置き場** | `MessageRole` の parse / 検証は **domain**（`TryFrom` 実装）。`RequestService` は変換エラーを protocol エラーに写像するだけ |
| **`system` role** | enum に含める（wire 互換・将来のクライアント / provider 用）。**aibe 内部生成**（`ChatMessage::user` 等）は phase 1 では `user` / `assistant` / `tool` のみ。wire から `system` を受け取った場合は **受理** し会話に載せる。**max-round 終端時も `system` は捨てない**（下記「終端戦略と `system`」） |
| **provider 正規化** | LLM adapter が provider 固有 role を返す場合の正規化は **adapter 維持**（0001 方針） |
| **注入メッセージ** | phase 2 で `InjectedContent`（または `ChatMessageKind`）を domain に追加し prefix 依存を削除。phase 1 では prefix 比較は **残してよい** が、`MessageRole::User` 等の enum 比較へ置換する |
| **不変条件（phase 1）** | shell log 前置・max-round 要約の **ユーザー可視出力**（最終 assistant 本文・`agent_turn_result` の wire JSON）は 0003 / 0006 現行と同一 |

### `MessageRole` / `ChatMessage` API（phase 1）

```rust
// aibe/src/domain/message.rs（準正規義）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
    Tool,
    System,
}

impl MessageRole {
    pub fn parse_wire(s: &str) -> Result<Self, UnknownMessageRole>; // TryFrom から利用
}

pub struct ChatMessage {
    pub role: MessageRole,
  // content, tool_call_id, tool_calls — 既存どおり
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self;       // role = User
    pub fn assistant(...) -> Self;                          // role = Assistant
    pub fn tool(...) -> Self;                               // role = Tool
    pub fn is_role(&self, role: MessageRole) -> bool;
    // phase 1 では ChatMessage::system は **追加しない**（wire 受信のみ system を許容）
}
```

### protocol 変換（phase 1）

```rust
// aibe/src/protocol/request.rs
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProtocolMessage {
    pub role: String,   // wire DTO のまま
    pub content: String,
}

#[derive(Debug)]
pub enum ProtocolMessageConversionError {
    UnknownRole { role: String },
}

impl TryFrom<ProtocolMessage> for ChatMessage {
    type Error = ProtocolMessageConversionError;
    // role を MessageRole::parse_wire、tool 専用フィールドは phase 1 では従来どおり default
}
```

`RequestService::handle` では `messages` を `try_from` で変換し、失敗時:

```rust
ClientResponse::error(id, ErrorCode::InvalidRequest, e.to_string())
```

### 終端戦略と `system`（0006 連携）

`MessageRole::System` と **user メッセージの `[system]` content 前置**は別物。後者は注入・除外対象（`initial_user_request` の prefix フィルタ）。前者は通常の会話メッセージとして扱う。

| 終端戦略 | `MessageRole::System` の扱い |
|----------|-------------------------------|
| **ConversationReplay** | ループ会話を無加工で渡すため **そのまま残る**（0006 現行どおり） |
| **SummaryPrompt** | ループ会話内の `system` を **出現順のまま全件** `final_conversation` 先頭側に含める。その後 `initial_user_request`（1 件）→ 要約 user |

```rust
// summary.rs — SummaryPrompt（0008 以降）
let mut final_conversation: Vec<ChatMessage> = system_messages(conversation);
if let Some(user) = initial_user_request(conversation) {
    final_conversation.push(user);
}
final_conversation.push(ChatMessage::user(summary.into_prompt_section(max_rounds)));
```

`system` を含む会話は SummaryPrompt 終端入力が **意図的に拡張** される（従来は `system` が落ちていた）。`system` なしの会話は 0003 / 0006 現行と同一の終端入力。

## 段階 PR（推奨）

| PR | 内容 | 完了時の状態 |
|----|------|--------------|
| **1 — role enum** | `MessageRole`、`ChatMessage.role` 型変更、`TryFrom<ProtocolMessage>`、`From` 削除、role リテラル比較の enum 化、既知 / 未知 role テスト | prefix フィルタ（`[shell log tail]` 等）は **残存可**。ただし `m.role == "user"` は **`m.is_role(MessageRole::User)`** 等に置換済み |
| **2 — 注入 enum** | `InjectedContent` / `ChatMessageKind`、`inject_shell_log_tail` と `initial_user_request` の prefix 排除 | 0006 の除外プレフィックス定数を **型タグ** に置換 |

PR 1 マージ前に PR 2 の設計（enum 名・フィールド配置）だけ合意しておく。PR 1 で role 比較が散在したまま PR 2 に進むと効果が薄れる。

## 受け入れ条件

1. `ChatMessage.role` が `MessageRole`（serde は snake_case 文字列。wire roundtrip 互換）。
2. **aibe クレート全体**で role 文字列リテラル（`"user"` / `"assistant"` / `"tool"` / `"system"`）を **直接比較しない**。`MessageRole` 比較、`is_role()`、または `ChatMessage::user` 等のコンストラクタ経由に統一。対象例: `terminator/summary.rs`、`mock_llm.rs`、`application/*`。
3. 既知 role（`user`, `assistant`, `tool`, `system`）の protocol roundtrip テスト（JSON → `ProtocolMessage` → `TryFrom` → serialize）が通る。
4. 未知 role の wire メッセージは **`invalid_request`**（`type: error`, `code: invalid_request`）。domain エラーとして application 深部まで伝播させない。
5. 0003 / 0006 の shell log 前置・max-round 要約挙動は **ユーザー可視出力が不変**（`MessageRole::System` を含まない会話。最終 assistant 本文、成功時 `agent_turn_result` の wire 形。内部表現は変えてよい）。
6. max-round 到達時、ループ会話に `MessageRole::System` がある場合、**ConversationReplay / SummaryPrompt のいずれも `system` を LLM 終端入力から落とさない**（SummaryPrompt は上記「終端戦略と `system`」のとおり先頭に連結）。

## 実装マップ

| ファイル | 変更 |
|----------|------|
| `aibe/src/domain/message.rs` | `MessageRole`, `UnknownMessageRole`, `ChatMessage.role` 型変更、helper |
| `aibe/src/domain/mod.rs` | `MessageRole` re-export（必要なら `lib.rs` も） |
| `aibe/src/protocol/request.rs` | `TryFrom<ProtocolMessage>`、`From` 削除、`ProtocolMessageConversionError` |
| `aibe/src/application/request_service.rs` | messages の `TryFrom` 変換と `invalid_request` 写像 |
| `aibe/src/application/agent_turn.rs` | `inject_shell_log_tail`（phase 1: enum 比較のみ / phase 2: 注入タグ） |
| `aibe/src/adapters/outbound/terminator/summary.rs` | `initial_user_request`、`system_messages`（**SummaryPrompt で system 保持**）、enum 比較 |
| `aibe/src/adapters/outbound/terminator/replay.rs` | shell log 関連テスト・比較の追随 |
| `aibe/src/adapters/outbound/mock_llm.rs` | `m.role == "user"` → enum 比較 |
| `aibe/tests/*`, `aibe/src/**/tests` | roundtrip・未知 role・terminator 回帰 |

**含めない**: `tool_round_terminator.rs` は `summary` へ委譲しているだけのため、直接変更は通常不要。

## テスト

### 単体

- `MessageRole::parse_wire`: 既知 4 role、未知 role → Err
- `TryFrom<ProtocolMessage>`: 既知 role → Ok、未知 role → Err

### 統合

- `request_service` / socket: 未知 role 含む `agent_turn` → `invalid_request`
- 既知 role の `agent_turn` → 従来どおり成功

### 回帰（0003 / 0006）

- `[shell log tail]` 注入が `agent_turn.rs` **1 箇所**から出る（phase 1 維持）
- `initial_user_request` が shell log / 要約 user を除外（phase 1: prefix、phase 2: 型タグ）。**`MessageRole::System` は除外しない**
- SummaryPrompt: ループ会話に `role: system` があるとき `final_conversation` 先頭に **出現順で全件** 含める（`system` + 元 user + 要約 user）
- max-round 要約の最終 assistant 本文が phase 1 前後で同一（**`system` なし** fixture）

## 0001 / 0003 / 0005 / 0006 との関係

| ドキュメント | 関係 |
|-------------|------|
| **0001** | エージェントループ・tool result 方針は不変。0008 は `ChatMessage` 内部表現の強化。`MockLlm` / summary 経路の role 比較を 0008 で置換 |
| **0003** | status / tool_calls の型強化は 0003 完了。`ChatMessage.role` は 0008 に委譲（0003 L171） |
| **0005** | `TryFrom` による protocol → domain 変換パターンを **messages にも適用** |
| **0006** | `initial_user_request` の除外プレフィックス（`[shell log tail]` / `[system]` **content 前置** / `## Tool execution results`）は phase 1 維持、phase 2 で型タグ化。`MessageRole::System` は終端入力から **落とさない**（0006 終端入力表を 0008 と同期） |

## docs 同期（実装 PR に含める）

- `../architecture.md` — `messages` 節に「wire role 文字列は不変、内部 `MessageRole`」を 1 行追記
- `AGENTS.md` — 0008 状態を **実装済み** に更新（完了 PR 時）

## 未確定・残リスク

- phase 2 の `InjectedContent` フィールド配置（`ChatMessage` 直付け vs ラッパ enum）は PR 1 着手前に短い ADR 可
- `system` を aibe 内部から生成する要件が将来入った場合、0008 phase 1 後に `ChatMessage::system` を追加する別判断
- `[system]` content 前置（user role）と `MessageRole::System` の混同に注意（前者は注入除外、後者は終端でも保持）
