# P0: 安定化（v0.1）

[← 索引](README.md)

> **Sprint 1 完了**（2026-05-26）。本ページのチェックリストはすべて実装済み。次は [p1-protocol-split.md](p1-protocol-split.md)。

機能追加より **v0.1 として安心して常用できるライン** を先に作る、というレビュー提案の P0 部分。

| # | 項目 | 内容 |
|---|------|------|
| 1 | ~~CI 追加~~ **完了** | [0014](../../done/0014_ci-smoke-stabilization-spec.md)、`.github/workflows/ci.yml` |
| 2 | ~~docs / 実装ズレ~~ **完了** | [0013](../../done/0013_provider-docs-alignment-spec.md)、[concerns.md](concerns.md) §1 |
| 3 | ~~`command_start` サニタイズ~~ **完了** | [0012](../../done/0012_command-start-log-sanitize-spec.md)、[concerns.md](concerns.md) §2 |
| 4 | ~~LICENSE~~ **完了** | リポジトリ直下に [LICENSE](../../../LICENSE)（MIT） |
| 5 | ~~スモーク固定化~~ **完了** | [0014](../../done/0014_ci-smoke-stabilization-spec.md)、`scripts/smoke-mock.sh` |

この段階では **新機能を足さない** 方がよい、という提案。

## 実装順（採用）

レビュー表の番号とは別に、セキュリティ優先で次の順で着手する（詳細: [implementation-order.md](implementation-order.md)）。

1. ~~`command_start` サニタイズ~~ **完了**（[0012](../../done/0012_command-start-log-sanitize-spec.md)）  
2. ~~docs / provider 表記の整合~~ **完了**（[0013](../../done/0013_provider-docs-alignment-spec.md)）  
3. ~~CI + スモーク~~ **完了**（[0014](../../done/0014_ci-smoke-stabilization-spec.md)）  
4. ~~LICENSE~~ **完了**（MIT、[LICENSE](../../../LICENSE)）  

スプリントへの落とし込み: [sprints.md](sprints.md) Sprint 1
