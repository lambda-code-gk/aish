# aish — AI エージェント向けガイド

このファイルは **未来の自分** と **AI（Cursor / Codex）** のための単一の入口です。実装前に必ず読んでください。

## 目的

シェル操作に LLM を組み込み、**LLM ネイティブなシェル体験** を実現する。

## 優先順位

判断がぶつかったときは、この順で妥協しない。

1. **セキュリティ**（秘密情報・権限・ログ漏洩）
2. **シェル統合体験**（aish のログと ai のコンテキスト連携）
3. **保守性**（レイヤー境界・テスト・ドキュメント）

**後方互換よりアーキテクチャ的に正しい実装** を選ぶ。古い仮 API を残して互換を保つより、依存方向と責務を正す。

## ワークスペース構成

| クレート | 責務 |
|---------|------|
| **aish** | シェルの起動・実行・入出力の記録のみ |
| **aibe** | LLM・エージェントループ・ツール・Unix socket サーバ（他クライアントからも利用可） |
| **ai** | aibe クライアント。LLM API は **直接呼ばない** |

詳細な禁止事項は `.cursor/rules/10-boundaries.mdc` を参照。

## プロトコル・LLM

- **aibe ↔ クライアント**: Unix domain socket + **stdio JSON**（スキーマは `docs/architecture.md` で管理）
- **aibe ツールの cwd**: 相対パスは **クライアント**（`ai` の `current_dir` → `context.cwd`）基準。新規ツールは `ToolExecutionContext::base_dir` / `resolve_path` を使う（`docs/architecture.md`「ツールとカレントディレクトリ」）
- **プロバイダ**（aibe 内）: OpenAI、OpenAI 互換（ローカル等）、Gemini
- **API キー**: aibe の設定ファイルのみ。リポジトリ・ログ・ai バイナリに含めない

## プラットフォーム

- **Unix 専用**（Linux 等）。Windows 対応はスコープ外。

## AI に任せること / 人間が行うこと

| AI が行ってよい | 人間が行う（明示指示がない限り AI はしない） |
|----------------|---------------------------------------------|
| 実装 | `git commit` / `git push` |
| テストの追加・実行 | API キー・本番設定の投入 |
| ドキュメント更新（実装と同期） | セキュリティ設計の最終判断 |
| | 手動シェル検証の最終確認 |

## 完了の定義（DoD）

機能を「完了」と報告するには、すべてを満たすこと。

1. 受け入れ条件を満たす **本番経路の実装**（仮実装・サンプル止まり禁止）
2. 該当する **単体 / 統合 / E2E** テストの追加と成功
3. 手動検証が必要なら `docs/manual/` に手順を書き、未実施ならその旨を報告
4. 挙動・プロトコル・設定に触れたら **`docs/` を同じ変更で更新**
5. 以下が成功すること:

```bash
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
# アーキテクチャに触れた変更では:
./scripts/check-architecture.sh   # クレート境界 + check-hexagonal.sh（application→adapters 等）
```

## 報告義務（必須）

返信の末尾に、該当があれば次の見出しで列挙する。

### 未確定・推測・指示外

- 指示に含まれていない仕様への推測
- 指示と異なる実装・省略した要件
- 意図的に残した TODO や未対応プロバイダ

### 残リスク

- 未実行の手動検証
- セキュリティ上の未検討点

**禁止**: 「最小サンプル」「仮実装」「とりあえず Hello world」のままタスクを完了にしない。

## コミュニケーション

- ユーザー向けの説明・コメント・コミットメッセージ案: **日本語**
- コミットメッセージ案: `feat:` / `fix:` / `docs:` など Conventional Commits 風 + 日本語本文可

## Git

- **main** がベース。機能実装は **feature ブランチ** → 整理後 PR（詳細: `.cursor/rules/05-git-workflow.mdc`）
- **コミット・push はユーザーが明示したときのみ**（feature 実装タスク中の WIP commit は同ルール参照）

```text
仕様(docs/) → feature/<name> 作成 → 実装中は WIP commit → 整理(commit し直し) → push/PR
```

## ツール

- **Cursor**（`.cursor/rules/` が補足規約）— **実装**・テスト・`docs/` の実装同期
- **Codex**（MCP `codex` / `codex-reply`）— **サブエージェント**（repo 内自律調査・編集可、パス境界は `.codex/config.toml`）。`docs/codex-delegation.md`、ルール: `.cursor/rules/50-codex-subagent.mdc`

`cursor_tasks/` の r 系列タスクファイルは **使用しない**。

### Cursor と Codex の流れ

1. 親が **タスク文** + `./scripts/codex-mcp-prompt.sh` を MCP `prompt` に渡す → Codex がサブエージェントとして作業（要約 + `threadId` のみ Cursor に残す）
2. 親が統合・最終判断・必要なら Cursor でも追実装
3. 続きは `codex-reply`。親が diff を絞りたいときだけ `CODEX_USE_PACKET=1`
4. **`git commit` / `push` はユーザー明示時のみ**（Codex にも明示がない限りさせない）

詳細: `docs/codex-delegation.md`（MCP は `sandbox: danger-full-access` 必須）

## ドキュメント

| パス | 内容 |
|------|------|
| `docs/architecture.md` | レイヤー、依存、プロトコル、設定 |
| `docs/testing.md` | テスト種別と実行方針 |
| `docs/security.md` | 秘密情報・ログ・権限 |
| `docs/manual/` | 手動検証チェックリスト |
| `docs/codex-delegation.md` | Codex サブエージェント（権限・MCP 手順） |
| `docs/codex-review.md` | オプション: 厚い diff パケット（`CODEX_USE_PACKET=1`） |
| `docs/0000_spec-index.md` | 000x 仕様・指示書の一覧 |
| `docs/0001_aibe-tool-agent-loop-spec.md` | aibe ツール付きエージェントループ仕様（ドラフト） |
| `docs/0002_ai-tools-client-spec.md` | ai クライアントのツール連携仕様（ドラフト） |
| `docs/0003_architecture-review-refactor-spec.md` | アーキテクチャレビュー反映（指示書・実装済み） |
| `docs/0004_tool-name-type-adoption-spec.md` | ToolName 型の API 全面適用（実装済み） |
| `docs/0005_request-context-domain-spec.md` | RequestContext / AgentTurnContext ドメイン化（未実装） |
| `docs/0006_max-tool-rounds-terminator-spec.md` | max_tool_rounds 終端戦略の port 化（実装済み） |
| `docs/0007_agent-turn-loop-modularization-spec.md` | agent_turn 1 ラウンド実行の分割（実装済み） |
| `docs/0008_chat-message-and-protocol-typing-spec.md` | ChatMessage / MessageRole 型強化（未実装） |
| `docs/0009_ai-tool-category-sync-spec.md` | ai カテゴリ表と aibe ツール名の同期強化（未実装） |
| `.codex/config.toml` | Codex プロファイル（CLI/MCP は scripts 経由） |
| `scripts/codex-fix-linux-sandbox.sh` | bwrap / Landlock 診断 |

機能変更時は、上記のいずれかを **必ず** 実装と同時に更新する。

## 直近の実装目標（参考）

- **aibe**: ツールを使ったエージェントループ、Unix socket + stdio JSON
- **aish**: シェル実行とログ記録（LLM・aibe へのネットワークなし）
- **ai**: aibe 経由で LLM 応答を表示（LLM HTTP クライアント禁止）

## 関連ファイル

- `.cursor/rules/00-project.mdc` — 常時: 目的・優先順位・言語
- `.cursor/rules/05-git-workflow.mdc` — 常時: feature ブランチ・WIP commit・整理 commit
- `.cursor/rules/10-boundaries.mdc` — 常時: クレート境界
- `.cursor/rules/20-rust.mdc` — `**/*.rs`
- `.cursor/rules/30-architecture.mdc` — クレート配下
- `.cursor/rules/40-no-stubs.mdc` — 常時: 仮実装禁止・報告
