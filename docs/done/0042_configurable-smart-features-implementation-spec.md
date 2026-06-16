# 0042 — Configurable Smart Features 実装指示書

> **設計正本**: [0042_configurable-smart-features-spec.md](../spec/0042_configurable-smart-features-spec.md)

## 実装タスク

### aibe

1. `domain/feature_registry.rs` — TOML 読み込み、trigger マッチ、`FeatureAction` 展開
2. `memory/packs/aish-memory/features.toml` — baseline（inspect_error / clarify_goal / memory_context）
3. `adapters/outbound/filesystem_feature_registry.rs` — `feature_files` 読み込み
4. `MemoryConfig.feature_files` + `toml_config` パース
5. `route_turn.rs` — action schema プロンプト、registry マージ、`RouteTurnService` に registry 注入
6. `RequestService` / `server.rs` — startup 時に registry ロード

### ai

7. `feature_executor.rs` — log tail clamp、`history_summaries` 追加
8. `main.rs` — history 記録時に summary のみ使用
9. `tests/smart_feature_plan.rs` — unit + integration 相当

### docs

10. `architecture.md` — recommended_tools vs SetRecommendedTools
11. `docs/manual/ai-smart-entry.md` — smart feature plan 手動検証
12. `docs/aibe.config.example.toml` — `feature_files` 例

## 受け入れ

- `./scripts/verify.sh`
- `./scripts/smoke-mock.sh`
