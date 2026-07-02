# 0054 — Safe File Write Tools Phase 6 実装指示書

> **種別**: 実装指示書（`docs/tasks/`）  
> **設計正本**: [0054_safe-file-write-tools-spec.md](../spec/0054_safe-file-write-tools-spec.md)  
> **マスター**: [0054_safe-file-write-tools-implementation-spec.md](0054_safe-file-write-tools-implementation-spec.md)  
> **状態**: 未着手（Phase 6）  
> **前提**: [Phase 5](0054_safe-file-write-tools-phase5-implementation-spec.md) 完了

## 0. 目的

設計書 §6（`@edit`）、§9（`write_file`）、§22 を実装する。Phase 5 の `FileChangeService` に **`write_file` executor** を接続し、**最初の本番 mutation tool** として動かす。`file_change_common.rs` をここで作成し Phase 7 の `apply_patch` と共有する。

## 1. スコープ

### 1.1 対象

| 項目 | 設計参照 |
|------|----------|
| `@edit` category（read-only + write_file + apply_patch） | §6 |
| `@full` は不変 | §6.1 |
| `write_file` create / replace | §9 |
| `file_change_common.rs` | §24.4 |
| `ToolRoundExecutor::require_capability(FileWrite)` | §7 |
| tool_defs LLM description（replace 前に read_file 等） | §9.5 |
| startup warning（allowlist に write tool あり） | §27.1 |
| tool result + `change_id` | §19.5 |
| error 時も turn 継続 | §22 |

### 1.2 非対象

| 項目 | Phase |
|------|-------|
| `apply_patch` parser | 7 |
| `ai` 承認 UI（`approval=ask` の本番 UI） | 8（本 Phase は fake gate または `approval=always` でテスト） |

## 2. 受け入れ条件

| id | 条件 | テスト関数 | pending |
|----|------|------------|---------|
| `edit_category_expansion` | `@edit` が設計どおり展開 | `edit_tool_category_includes_write_tools` | true |
| `write_file_create_success` | create 成功 | `write_file_create_succeeds` | true |
| `write_file_create_target_exists` | 既存へ create 拒否 | `write_file_create_rejects_existing_target` | true |
| `write_file_create_parent_missing` | parent 不存在拒否 | `write_file_create_rejects_missing_parent` | true |
| `write_file_replace_success` | replace + hash 成功 | `write_file_replace_succeeds_with_matching_hash` | true |
| `write_file_replace_requires_hash` | hash 未指定拒否 | `write_file_replace_requires_expected_sha256` | true |
| `write_file_stale_hash` | hash 不一致拒否 | `write_file_replace_rejects_stale_hash` | true |
| `write_file_empty_content` | 空ファイル / 空置換を許可 | `write_file_allows_empty_content` | true |
| `write_file_preserves_permissions` | replace で permission 維持 | `write_file_replace_preserves_permissions` | true |
| `write_file_capability_gate` | capability なし → `capability_denied` | `write_file_requires_file_write_capability` | true |
| `startup_warning_write_tools` | write tool 有効時 warning | `ai_warns_when_write_tools_enabled` | true |
| `race_stale_write_file` | 承認待ち中の外部変更 → `stale_file` | `write_file_detects_stale_file_after_approval_wait` | true |
| `tool_round_capability_gate` | ToolRoundExecutor が FileWrite を要求 | `tool_round_executor_requires_file_write_for_write_tools` | true |

## 3. 変更ファイル（目安）

| 区分 | パス |
|------|------|
| adapter | `aibe/src/adapters/outbound/tools/write_file.rs` |
| adapter | `aibe/src/adapters/outbound/tools/file_change_common.rs` |
| application | `aibe/src/application/tool_defs.rs` |
| application | `aibe/src/application/tool_round/executor.rs` |
| application | `aibe/src/application/server.rs`（composition root） |
| ai | `ai/src/domain/tools.rs`（`@edit`） |
| ai | `ai/src/main.rs` または `clap_cli.rs`（startup warning） |
| tests | `aibe/tests/write_file.rs` |
| tests | `ai/tests/edit_tool_category.rs` |

## 4. 実装手順

### 4.1 `@edit`（§6）

```text
@edit → read_file, list_dir, grep, git_diff, git_status, write_file, apply_patch
```

`@full` は変更しない。

### 4.2 `write_file`（§9）

- `mode=create`: `expected_sha256` 禁止、存在時 `target_exists`、parent 必須、umask `0666` 相当
- `mode=replace`: `expected_sha256` 必須、不一致 `stale_file`、permission 維持

### 4.3 `file_change_common`

prepare 引数検証、path、size、snapshot 読み込みを共通化。Phase 7 は patch 適用だけ追加。

### 4.4 startup warning（§27.1）

`ai` 起動時、tool allowlist に `write_file` または `apply_patch` が含まれるとき stderr に 1 行 warning。

### 4.5 テスト方針

- 単体: fake `ToolApprovalGate` + `approval=always` または制御可能 fake
- race: fake gate で承認を遅延し、別スレッドでファイル変更

## 5. 本 Phase で返すエラー語彙

`invalid_arguments`, `tool_disabled`, `capability_denied`, `parent_not_found`, `target_exists`, `target_not_found`, `precondition_required`, `stale_file`, `input_too_large`, `approval_denied`, `approval_unavailable`, `write_denied_by_policy`

## 6. 検証

```bash
./scripts/verify-targeted.sh --package aibe
./scripts/verify-targeted.sh --package ai
cargo test -p aibe write_file -j 1
./scripts/verify.sh
```
