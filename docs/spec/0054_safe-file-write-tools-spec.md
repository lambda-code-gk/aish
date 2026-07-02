# 0054 — Safe File Write Tools 設計書

> **種別**: 設計書（`docs/spec/`）
> **状態**: 設計確定
> **起票**: 2026-07-02
> **実装**: [マスター指示書](../tasks/0054_safe-file-write-tools-implementation-spec.md)（Phase 1–9 に分割）
> **関連**: [0023_shell-exec-approval-hardening-spec.md](../done/0023_shell-exec-approval-hardening-spec.md)、[0036_shell-exec-approval-ux-spec.md](0036_shell-exec-approval-ux-spec.md)、[0049_aish-command-output-replay-spec.md](0049_aish-command-output-replay-spec.md)、[architecture.md](../architecture.md)、[security.md](../security.md)、[testing.md](../testing.md)

## 0. 目的

AIBE の first-class tool として、LLM が安全にテキストファイルを作成・変更できる機能を追加する。

追加するツール:

```text
write_file
apply_patch
```

狙い:

1. `shell_exec` に `sed -i`、`cat > file`、`python` 等を渡さず、構造化されたファイル編集を行う
2. 実行前に、実際に適用される差分をユーザーへ提示する
3. 読み取り後に外部でファイルが変更された場合、古い内容で上書きしない
4. 許可されたディレクトリ外、symlink 経由、バイナリファイル等への書き込みを拒否する
5. 書き込みを原子的に行い、途中失敗でファイルを壊さない
6. 成功前の内容を退避し、手動復旧可能な状態を残す
7. 将来の client-side tool 実行へ移行可能な責務境界を保つ

## 1. 設計上の結論（v1）

- `write_file` / `apply_patch` は AIBE の first-class tool
- `shell_exec` や外部 `patch` コマンドは使用しない
- 書き込み先は `context.cwd` 基準
- 書き込み可能範囲は read とは別の `allowed_roots`
- write tool はクライアント allowlist に明示されている場合のみ LLM へ公開
- `route_turn` / Smart Preprocessor は write tool を自動追加しない
- 既定承認モードは `ask`
- `ask` では AIBE が実ファイルから生成した差分を `ai` に送り、承認後のみ書き込む
- 既存ファイル変更では SHA-256 楽観的排他制御を必須
- 1 tool call で 1 ファイルのみ
- 削除 / rename / chmod / 親 dir 自動作成 / 複数ファイル transaction は v1 外

## 2. 非目標

任意バイナリ編集、削除、rename、permission 変更、親 dir 自動作成、複数ファイル atomic transaction、git 自動実行、GNU patch 完全互換、remote FS、自動 rollback tool、write 承認 session cache、destructive 自動承認、`@full` の黙示拡張、Windows、`revert_file_change`（別仕様）。

## 3–25. 詳細仕様

設計レビュー稿（2026-07-02）の §3–§25 と同一。実装時の正本は本ファイルおよび各 Phase 指示書。

主要節:

| 節 | 内容 |
|----|------|
| §4 | パック構成: **No** |
| §5 | tool 名 `write_file` / `apply_patch` |
| §6 | `@edit` category、`@full` 不変、Smart Preprocessor 禁止 |
| §7 | `Capability::FileWrite`（`file:write`） |
| §8 | `read_file(include_metadata=true)` |
| §9 | `write_file` create/replace |
| §10 | `apply_patch` strict unified hunk |
| §11 | テキスト / サイズ / 種別制約 |
| §12 | write 専用 `allowed_roots`、`safe_path` 共通化 |
| §13 | 承認 `never`/`ask`/`always`、session cache なし |
| §14 | 汎用 `ToolApprovalPrompt` / `ToolApproval` |
| §15 | `ai` 承認 UI（stderr、non-TTY fail-closed） |
| §16 | AIBE 生成 unified diff preview |
| §17 | prepare → approve → revalidate → journal → commit |
| §18 | temp + rename atomic write |
| §19 | rollback journal |
| §20 | 監査 sanitized arguments、`write_like` risk |
| §21 | 固定エラー語彙（28 種） |
| §22 | error でも turn 継続 |
| §23 | `ToolRegistry` executor イテレータ化 |
| §24 | 推奨モジュール構成 |
| §25 | 責務分離（protocol / aibe / ai） |

### §21 エラー語彙（正本）

```text
invalid_arguments
tool_disabled
capability_denied
write_denied_by_policy
path_not_allowed
symlink_not_allowed
unsupported_file_type
parent_not_found
target_exists
target_not_found
precondition_required
stale_file
invalid_utf8
binary_file_not_supported
unsupported_line_endings
file_too_large
input_too_large
invalid_patch
patch_conflict
approval_denied
approval_unavailable
journal_failed
journal_capacity_exceeded
write_failed
timeout
cancelled
```

### §26 セキュリティ不変条件（18 件）

1. allowlist 明示必須 2. Smart Preprocessor 自動追加禁止 3. `FileWrite` capability 必須 4. write `allowed_roots` 外拒否 5. `..` 拒否 6. symlink/special 拒否 7. expected SHA-256 必須 8. commit 前 hash 再確認 9. モデル patch を preview 正本にしない 10. `ask` で UI なしは fail-closed 11. raw content/patch を監査に残さない 12. journal 失敗時 write しない 13. 直接 truncate write 禁止 14. timeout/cancel で部分書き込み残さない 15. 1 call 1 ファイル 16. delete/rename/chmod 暗黙禁止 17. `@full` 不変 18. shell approval 非破壊

## 27. テスト要件

§27.1–27.11 は `scripts/spec-acceptance.toml`（`spec = "0054"`）に Phase ごと登録。各 Phase 指示書に AC 対応表あり。

## 28. 受け入れシナリオ

- §28.1 新規作成（`ai --tools @edit`）
- §28.2 既存修正（`read_file` metadata → `apply_patch`）
- §28.3 外部 editor 競合 → `stale_file`
- §28.4 ユーザー拒否 → `approval_denied`、ファイル不変

## 29. 実装順序（9 Phase）

| Phase | 指示書 | 内容 |
|-------|--------|------|
| 1 | [phase1](../tasks/0054_safe-file-write-tools-phase1-implementation-spec.md) | protocol / name / config / DTO / registry |
| 2 | [phase2](../tasks/0054_safe-file-write-tools-phase2-implementation-spec.md) | safe_path / SHA-256 / text / read_file 移行 |
| 3 | [phase3](../tasks/0054_safe-file-write-tools-phase3-implementation-spec.md) | read_file metadata |
| 4 | [phase4](../tasks/0054_safe-file-write-tools-phase4-implementation-spec.md) | diff / atomic / journal |
| 5 | [phase5](../tasks/0054_safe-file-write-tools-phase5-implementation-spec.md) | FileChangeService / ToolApprovalGate |
| 6 | [phase6](../tasks/0054_safe-file-write-tools-phase6-implementation-spec.md) | @edit / write_file |
| 7 | [phase7](../tasks/0054_safe-file-write-tools-phase7-implementation-spec.md) | apply_patch |
| 8 | [phase8](../tasks/0054_safe-file-write-tools-phase8-implementation-spec.md) | ai 承認 UI |
| 9 | [phase9](../tasks/0054_safe-file-write-tools-phase9-implementation-spec.md) | 統合 / docs |

前 Phase の `pending = false` になるまで次 Phase に進まない。

## 30. 完了条件

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
