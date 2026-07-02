# 0054 — Safe File Write Tools Phase 9 実装指示書

> **種別**: 実装指示書（`docs/tasks/`）  
> **設計正本**: [0054_safe-file-write-tools-spec.md](../spec/0054_safe-file-write-tools-spec.md)  
> **マスター**: [0054_safe-file-write-tools-implementation-spec.md](0054_safe-file-write-tools-implementation-spec.md)  
> **状態**: 未着手（Phase 9）  
> **前提**: [Phase 8](0054_safe-file-write-tools-phase8-implementation-spec.md) 完了

## 0. 目的

設計書 §28（受け入れシナリオ）、§30（完了条件）、§27.10–27.11 の **残り統合テスト** と **docs / manual 同期** を行い、0054 を完了とする。

## 1. スコープ

### 1.1 対象

| 項目 | 設計参照 |
|------|----------|
| socket disconnect 時に write しない | §27.10 |
| 監査フィールド完全性（risk / approval_source / decision） | §27.11 |
| §28.1–28.4 受け入れシナリオの自動 or 手動手順 | §28 |
| example config | §29 Phase 7 |
| `docs/architecture.md` 同期 | §25 |
| `docs/security.md` 同期 | §26 |
| `docs/testing.md` 同期 | §27 |
| `docs/manual/` 手動検証手順 | §28 |
| `docs/0000_spec-index.md` 更新 | 完了時 |
| 指示書を `docs/done/` へ移動 | 完了時 |

### 1.2 非対象

| 項目 | 理由 |
|------|------|
| `verify.sh` を spec-acceptance に登録 | 0050 同様。循環実行回避 |

## 2. 受け入れ条件

| id | 条件 | テスト関数 | pending |
|----|------|------------|---------|
| `disconnect_during_approval` | disconnect で write しない | `disconnect_during_write_approval_writes_nothing` | true |
| `audit_write_like_risk_class` | risk_class = write_like | `write_tools_audit_uses_write_like_risk_class` | true |
| `audit_approval_source_vocabulary` | approval_source 固定語彙 | `write_tools_audit_uses_fixed_approval_source` | true |
| `audit_decision_matrix` | success/deny/unavailable/stale/no_change 識別 | `write_tools_audit_decision_matrix` | true |
| `shell_exec_regression` | shell 承認の既存動作を壊さない | `shell_exec_approval_regression_unchanged` | true |
| `acceptance_create_scenario` | §28.1 新規作成シナリオ | `acceptance_write_file_create_flow` | true |
| `acceptance_patch_scenario` | §28.2 既存修正シナリオ | `acceptance_apply_patch_flow` | true |

## 3. docs 更新チェックリスト

| ファイル | 追記内容 |
|----------|----------|
| `docs/architecture.md` | write tool 責務境界、承認プロトコル、`@edit` |
| `docs/security.md` | write tool 不変条件、non-TTY、`file:write` capability |
| `docs/testing.md` | 新規テストファイルと役割分担 |
| `docs/manual/ai-ask-tools.md` 等 | file write 承認の手動手順（§28） |
| `docs/examples/` または config コメント | `[tools.file_write]` 例 |

## 4. 手動検証（§28）

未実施の場合は完了報告に明記する。

```bash
# §28.1 新規作成
ai --tools @edit 'src/example.rsを作成してください'

# §28.2 既存修正
ai --tools @edit 'このエラー処理を修正してください'

# §28.3 外部 editor 競合（承認待ち中に別 editor で編集 → stale_file）

# §28.4 拒否（n → approval_denied、ファイル不変）
```

## 5. 全体完了条件（§30）

1. `write_file` / `apply_patch` が first-class tool として動く
2. `@edit` から明示有効化
3. Smart Preprocessor 暗黙有効化なし
4. SHA-256 必須（replace / apply_patch）
5. 承認前に実 diff 表示
6. 拒否時ファイル不変
7. non-TTY `ask` fail-closed
8. 承認待ち外部変更を上書きしない
9. allowed root 外 / symlink 拒否
10. binary / special file 拒否
11. atomic write
12. journal 作成
13. journal 失敗時 write なし
14. raw content/patch 監査に残さない
15. `@full` 不変
16. shell approval 非破壊
17. `./scripts/verify.sh` 成功
18. security invariant 対応テストあり

## 6. 完了作業

```bash
./scripts/verify.sh
./scripts/check-spec-acceptance.py   # spec=0054 がすべて pending=false
```

1. `docs/tasks/0054_safe-file-write-tools*.md` を `docs/done/` へ移動
2. `docs/0000_spec-index.md` で 0054 を「設計確定（実装済み）」に更新

## 7. 仕様との差分

- なし（意図的縮小なし）
