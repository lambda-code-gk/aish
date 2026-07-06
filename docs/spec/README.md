# docs/spec — 設計書

Codex が作成する **設計書**（境界・方針・スキーマ案・非目標）の置き場所。

| 項目 | 方針 |
|------|------|
| 作成者 | 既定は **Codex**（`CODEX_TASK=spec`） |
| ファイル名 | `00xx_<topic>-spec.md`（番号は [0000_spec-index.md](../0000_spec-index.md) で管理） |
| 実装指示 | 本ディレクトリには書かない。Cursor 実装用は [tasks/](../tasks/) に **同じ番号**（例: 設計 `0026_*-spec.md` → 実装 `0026_*-implementation-spec.md`） |
| 完了後 | 設計書は **本ディレクトリに残す**（`done/` へ移さない） |

一覧: [0000_spec-index.md](../0000_spec-index.md)

## 新規 feature（0056 以降）

spec 番号 **0056 以降** の新規設計書は、テンプレートに従う。

1. [`_feature-spec-template.md`](_feature-spec-template.md) をコピーして `00xx_<topic>-spec.md` を作成
2. [`docs/feature-development-policy.md`](../feature-development-policy.md) の Core outcome / Fault Model / Scope Lock を満たす
3. `scripts/feature-scope.toml` に feature entry を追加（`status = draft` から開始、実装開始時に `locked`）
4. `./scripts/check-feature-scope.py` で Complexity Gate を確認

0026–0055 は grandfathered（registry 未登録でも可）。大きく拡張する場合は新 spec 番号へ分割する。

直近の大きな設計: [0045_pack-composition-spec.md](0045_pack-composition-spec.md)（optional 機能の静的合成・脱着機構。参照実装は [0038](0038_contextual-memory-pack-phase-d-spec.md) Contextual Memory Pack）。
