# Codex レビュー深度（`CODEX_TASK=review`）

実装後監査のパケット生成とシェル方針。全体像は [codex-delegation.md](./codex-delegation.md)。

## パケット生成

```bash
./scripts/codex-review-context.sh
# 同等
CODEX_TASK=review ./scripts/codex-context.sh
```

## レビューモード

| `CODEX_REVIEW_MODE` | パケット | Codex シェル目安 |
|---------------------|----------|------------------|
| `fast` | diff + arch-check | 最大 2 回・許可パスのみ |
| `standard`（既定） | + 変更ファイル抜粋 | 最大 12 回 |
| `deep` | + 広い抜粋 | 最大 20 回 |

```bash
CODEX_REVIEW_MODE=deep ./scripts/codex-review-context.sh
```

`developer-instructions` は `.cursor/rules/50-codex-subagent.mdc` の **review** 節。

## 遅延を避ける

- プロンプトに diff / 抜粋が無いと Codex が全リポジトリ探索しやすい（数分級）。
- 再レビューは `codex-reply` + 差分だけ。新規 `codex` で全文再走査しない。

## 深い確認の代替

| 手段 | 説明 |
|------|------|
| `deep` モード | パケットを厚くする |
| 親が `Read` → `codex-reply` | Cursor 側で読み、Codex に貼る |
| `codex review --uncommitted` | MCP 外の CLI 差分レビュー |

## 禁止（review / audit 共通）

- `rg --files` / `find` / `grep -r` / 全体 `cargo test`・`cargo build`
- 許可パス外の読取、同一コマンドの `require_escalated` 連打
