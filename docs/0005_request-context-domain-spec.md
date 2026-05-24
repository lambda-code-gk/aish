# 0005 — RequestContext / クライアントコンテキストのドメイン化 — 指示書

> **出典**: Codex `review`（2026-05-24）ドメインオブジェクト化候補（`RequestContext`、優先度: 中）。0003 で `ClientCwd` と `require_client_cwd()` のみ導入。  
> **レビュー**: Codex `review`（2026-05-24）×2 — 本書に反映済み（検証順序・責務固定・公開 API パス・`from_wire` 正規化・移行経路・テスト）。  
> **状態**: **実装済み**。

## 目的

`agent_turn.context`（`shell_log_tail`, `cwd`）を **protocol DTO の素朴な入れ物** から、不変条件付きの **ドメイン型** に昇格させる。

- cwd 検証ロジックを protocol 層から domain / application に集約
- 将来のコンテキスト拡張（session id、locale、client 種別）の置き場所を固定
- 非 `ai` クライアント追加時の検証ポリシーを 1 箇所で定義

## スコープ

### 対象

- **aibe**: `RequestContext` → `AgentTurnContext` domain 型へ変換
- `ShellLogTail` の長さ上限・空文字正規化（0001 / security 方針に合わせる）
- `ClientCwd` 必須条件（0003 踏襲: tools 非空時必須）
- **ai**: `AskRequest` → aibe 送信時の context 組み立てを domain 定数・型経由に

### 対象外

- cwd 必須ポリシーの **緩和**（ツール実行時のみ必須等）→ 本書では 0003 厳格方針を維持。緩和するなら別判断・architecture 更新
- `aish` ログ形式変更
- マルチターン会話 state

## 確定した設計判断（レビュー反映）

| 項目 | 方針 |
|------|------|
| **検証順序** | `tools` 非空なら **cwd 検証を tool 名解決より先** に行う。missing cwd + unknown tool が同時でも **`invalid_request` を優先**（0003 受け入れ条件 2 踏襲） |
| **`shell_log_tail` 上限** | 正本は **aibe** `ShellLogTail::MAX_BYTES`（`16 * 1024`）。**公開 API は `aibe::ShellLogTail`**（`ToolName` と同様に `lib.rs` で re-export）。ai は `aibe::ShellLogTail::MAX_BYTES` を参照し直書き禁止 |
| **上限超過** | inbound 変換時は **truncate**（wire を拒否しない）。ai 側 `tail_bytes` と二重防御 |
| **空 tail** | wire 上 `""` または whitespace のみは **`None` に正規化**（`Err` にしない）。`[shell log tail]` 注入しない（0003 現行挙動維持） |
| **tail 注入** | `[shell log tail]` 前置ロジックは **aibe application 1 箇所**（`agent_turn` または専用 helper） |
| **移行** | 0005 完了 PR で `RequestContext::require_client_cwd()` を **削除**（deprecated 期間なし）。`AgentTurnService` は **`AgentTurnContext` のみ** 受け取る |
| **raw `RequestContext` の寿命** | **`RequestService` 内のみ**。`TryFrom` 後は `AgentTurnService` 以降に `RequestContext` を渡さない |

## 公開 API（クレート境界）

`ClientCwd` / `ToolName` と同じ re-export パターンに揃える。

| 配置 | 内容 |
|------|------|
| `aibe/src/domain/shell_log_tail.rs` | `ShellLogTail` 定義 |
| `aibe/src/domain/mod.rs` | `pub use shell_log_tail::ShellLogTail;` |
| `aibe/src/lib.rs` | `pub use domain::ShellLogTail;`（**ai が参照する正本パス**） |
| `aibe/src/domain/agent_turn_context.rs` | `AgentTurnContext` 定義 |
| `aibe/src/domain/mod.rs` | `pub use agent_turn_context::AgentTurnContext;` |

**ai 側の参照**:

```rust
use aibe::ShellLogTail;

log.tail_bytes(ShellLogTail::MAX_BYTES)?;
```

`16 * 1024` 等のリテラル直書きは **禁止**（回帰テストで検知）。

## ドメイン型（準正規義）

```rust
// aibe/src/domain/shell_log_tail.rs
pub struct ShellLogTail(String);

impl ShellLogTail {
    pub const MAX_BYTES: usize = 16 * 1024;

    /// wire 文字列を正規化。空・空白のみ → None。超過 → MAX_BYTES で truncate。
    pub fn from_wire_opt(raw: &str) -> Option<Self>;

    pub fn as_str(&self) -> &str;
}

// aibe/src/domain/agent_turn_context.rs
pub struct AgentTurnContext {
    pub client_cwd: Option<ClientCwd>,
    pub shell_log_tail: Option<ShellLogTail>,
}

impl AgentTurnContext {
    pub fn for_tool_turn(client_cwd: ClientCwd, tail: Option<ShellLogTail>) -> Self;
    pub fn for_text_only(tail: Option<ShellLogTail>) -> Self;

    /// tools 非空時に cwd が揃っていることを検証。欠落時は `ContextError::MissingCwd`。
    pub fn validate_tools_enabled(&self, tools: &[ToolName]) -> Result<(), ContextError>;
}
```

- protocol の `RequestContext` は **serde のみ**（wire 互換）。domain 変換は `protocol/request.rs` の `TryFrom<RequestContext> for AgentTurnContext` に閉じる。
- `TryFrom` は **cwd 必須検証を行わない**（tools 非空時の cwd 必須は `RequestService` が tool 名解決より先に行う）。
- `TryFrom` 内の tail 変換: `shell_log_tail.as_deref().and_then(ShellLogTail::from_wire_opt)`。

## リクエスト処理フロー（責務固定）

```text
RequestService::handle(AgentTurn { tools, context, .. })
  1. messages を domain 型へ変換
  2. if tools.is_empty() == false:
       context.cwd の cwd 必須検証（domain helper 経由可）→ 失敗なら ErrorCode::InvalidRequest（return）
  3. parse_tool_names(tools) → 失敗なら ErrorCode::ToolNotAllowed
  4. let ctx = AgentTurnContext::try_from(context)?  // protocol/request.rs — ここで RequestContext を消費
  5. AgentTurnService::run(id, messages, tools, ctx)  // ctx のみ。RequestContext は渡さない
```

**移行完了時のシグネチャ（目標）**:

```rust
// application/agent_turn.rs
impl AgentTurnService {
    pub async fn run(
        &self,
        id: String,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolName>,
        context: AgentTurnContext,  // RequestContext ではない
    ) -> ClientResponse { ... }
}
```

| 層 / モジュール | 責務 | raw `RequestContext` を読むか |
|-----------------|------|------------------------------|
| `protocol::RequestContext` | serde のみ。wire 互換 | — |
| `protocol/request.rs` `TryFrom` | tail 正規化・truncate、cwd → `Option<ClientCwd>` パース | **変換時のみ**（所有権消費） |
| `application::RequestService` | ディスパッチ。cwd 先行検証 → `TryFrom` → `run` | **step 2 の cwd 検証のみ** |
| `domain::AgentTurnContext` | 不変条件・組み立て helper・`validate_tools_enabled` | 読まない |
| `application::AgentTurnService` | validate 済み `AgentTurnContext` でループ。tail 注入 1 箇所 | **読まない** |

## 受け入れ条件

1. `agent_turn.rs` が `RequestContext` を引数・フィールド参照しない。`AgentTurnContext` のみ使用。
2. `tools` 非空 + cwd 欠落 / 相対 cwd → **`invalid_request`**（0003 維持）。
3. `tools` 非空 + cwd 欠落 + unknown tool 名が **同時** でも **`invalid_request` を優先**（`tool_not_allowed` にしない）。
4. `shell_log_tail` が空文字・空白のみのとき **`[shell log tail]` を注入しない**（`from_wire_opt` → `None`）。
5. `ShellLogTail::MAX_BYTES` が **aibe 1 定義**。ai は **`aibe::ShellLogTail::MAX_BYTES`** のみ参照（`16 * 1024` 直書きなし）。
6. hexagonal チェック通過。protocol → domain 変換は `protocol/request.rs` に閉じる。**順序 2→3 は維持**。
7. `RequestContext::require_client_cwd()` を削除。呼び出し元・テストに残存しない。
8. `RequestService` 以外の `application` 層に `use crate::protocol::RequestContext` が **残らない**（`RequestService` のみ可）。

## ai 側

| 項目 | 方針 |
|------|------|
| ログ読取 | `log.tail_bytes(aibe::ShellLogTail::MAX_BYTES)?` |
| 送信 | `AskRequest` → `aibe_client` は従来どおり `RequestContext { shell_log_tail, cwd }` を組み立て（wire 形は不変） |
| cwd | 0003 どおり送信直前に絶対パス検証（ai domain） |

## テスト

### 単体（domain / protocol）

- `ShellLogTail::from_wire_opt`: 正常、空文字 → `None`、空白のみ → `None`、上限超過 → truncate 後 `Some`
- `TryFrom<RequestContext>`: 相対 cwd → `client_cwd: None`、絶対 cwd → `Some`、空 tail → `shell_log_tail: None`
- `AgentTurnContext::validate_tools_enabled`: tools 空なら cwd 不要、tools 非空 + cwd 欠落 → Err

### 回帰（0003 / 挙動）

- tools 非空 + cwd 未送信 → `invalid_request`
- tools 非空 + cwd 欠落 + unknown tool **同時** → `invalid_request`（`tool_not_allowed` にならない）
- tools 空 + cwd 未送信 → ok
- `shell_log_tail: ""` → tail 注入メッセージなし
- `shell_log_tail` あり → `[shell log tail]` 前置が 1 箇所から出る

### 回帰（移行・定数）

- **ai**: `ask` 経路が `aibe::ShellLogTail::MAX_BYTES` を参照（`16 * 1024` リテラル不在）。ソース grep または専用テストで固定
- **aibe**: `require_client_cwd` シンボルがコードベースに **存在しない**
- **aibe**: `application/agent_turn.rs` に `RequestContext` 型参照が **存在しない**

## 0003 との関係

0003 の `RequestContext::require_client_cwd()` は **移行期 API**。0005 完了 PR で削除し、`RequestService` の cwd 先行検証 + `AgentTurnContext` に統合する。0003 受け入れ条件 2（検証順序）は本書「確定した設計判断」で継承。

## docs 同期（実装 PR に含める）

- `docs/security.md` — context tail に `aibe::ShellLogTail::MAX_BYTES` 参照を 1 行追加
- `docs/architecture.md` — 必要なら context 節に domain 型名を追記（wire JSON 形は不変）

## 未確定・残リスク

- 将来フィールド追加時の protocol 後方互換（`#[serde(default)]` 継続）
