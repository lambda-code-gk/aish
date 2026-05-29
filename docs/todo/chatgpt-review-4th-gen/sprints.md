# スプリント案

[← 索引](README.md)

レビューが提案する 3 スプリントの切り方。各 P0〜P3 との対応は各ファイル末尾を参照。

## Sprint 1: v0.1 安定化 — **完了**（2026-05-26）

**実装順（採用）** — [implementation-order.md](implementation-order.md):

1. ~~`command_start` args のサニタイズ（+ テスト）~~ **完了** — [0012](../../done/0012_command-start-log-sanitize-spec.md)
2. README の provider 不整合修正（`openai` → `openai_compatible` 明記）
3. ~~CI 追加 + `scripts/smoke-mock.sh`~~ **完了** — [0014](../../done/0014_ci-smoke-stabilization-spec.md)
4. ~~LICENSE 追加~~ **完了**（MIT、[LICENSE](../../../LICENSE)）

**新機能は足さない。**

詳細: [p0-stabilization.md](p0-stabilization.md)

## Sprint 2: クライアント境界整理 — **完了**（0017）

- ~~`aibe-protocol` 分離~~
- ~~`aibe-client` 分離~~
- ~~`ai` は `aibe` 本体ではなく client/protocol のみに依存~~
- ~~`architecture.md` 更新~~
- P2 docs 同期（0018）— **完了**
- 既存テスト維持

詳細: [p1-protocol-split.md](p1-protocol-split.md)

## Sprint 3: AISH 体験の最小完成 — **P3 完了**（0019）

- ~~`aish shell` の現在ログを `ai ask` が自動利用（P3）~~ **実装済み** — [0019](../../done/0019_aish-session-log-integration-spec.md)
- ~~`ai ask` のデフォルト context 確認（P3）~~ **実装済み**（`AI_ASK_LOG=session` / `--session`）
- ~~`read_file` / `grep` / `git_diff` / `git_status` / `list_dir` 追加~~ **実装済み**（safe tools）
- ~~`shell_exec` は明示指定時のみ~~ **実装済み**
- 人間向けの短い実行サマリ整備（`--verbose-tools` 以外）

詳細: [p2-safe-tools.md](p2-safe-tools.md)、[p3-log-integration.md](p3-log-integration.md)
