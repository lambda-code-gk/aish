# 本リポジトリでの突き合わせ

[← 索引](README.md)

ChatGPT レビュー記述とリポジトリの突き合わせ。**初回**: 2026-05-26（`main` 時点）。**P0 完了後**: 同ブランチで再確認（2026-05-26）。

| 項目 | 初回（main） | P0 完了後（`feature/v0.1-stabilization`） |
|------|----------------|-------------------------------------------|
| README `openai` provider | 不一致 | **解消** — [0013](../../done/0013_provider-docs-alignment-spec.md) |
| `command_start` サニタイズ | 生ログ | **解消** — [0012](../../done/0012_command-start-log-sanitize-spec.md) |
| `.github/workflows` | 無し | **解消** — [0014](../../done/0014_ci-smoke-stabilization-spec.md)、[ci.yml](../../../.github/workflows/ci.yml) |
| リポジトリ直下 `LICENSE` | 無し | **解消** — MIT、[LICENSE](../../../LICENSE) |
| P0 の 00xx 指示書 | 未昇格 | **解消** — [0012](../../done/0012)〜[0014](../../done/0014_ci-smoke-stabilization-spec.md) を `docs/done/` に配置 |

**次の着手**: Sprint 2 — [implementation-order.md](implementation-order.md) / [p1-protocol-split.md](p1-protocol-split.md)
