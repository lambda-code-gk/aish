# 本リポジトリでの突き合わせ

[← 索引](README.md)

ChatGPT レビュー記述と現行 `main` の突き合わせ（**2026-05-26 実施**）。実装順は [implementation-order.md](implementation-order.md)。

- [x] README の `openai` provider 記載が現行 `main` と一致するか → **不一致**。README L195 付近に `openai` 行あり。実装 `parse_provider_kind` は `mock` / `openai_compatible` / `openai-compatible` / `gemini` のみ（`aibe/src/adapters/outbound/toml_config.rs`）。→ [concerns.md](concerns.md) §1
- [x] `aish` の `command_start` 実装がレビュー記述どおりか → **レビューどおり**。`stdout`/`stderr` のみ `sanitize_log_text`。`LogEvent::command_start` は `command`/`args` を生で記録（`aish/src/application/execute_and_record.rs`, `domain/log_event.rs`）。→ [concerns.md](concerns.md) §2
- [x] `.github/workflows` の有無 → **無し**（2026-05-26 時点）。→ [p0-stabilization.md](p0-stabilization.md) §1
- [x] リポジトリ直下の `LICENSE` の有無 → **無し**（`.codex-mcp/skills/**/LICENSE*` のみ）。→ [p0-stabilization.md](p0-stabilization.md) §4
- [x] 上記 P0 を `docs/00xx_*-spec.md` に昇格するか → **Sprint 1 完了後に検討**。着手は [implementation-order.md](implementation-order.md) のチェックリストで進める
