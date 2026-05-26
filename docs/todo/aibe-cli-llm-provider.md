# aibe — CLI ベース LLM プロバイダ（Codex / Claude Code）検討メモ

> **状態**: 検討中（未実装）  
> **起票**: 2026-05-26 — 「API の代わりに Codex CLI や Claude Code CLI を aibe が呼び出せるか」の整理

## 質問

HTTP LLM API（OpenAI 互換 / Gemini 等）の代わりに、**Codex CLI** や **Claude Code CLI** を aibe が subprocess で呼び出すことはできるか。

## 結論（要約）

| 項目 | 判断 |
|------|------|
| 技術的に可能か | **可能**。aibe は outbound adapter として子プロセス起動してよい（`shell_exec` と同様、クレート境界上も LLM 呼び出しは aibe に閉じる） |
| 現行 API のドロップイン代替か | **ツール付きエージェントループではほぼ無理に近い** |
| 現実的な切り口 | まず **ツールなしのテキスト専用プロファイル**；必要なら **agent_turn 全体委譲**を別経路で検討 |

## 現行 aibe の前提

aibe は `LlmProvider` port 経由で、**1 ステップずつ**次を繰り返す（`application/tool_round/executor.rs`）。

1. `complete_with_tools(messages, tool_definitions)` → 構造化された `tool_calls`
2. aibe 側で `shell_exec` / `read_file` を実行（ポリシー・`context.cwd` は aibe が管理）
3. 結果を会話に戻して再推論

関連:

- port: `aibe/src/ports/outbound/llm.rs`
- プロファイル: `docs/done/0011_llm-profiles-spec.md`
- アーキテクチャ: `docs/architecture.md`（aibe → LLM API）

## Codex CLI / Claude Code CLI とのズレ

Codex CLI・Claude Code CLI は **チャット補完 API ではなく、内蔵ツール付きのエージェント実行環境** に近い。

| 観点 | 現行 aibe（OpenAI 互換 / Gemini） | Codex / Claude Code CLI |
|------|-----------------------------------|-------------------------|
| 制御 | aibe がツール定義・実行・max-round 終端を握る | CLI 側が独自ループ・独自ツール |
| 入出力 | `ChatMessage` + `ToolDefinition` | プロンプト文字列・ログ・製品固有フォーマット |
| 認証 | `~/.config/aibe/config.toml` の API キー等 | ログイン / サブスク / CLI 独自設定 |
| `ai` + aish 連携 | `context.shell_log_tail` を載せ、aibe の `shell_exec` で実行 | CLI 内蔵シェルと二重化しやすい |

そのため HTTP アダプタと同じ `LlmProvider` に「CLI を 1 回呼ぶ」だけでは、**`complete_with_tools` の契約を満たせない**可能性が高い（CLI が aibe の `shell_exec` / `read_file` スキーマで function calling してくれる保証は製品・バージョン依存）。

## 実現パターン（現実度順）

### 1. テキストのみ委譲（比較的現実的）

- `tools = []` の `agent_turn` だけ CLI に任せる
- メッセージ列を 1 本のプロンプトにまとめ、CLI を非対話実行 → stdout を assistant 本文として返す
- 設定イメージ: `provider = "codex_cli"` / `"claude_code_cli"` を `[llm.<name>]` に追加（`llm_factory` 拡張）
- **制約**: `ai ask` のツール連携・aish ログとシェル実行の一体ループは使えない

### 2. 丸ごと 1 タスク委譲（設計変更が大きい）

- `agent_turn` 全体を「CLI エージェント 1 本」に渡し、最終回答だけ socket で返す
- aibe の `ToolRoundExecutor` ループはスキップ（またはプロトコルに別 `type`）
- **aish 統合の価値が薄れる**（シェルは CLI 側、ログは aibe/ai 側、という二重構造）

### 3. `LlmProvider` の完全代替（最も難しい）

- CLI が **安定した JSON** で「次に呼ぶツール名 + 引数」を返す必要がある
- 返却を aibe の `ToolRoundExecutor` が実行する形なら、理論上は HTTP と同じループに載せられる
- Codex / Claude Code の公式インターフェースがそれを保証するかは **要調査・バージョン固定**。壊れやすい

### 4. 本リポジトリの Codex MCP とは別物

- `docs/codex-delegation.md` の Codex は **Cursor 親 → MCP サブエージェント** 用
- aibe デーモンから同じ経路を使う設計にはなっていない
- aibe に MCP クライアントを組み込むのは別タスク（常駐・認証・同時リクエストの整理が必要）

## アーキテクチャ・運用上の注意

- **クレート境界**: LLM 呼び出しは aibe に閉じる（`ai` の HTTP 直叩き禁止）。CLI subprocess は aibe adapter として許容される想定（実装時は `scripts/check-architecture.sh` で明示的に問題ないか確認）
- **同時リクエスト**: aibe は Unix socket で複数クライアント想定。CLI は重い・排他になりがち → キュー、ワーカー、タイムアウトが必要
- **セキュリティ**: CLI は独自サンドボックス・ネットワーク権限を持つ。`docs/security.md` の「aibe がツールポリシーを握る」と衝突しうる
- **設定**: `0011` の `[llm.*]` / `api_key` モデルに加え、`command`, `args`, `profile`, `cwd`, `timeout` 等が要る可能性
- **テスト**: CLI 依存は CI で不安定 → 契約テスト用のフェイク CLI バイナリがほぼ必須

## 推奨方針（案）

「API キーなしで Codex / Claude の契約を使いたい」が目的なら:

- **ツールあり `ai ask`**: 従来の HTTP プロバイダのまま
- **調査・長文・単発 Q&A**: CLI 専用プロファイル（テキストのみ）

二系統に分けるのがバランス良い。

## 実装に進む前の確認事項

- [ ] 対象 CLI（Codex / Claude Code / 両方）とバージョン範囲
- [ ] 非対話モード・機械可読出力（`exec` / `--json` 等）の有無とスキーマ固定
- [ ] ツールありかテキストのみか（上記パターン 1 vs 2 vs 3）
- [ ] 認証方式（環境変数・設定ファイル・対話ログインの扱い）
- [ ] aibe 常駐デーモンからの同時実行・タイムアウト・キャンセル
- [ ] 昇格時: `docs/00xx_*-cli-provider-spec.md` として受け入れ条件・手動検証（`docs/manual/`）を書く

## 関連ドキュメント

- [architecture.md](../architecture.md)
- [0011_llm-profiles-spec.md](../done/0011_llm-profiles-spec.md)
- [codex-delegation.md](../codex-delegation.md)
- [manual/ai-ask-tools.md](../manual/ai-ask-tools.md)
- [security.md](../security.md)
