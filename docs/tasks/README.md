# docs/tasks — 実装指示書（進行中）

Cursor が実装するときの **指示書**（受け入れ条件・タスク分解・テスト計画）の置き場所。

| 項目 | 方針 |
|------|------|
| 作成者 | Codex または Cursor（設計書 `docs/spec/` を正本にする） |
| ファイル名 | `00xx_<topic>-implementation-spec.md`（**設計書と同じ番号**。例: [spec/0026_…](../spec/0026_external-commands-spec.md) → `0026_external-commands-implementation-spec.md`） |
| 設計の正本 | [spec/](../spec/) の設計書（同番号）を参照する |
| 完了後 | **`docs/done/` へ移動**し、[0000_spec-index.md](../0000_spec-index.md) を更新する（**`scripts/spec-acceptance.toml` の当該 spec がすべて `pending = false` のときのみ**） |

**コミット時**: 実装が完了した指示書は、同じ PR / コミットで `docs/done/` へ移す。

受け入れ条件 ↔ テストの正本: [`scripts/spec-acceptance.toml`](../../scripts/spec-acceptance.toml)（検査: `./scripts/check-spec-acceptance.py`）。未到達の AC は `pending = true` + `#[ignore]` テストを **実装より先に** 追加する。

一覧: [0000_spec-index.md](../0000_spec-index.md)
