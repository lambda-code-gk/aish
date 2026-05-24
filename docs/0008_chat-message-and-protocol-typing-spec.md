# 0008 — ChatMessage / プロトocol メッセージの型強化 — 指示書

> **出典**: Codex `review`（2026-05-24）低優先度（プロトocol DTO の弱い型付けの残存）。0003 で `AgentTurnStatus` / `ExecutedToolCall` は強化済み。  
> **状態**: **未実装**。

## 目的

会話メッセージの `role: String` や、system / user / assistant / tool の **区別を型で表現** し、不正 ro role 混入・終端要約用 user 判定の文字列 prefix 依存を減らす。

0003 残存例:

- `ChatMessage.role` が `String`
- `initial_user_request()` が `starts_with("[shell log tail]")` 等でフィルタ
- `ProtocolMessage` ↔ `ChatMessage` 変換が無検証

## スコープ

### 対象

- `MessageRole` enum（`user`, `assistant`, `tool`, `system` — 0001 使用分）
- `ChatMessage` の role フィールド型変更
- `ProtocolMessage` デシリアライズ時の role 検証（未知 role は `invalid_request` または domain エラー）
- `initial_user_request` を **型 + 明示タグ**（例: `MessageKind::InjectedShellLog`）へ移行する設計検討

### 対象外

- OpenAI / Gemini 固有メッセージ形の adapter 内完結（既存方針）
- マルチモーダル content
- wire JSON の role 文字列変更（引き続き `"user"` 等）

## 設計判断（実装前に確定）

| 項目 | 論点 |
|------|------|
| tool メッセージ | `ChatMessage` に `tool_call_id` 既存。role=tool と整合 |
| 注入メッセージ | shell log / max-round 要約 user を `InjectedContent` enum で持つか、prefix 維持か |
| 後方互換 | 未知 role の JSON を拒否するか無視するか |

**推奨（MVP）**: role enum 化 + 注入メッセージは **別フィールドまたは enum ラッパ** で prefix 依存を段階的に削除。

## 受け入れ条件

1. `ChatMessage.role` が `MessageRole`（serde は snake_case 文字列）。
2. `agent_turn` / terminator が role 文字列リテラル `"user"` を **直接比較しない**（enum 比較または helper）。
3. protocol roundtrip テスト（JSON → ChatMessage → JSON）が既知 role で通る。
4. 0003 の shell log 前置・max-round 要約挙動は **ユーザー可視出力が不変**。

## 実装マップ（案）

```
aibe/src/domain/message.rs       # MessageRole, ChatMessage
aibe/src/protocol/request.rs     # ProtocolMessage 変換
aibe/src/application/tool_round_terminator.rs  # initial_user_request 刷新
```

## 0003 との関係

0003 は status / tool_calls のみ型強化。0008 は **会話モデル** の残り。

## 未確定・残リスク

- 注入メッセージのモデル化は diff が広がりやすい。段階 PR 推奨（role enum → 注入 enum）
- LLM adapter が provider 固有 role を返す場合の正規化責務は adapter 維持
