# 0009 — ai カテゴリ表と aibe ツール名の同期強化 — 指示書

> **出典**: Codex `review`（2026-05-24）低優先度（`tool_defs` と `ai/domain/tools` の二重定義）。0003 で aibe 定数正本化 + 同期テスト簡略化済み。  
> **状態**: **未実装**。

## 目的

`ai` 側の `@read-only` / `@exec` / `@full` カテゴリ展開と、`aibe` 組み込みツール名の **同期を機械的に保証** する。0003 時点では `ai/tests/tool_names_sync.rs` が `KNOWN_TOOLS` 受け入れのみ検証し、**カテゴリ ↔ ツール集合** のドリフトは未検査。

## スコープ

### 対象

- カテゴリ展開結果が `aibe::KNOWN_TOOLS` の部分集合であることの **テストまたは生成**
- 新ツール追加時のチェックリストを `docs/` に 1 箇所化
- 選択肢の一つを実装:
  - **A**: `tests/tool_catalog_sync.rs` — カテゴリ表を Rust 定数で持ち、aibe 名と assert
  - **B**: ビルドスクリプト / `build.rs` で JSON manifest から両者生成（将来ツール数増時）
  - **C**: `aibe` が `pub const CATEGORIES: ...` を公開し `ai` が参照（カテゴリを aibe が知る）

### 対象外

- 0004 の `ToolName` API 全面適用
- 動的カテゴリ / ユーザー定義カテゴリ
- `list_tools` プロトocol

## 設計判断（ユーザー判断が必要）

| 案 | メリット | デメリット |
|----|----------|------------|
| **A** テストのみ | 最小 diff、0002 方針（カテゴリは ai のみ）維持 | カテゴリ定義は依然 ai 専有 |
| **B** manifest 生成 | ドリフトほぼ不可能 | ビルド複雑化 |
| **C** aibe がカテゴリ公開 | 正本が 1 箇所 | 0002「カテゴリは ai のみ知る」方針と矛盾 |

**0002 整合の推奨**: **案 A**（テスト強化）。案 C は 0002 改定が必要。

## 受け入れ条件（案 A）

1. `@read-only` → `{read_file}`、`@exec` → `{shell_exec}`、`@full` → 両方、が **テストで固定** される。
2. `aibe::KNOWN_TOOLS` に新名が増えたら、テストが「ai カテゴリ未覆盖」を検出（失敗メッセージに追加手順を含む）。
3. `docs/0002_ai-tools-client-spec.md` カテゴリ表と Rust 定数が **同一**（または doc が生成元である旨を明記）。

## テスト例

```rust
#[test]
fn full_category_matches_known_tools_subset() {
    let expanded = resolve_tools(Some("@full"), &ConfigToolsTokens::default()).unwrap();
    for name in expanded.allowlist.names() {
        assert!(aibe::is_known_tool(name));
    }
    assert_eq!(expanded.allowlist.names().len(), aibe::KNOWN_TOOLS.len());
}
```

（`@full` が KNOWN_TOOLS と一致することを明示）

## 0002 / 0003 / 0004 との関係

| ドキュメント | 関係 |
|-------------|------|
| 0002 | カテゴリ表の仕様正本 |
| 0003 | ツール名定数の aibe 正本化済み |
| 0004 | ToolName 型化後も本テストは有効 |

## 未確定・残リスク

- 案 B/C 採用時は 0002 の「カテゴリは ai のみ」を改定する必要あり
- ツール 2 個の間は手動同期 + 案 A で十分な可能性
