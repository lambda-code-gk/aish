# 0005 — RequestContext / クライアントコンテキストのドメイン化 — 指示書

> **出典**: Codex `review`（2026-05-24）ドメインオブジェクト化候補（`RequestContext`、優先度: 中）。0003 で `ClientCwd` と `require_client_cwd()` のみ導入。  
> **状態**: **未実装**。

## 目的

`agent_turn.context`（`shell_log_tail`, `cwd`）を **プロトocol DTO の素朴な入れ物** から、不変条件付きの **ドメイン型** に昇格させる。

- cwd 検証ロジックを protocol 層から domain / application に集約
- 将来のコンテキスト拡張（session id、locale、client 種別）の置き場所を固定
- 非 `ai` クライアント追加時の検証ポリシーを 1 箇所で定義

## スコープ

### 対象

- **aibe**: `RequestContext` → 内部で `AgentTurnContext`（仮称）domain 型へ変換
- `shell_log_tail` の長さ上限・空文字扱い（0001 / security 方針に合わせる）
- `ClientCwd` 必須条件（0003 踏襲: tools 非空時必須）
- **ai**: `AskRequest` → aibe 送信時の context 組み立てを domain 型経由に

### 対象外

- cwd 必須ポリシーの **緩和**（ツール実行時のみ必須等）→ 本書では 0003 厳格方針を維持。緩和するなら別判断・architecture 更新
- `aish` ログ形式変更
- マルチターン会話 state

## 提案する型（案）

```rust
// aibe domain（概念）
pub struct AgentTurnContext {
    pub client_cwd: Option<ClientCwd>,   // tools 非空時 Some 必須（validate で保証）
    pub shell_log_tail: Option<ShellLogTail>,
}

impl AgentTurnContext {
    pub fn for_tool_turn(client_cwd: ClientCwd, tail: Option<ShellLogTail>) -> Self;
    pub fn for_text_only(tail: Option<ShellLogTail>) -> Self;
    pub fn validate_tools_enabled(&self, tools: &[ToolName]) -> Result<(), ContextError>;
}
```

- `ShellLogTail` — 最大バイト数・trim ルールを型で保持（値: `String` ラップ）
- protocol の `RequestContext` は **inbound adapter** として `TryInto<AgentTurnContext>`

## 受け入れ条件

1. `agent_turn.rs` が `RequestContext` フィールドを直接読まず、`AgentTurnContext` を受け取る（または変換直後にのみ使用）。
2. tools 非空 + cwd 欠落は **変換または validate 段階** で `invalid_request`（0003 挙動維持）。
3. `shell_log_tail` 注入（`[shell log tail]` 前置）ロジックは application 1 箇所。
4. hexagonal チェック通過。protocol → domain 変換は `protocol/request.rs` または inbound adapter に閉じる。

## レイヤー配置

| 層 | 責務 |
|----|------|
| `protocol::RequestContext` | serde のみ。wire 互換 |
| `domain::AgentTurnContext` | 不変条件・組み立て |
| `application::AgentTurnService` | validate 済み context でループ |

## テスト

- 単体: `AgentTurnContext::validate_tools_enabled`
- 単体: protocol DTO → domain 変換（相対 cwd、空 tail）
- 回帰: 0003 cwd 必須テスト一式

## 0003 との関係

0003 の `RequestContext::require_client_cwd()` は **移行期 API**。0005 完了後は deprecated し `AgentTurnContext` に統合。

## 未確定・残リスク

- `shell_log_tail` 上限バイト数の正本（ai 側 16KiB 読取 vs aibe 側検証）の **二重定義** をどう避けるか
- 将来フィールド追加時の protocol 後方互換（`#[serde(default)]` 継続）
