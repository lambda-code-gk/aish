# 実装の進め方（採用順）

[← 索引](README.md)

**記録日**: 2026-05-26  
**状態**: 採用 — **Sprint 1（P0）・Sprint 2（P1 / 0017）・P2 docs（0018）・P3（0019）完了**（2026-05-29 時点）。各ステップは feature ブランチ + 意味単位の commit。

## 全体の流れ

```text
0. 突き合わせ（verification）— 完了 → [verification.md](verification.md)
1. Sprint 1 / P0 — v0.1 安定化 — **完了**
2. Sprint 2 / P1 — aibe-protocol / aibe-client 境界 — **完了**（0017）
3. P2 docs — safe-tools-policy 同期 — **完了**（0018）
4. Sprint 3 / P2 残り + P3 — 安全なツール拡張 + aish ログ連携 — **P3 完了**（[0019](../../done/0019_aish-session-log-integration-spec.md)）
```

P1 以降は P0 がマージ可能な状態になってから。プロジェクト優先順位（セキュリティ > 体験 > 保守性）に合わせ、**Sprint 1 内の順序はレビュー表の番号とは異なる**（下記）。

## Sprint 1（P0）— 実装順

| 順 | 項目 | 内容 | 参照 |
|----|------|------|------|
| 1 | ~~`command_start` サニタイズ~~ **完了** | 指示書: [0012](../../done/0012_command-start-log-sanitize-spec.md) | [concerns.md](concerns.md) §2 |
| 2 | ~~docs / 実装ズレ~~ **完了** | 指示書: [0013](../../done/0013_provider-docs-alignment-spec.md) | [concerns.md](concerns.md) §1 |
| 3 | ~~CI + スモーク~~ **完了** | 指示書: [0014](../../done/0014_ci-smoke-stabilization-spec.md) | [p0-stabilization.md](p0-stabilization.md) §1, §5 |
| 4 | ~~LICENSE~~ **完了** | リポジトリ直下 [LICENSE](../../../LICENSE)（MIT）、`Cargo.toml` / README 同期 | [p0-stabilization.md](p0-stabilization.md) §4 |

**このスプリントでは新機能を足さない。**

Sprint 1（P0）は [0012](../../done/0012_command-start-log-sanitize-spec.md)・[0013](../../done/0013_provider-docs-alignment-spec.md)・[0014](../../done/0014_ci-smoke-stabilization-spec.md) および LICENSE（MIT）で完了（2026-05-26）。

## Sprint 2（P1）— 実装順

レビュー提案どおり。詳細は [p1-protocol-split.md](p1-protocol-split.md)。

1. `aibe-protocol` クレート分離（wire 型・NDJSON）
2. 可能なら `aibe-client` 分離
3. `ai` の依存を protocol/client のみへ
4. `docs/architecture.md` 更新、既存テスト維持

## Sprint 3（P2 + P3）— 実装順

1. 安全な読み取り系ツール（`read_file` / `grep` / `git_diff` 等）— [p2-safe-tools.md](p2-safe-tools.md)
2. `shell_exec` は明示指定時のみ・承認・監査の強化
3. `aish shell` ログを `ai ask` が自動利用 — [p3-log-integration.md](p3-log-integration.md)
4. 人間向け実行サマリ（`--verbose-tools` 以外）

詳細タスク切り: [sprints.md](sprints.md) Sprint 3。

## 見送り（P0 時点）

- `provider = "openai"` エイリアスの追加（docs 統一で足りる）
- P1 以前の大きなエージェント機能追加

## ブランチ名の目安

- Sprint 1: `feature/v0.1-stabilization`
- Sprint 2: `feature/aibe-protocol-split`
- Sprint 3: `feature/aish-daily-ux`（仮）
