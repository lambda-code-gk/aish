# Codex レビュー用パケット（オプション）

**既定の Codex 運用はサブエージェント**（タスク文 + repo 内自律調査）。本書は **親が diff を先に渡したいとき** のオプション。

全体: [codex-delegation.md](./codex-delegation.md)

## いつ使うか

| 方式 | 向く場面 |
|------|----------|
| **既定（パケットなし）** | 広く調べてほしい、関連ファイルの読み落としを減らしたい |
| **`CODEX_USE_PACKET=1`** | 変更範囲を明示したい、Cursor 側で既に diff を選んだ |

## パケット生成

```bash
CODEX_USE_PACKET=1 CODEX_TASK=review ./scripts/codex-mcp-prompt.sh
# または
CODEX_USE_PACKET=1 CODEX_TASK=review ./scripts/codex-context.sh
```

出力の **後ろにタスク文**を足して MCP `prompt` に渡す。

## レビューモード（`CODEX_REVIEW_MODE`）

`codex-context.sh` 内での抜粋量のみ変わる。Codex は **引き続き repo 内を読んでよい**（permission profile の範囲内）。

| モード | パケット |
|--------|----------|
| `fast` | diff + arch-check |
| `standard`（既定） | + 変更ファイル抜粋 |
| `deep` | + 広い抜粋 |

```bash
CODEX_REVIEW_MODE=deep CODEX_USE_PACKET=1 CODEX_TASK=review ./scripts/codex-mcp-prompt.sh
```

## 再レビュー

`codex-reply` + `threadId`。差分だけなら:

```bash
CODEX_REVIEW_MODE=fast CODEX_USE_PACKET=1 CODEX_TASK=review ./scripts/codex-context.sh
```

## 禁止（パケット運用時も）

- リポジトリ外への無許可アクセス（`workspace_roots` 外）
- 実 API キーのコミット
