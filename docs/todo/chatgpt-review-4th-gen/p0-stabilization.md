# P0: 安定化（v0.1）

[← 索引](README.md)

機能追加より **v0.1 として安心して常用できるライン** を先に作る、というレビュー提案の P0 部分。

| # | 項目 | 内容 |
|---|------|------|
| 1 | CI 追加 | `.github/workflows` が無い。Linux で `fmt` / `clippy` / `test` / `check-architecture.sh` |
| 2 | docs / 実装ズレ | 特に `provider = "openai"` — [concerns.md](concerns.md) §1 |
| 3 | ~~`command_start` サニタイズ~~ **完了** | [0012](../../done/0012_command-start-log-sanitize-spec.md)、[concerns.md](concerns.md) §2 |
| 4 | LICENSE | README は MIT OR Apache-2.0 想定だがリポジトリ直下に `LICENSE` が無い |
| 5 | スモーク固定化 | `docs/manual` に加え `scripts/smoke-local.sh` 等で mock aibe 導通を自動化 |

この段階では **新機能を足さない** 方がよい、という提案。

## 実装順（採用）

レビュー表の番号とは別に、セキュリティ優先で次の順で着手する（詳細: [implementation-order.md](implementation-order.md)）。

1. ~~`command_start` サニタイズ~~ **完了**（[0012](../../done/0012_command-start-log-sanitize-spec.md)）  
2. docs / provider 表記の整合（`openai` → `openai_compatible` へ明記、エイリアス実装は見送り）  
3. CI + スモーク  
4. LICENSE  

スプリントへの落とし込み: [sprints.md](sprints.md) Sprint 1
