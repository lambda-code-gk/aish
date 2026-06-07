# 0030 — `ai` スマート入口 実装指示書

> **種別**: 実装済み指示書（`docs/done/`）  
> **設計の正本**: [0030_ai-smart-entry-spec.md](../spec/0030_ai-smart-entry-spec.md)  
> **状態**: 実装済み  
> **完了確認**: `./scripts/verify.sh` / `./scripts/smoke-mock.sh`

0030 の実装は完了した。詳細な設計は [設計書](../spec/0030_ai-smart-entry-spec.md) を参照する。

## 実装要点

- `aibe-protocol` に `route_turn` の wire DTO を追加
- `aibe-client` に `route_turn` の送受信を追加
- `aibe` に `route_turn` と conversation store を追加
- `aish` から `AI_SESSION_ID` を export
- `ai` に smart entry と `--new` を追加
- テスト、manual doc、architecture / security / testing / index を更新
