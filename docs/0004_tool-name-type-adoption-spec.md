# 0004 — ToolName 型の API 全面適用 — 指示書

> **出典**: Codex `review`（2026-05-24）ドメインオブジェクト化候補（優先度: 高）。0003 で型定義のみ実装済み。  
> **状態**: **未実装**。0003 完了後のフォロー。正本の定数は `aibe::domain::tool_name`。

## 目的

組み込みツール名を `String` ではなく **`ToolName` 値オブジェクト** で API 境界まで運び、typo・未知名・`ai` / `aibe` 同期ズレを **コンパイル時またはパース時** に検出する。

0003 で定数正本化（`READ_FILE` / `SHELL_EXEC` / `KNOWN_TOOLS`）は済んでいる。本指示書は **型の全面採用** を定義する。

## スコープ

### 対象

- `aibe`: `agent_turn.tools`、`ToolCall.name`、`ExecutedToolCall.name`、`ToolRegistry::get`、`ToolDefinition.name`
- `aibe`: `definitions_for`、`is_known_tool` の引数・戻り値
- `ai`: `ToolAllowlist` 内部、`resolve_tools` 展開結果
- プロトocol: `ClientRequest::AgentTurn.tools` の **デシリアライズ直後** の検証（wire は引き続き JSON 文字列配列）

### 対象外

- NDJSON の JSON 形変更（クライアントは引き続き `"read_file"` 等の文字列を送る）
- `ai` カテゴリエイリアス（`@read-only` 等）のプロトocol 化 → **0009**
- 動的ツールディスカバリ / `list_tools`

## 設計判断（実装前に確定すること）

| 項目 | 推奨案 | 代替案 |
|------|--------|--------|
| 未知名の扱い | `ToolName::parse(s)` → `Result`、agent_turn 入口で一括検証 | serde カスタム deserializer |
| `ToolRegistry` キー | `HashMap<ToolName, _>` または `ToolName` 比較 | 内部のみ String、境界で変換 |
| `ai` allowlist | `ToolAllowlist(Vec<ToolName>)` | 送信時のみ `String` に落とす |
| エラー型 | `UnknownToolError` を protocol / domain で共有 | クレートごとにラップ |

## 受け入れ条件

1. `KNOWN_TOOLS` 以外の文字列は **`ToolName` 構築前** に拒否される（agent_turn リクエスト検証、ai allowlist 解決）。
2. `ToolCall` / `ExecutedToolCall` の `name` フィールド型が `ToolName`（serde 出力は従来どおり snake_case 文字列）。
3. `cargo test --workspace` / `check-architecture.sh` 通過。
4. 既存統合テストの JSON 文字列アサーションは **変更なし**（wire 互換）。

## 実装方針（案）

```
aibe/src/domain/tool_name.rs     # ToolName 拡張（Display, Serialize, Deserialize）
aibe/src/application/agent_turn.rs
aibe/src/ports/outbound/tool_registry.rs
aibe/src/application/tool_defs.rs
ai/src/domain/tools.rs           # ToolAllowlist: Vec<ToolName>
```

## テスト

| 種別 | 内容 |
|------|------|
| 単体 | `ToolName::from_str` 成功 / 失敗 |
| 単体 | serde roundtrip（JSON 文字列 ↔ ToolName） |
| 回帰 | 0001 / 0002 / 0003 の既存 agent_turn・ask 統合 |

## 0003 との関係

0003 で **見送り** と明記。0004 完了後は `domain/tool_name.rs` の `ToolName` が正本 API となる。

## 未確定・残リスク

- ツール数が少ない間は ROI が低い。3 ツール以上または外部クライアント増加で優先度を上げる。
- serde カスタム実装の失敗メッセージが UX を損ねないか要確認。
