# 0054 — Safe File Write Tools Phase 1 実装指示書

> **種別**: 実装指示書（`docs/tasks/`）  
> **設計正本**: [0054_safe-file-write-tools-spec.md](../spec/0054_safe-file-write-tools-spec.md)  
> **マスター**: [0054_safe-file-write-tools-implementation-spec.md](0054_safe-file-write-tools-implementation-spec.md)  
> **状態**: 完了（Phase 1）  
> **前提**: なし（最初の Phase）

## 0. 目的

設計書 §5–§7、§13.1（設定型）、§14.1–§14.3、§20.2、§23 の **契約と公開面の土台** を追加する。filesystem 書き込み・承認 UI・`@edit` 展開は **本 Phase では行わない**。

## 1. スコープ

### 1.1 対象

| 項目 | 設計参照 |
|------|----------|
| `WRITE_FILE` / `APPLY_PATCH` tool 名 | §5 |
| `KNOWN_TOOLS` 追加、`READONLY_ADVISORY_TOOLS` には含めない | §5 |
| `Capability::FileWrite`（wire: `file:write`） | §7 |
| `StaticCapabilityPolicy::local_full()` へ追加 | §7 |
| `[tools.file_write]` 設定（`FileWriteConfig`） | §12.1 |
| `FileWriteApprovalMode`（`never` / `ask` / `always`） | §13.1 |
| `ClientResponse::ToolApprovalPrompt` / `ClientRequest::ToolApproval` | §14.1–14.3 |
| `ToolApprovalOrigin`（`UiYes` / `UiNo` のみ） | §14.3 |
| `ToolRiskClass::WriteLike`（または同等） | §20.2 |
| `ToolApprovalState` 拡張候補の serde 定義 | §20.3 |
| `DefaultToolRegistry` の executor イテレータ化 | §23 |
| `sanitize_readonly_advisory_tools` が write tool を除外することの固定 | §6.2 |
| Smart Preprocessor / `route_turn` が write tool を自動追加しない回帰 | §6.2 |

### 1.2 非対象（後 Phase）

| 項目 | Phase |
|------|-------|
| `@edit` category 展開 | 6 |
| write tool executor 登録 | 6–7 |
| `read_file` metadata | 3 |
| filesystem / journal / diff | 2–5 |
| `ToolApprovalGate` 実装 | 5 |
| `ai` 承認 UI | 8 |
| `ToolRoundExecutor::require_capability` の write tool 実行時検査 | 6（executor 登録と同時） |

## 2. 受け入れ条件（`spec = "0054"`）

| id | 条件 | テスト関数 | pending |
|----|------|------------|---------|
| `tool_names_known` | `write_file` / `apply_patch` を parse できる | `write_tools_are_known_tool_names` | true |
| `full_category_unchanged` | `@full` 展開に write tool が含まれない | `full_tool_category_excludes_write_tools` | true |
| `sanitize_readonly_excludes_write` | advisory サニタイズが write tool を除外 | `sanitize_readonly_advisory_tools_excludes_write_tools` | true |
| `file_write_capability_wire` | `file:write` capability の wire roundtrip | `file_write_capability_roundtrip` | true |
| `tool_approval_prompt_roundtrip` | `ToolApprovalPrompt` serde roundtrip | `tool_approval_prompt_roundtrip` | true |
| `tool_approval_request_roundtrip` | `ToolApproval` serde roundtrip | `tool_approval_request_roundtrip` | true |
| `file_write_config_defaults` | `[tools.file_write]` 既定値が設計どおり | `file_write_config_defaults_match_spec` | true |
| `tool_registry_duplicate_rejected` | 重複 tool 名登録で起動失敗 | `tool_registry_rejects_duplicate_tool_name` | true |
| `route_turn_no_write_tools` | `route_turn` / local route が write tool を推薦しない | `route_turn_does_not_recommend_write_tools` | true |

## 3. 変更ファイル（目安）

| 区分 | パス |
|------|------|
| protocol | `aibe-protocol/src/tool_name.rs` |
| protocol | `aibe-protocol/src/request.rs`, `response.rs`, `executed_tool.rs` |
| domain | `aibe/src/domain/capability.rs` |
| config | `aibe/src/ports/outbound/config.rs`, `adapters/outbound/toml_config.rs` |
| application | `aibe/src/application/tool_defs.rs`（定義のみ。実行は未登録） |
| application | `aibe/src/ports/outbound/tool_registry.rs`, `adapters/outbound/tools/mod.rs` |
| ai | `ai/src/domain/tools.rs`（`@edit` **はまだ追加しない**） |
| tests | `aibe-protocol/tests/`, `aibe/tests/`, `ai/tests/` |

## 4. 実装手順

### 4.1 tool 名（§5）

```rust
pub const WRITE_FILE: &str = "write_file";
pub const APPLY_PATCH: &str = "apply_patch";
```

`KNOWN_TOOLS` に追加。`READONLY_ADVISORY_TOOLS` には追加しない。

### 4.2 capability（§7）

- `Capability::FileWrite`、wire `file:write`
- `local_full()` に意図的に追加
- **executor 未登録のため** `ToolRoundExecutor` の require は Phase 6 で追加

### 4.3 設定（§12.1）

```toml
[tools.file_write]
enabled = true
allowed_roots = ["."]
approval = "ask"
max_file_bytes = 1_048_576
max_patch_bytes = 1_048_576
max_preview_bytes = 32_768
journal_retention_days = 7
journal_max_bytes = 268_435_456
```

### 4.4 汎用承認 DTO（§14.1–14.3）

`shell_exec` 専用 DTO を流用しない。`ToolApprovalPrompt` に `tool_name`, `risk_class`, `summary`, `paths`, `preview`, `preview_truncated` を含める。

### 4.5 registry 整理（§23）

```rust
pub fn from_executors(executors: impl IntoIterator<Item = Arc<dyn ToolExecutor>>) -> Self
```

重複 tool 名は起動時エラー。

### 4.6 Smart Preprocessor 回帰（§6.2）

`file_write_candidate` が local route / `SetRecommendedTools` に入らないことをテストで固定。

## 5. 検証

```bash
./scripts/verify-targeted.sh --package aibe-protocol
./scripts/verify-targeted.sh --package aibe
./scripts/verify-targeted.sh --package ai
./scripts/check-spec-acceptance.py
```

Phase 完了時: 上記 Phase 1 の AC を `pending = false`、`#[ignore]` 解除後 `./scripts/verify.sh`。

## 6. 完了報告チェックリスト

- [x] write tool executor は **未登録**（スタブ成功禁止）
- [x] `@edit` は **未追加**
- [x] `@full` の既存展開は不変
- [x] shell 承認 DTO を流用していない
