# 0009 — ai カテゴリ表と aibe ツール名の同期強化 — 指示書

> **出典**: Codex `review`（2026-05-24）— カテゴリ表と `aibe::KNOWN_TOOLS` のドリフト検出。0003 で aibe 定数正本化 + `tool_names_sync` 簡略化済み。  
> **状態**: **実装済み**（案 A、`ai/tests/tool_catalog_sync.rs`）。

## 目的

`ai` 側の `@read-only` / `@exec` / `@full` カテゴリ展開と、`aibe` 組み込みツール名の **同期を機械的に保証** する。0003 時点では `ai/tests/tool_names_sync.rs` が `KNOWN_TOOLS` 受け入れのみ検証し、**カテゴリ ↔ ツール集合** のドリフトは未検査だった。

## スコープ

### 対象

- カテゴリ展開結果が `aibe::KNOWN_TOOLS` と矛盾しないことの **テスト**
- 新ツール追加時のチェックリスト — **運用正本**: [`docs/manual/ai-ask-tools.md`](manual/ai-ask-tools.md#新規組み込みツール追加チェックリスト)
- 採用: **案 A** — `ai/tests/tool_catalog_sync.rs`

### 対象外

- 0004 の `ToolName` API 全面適用（0004 実装済み。本テストは引き続き有効）
- 動的カテゴリ / ユーザー定義カテゴリ
- `list_tools` プロトコル（将来プロトコル追加時のスコープ。現行未実装）

## 設計判断

| 案 | メリット | デメリット | 採用 |
|----|----------|------------|------|
| **A** テストのみ | 最小 diff、0002 方針（カテゴリは ai のみ）維持 | カテゴリ定義は依然 ai 専有 | **採用** |
| **B** manifest 生成 | ドリフトほぼ不可能 | ビルド複雑化 | 未採用 |
| **C** aibe がカテゴリ公開 | 正本が 1 箇所 | 0002「カテゴリは ai のみ知る」方針と矛盾 | 未採用 |

**0002 整合**: 案 A。案 C は 0002 改定が必要。

### 案 B/C 再検討条件

次のいずれかを満たしたら、manifest 生成（案 B）等を再検討する。

- 組み込みツールが **3 件以上** になった
- カテゴリエイリアスが **2 つ以上** 追加された

## 正本の分担

| 内容 | 正本 |
|------|------|
| カテゴリ表の仕様 | `docs/0002_ai-tools-client-spec.md` §カテゴリ表 |
| カテゴリ展開の実装 | `ai/src/domain/tools.rs` `expand_category` |
| ツール名定数 | `aibe::KNOWN_TOOLS`（0003） |
| 新ツール追加の運用手順 | `docs/manual/ai-ask-tools.md` §新規組み込みツール追加チェックリスト |
| 同期テスト | `ai/tests/tool_catalog_sync.rs`（本指示書） |

## 受け入れ条件（案 A — 実装済み）

1. `@read-only` → `{read_file}`、`@exec` → `{shell_exec}`、`@full` → `read_file`, `shell_exec`（固定順）が **それぞれ個別に** テストで固定される。
2. `aibe::KNOWN_TOOLS` に新名が増え、`@full` がカバーしなければテストが失敗する。失敗メッセージに更新箇所とチェックリストへの参照を含む。
3. `ai` の `expand_category` と `aibe::KNOWN_TOOLS` が **一致** することをテストで検証する。`docs/0002` カテゴリ表は **手動同期**（自動検証対象外）。

## 分類責務

新しい組み込みツールを `aibe` に追加するとき、**メンテナ** が `@read-only` / `@exec` / `@full` のどれに含めるか（または複数カテゴリ）を判断する。`@full` は常に **全 KNOWN_TOOLS** を含む集合とする（0002 §カテゴリ表）。

## テスト

実装: `ai/tests/tool_catalog_sync.rs`

- 各カテゴリの展開集合を個別 assert
- `@full` が `KNOWN_TOOLS` と集合一致することを assert
- 展開結果がすべて `aibe::is_known_tool` であることを assert

```rust
#[test]
fn read_only_category_expands() {
    assert_category_eq("@read-only", &[READ_FILE]);
}

#[test]
fn exec_category_expands() {
    assert_category_eq("@exec", &[SHELL_EXEC]);
}

#[test]
fn full_category_expands_in_fixed_order() {
    assert_category_eq("@full", &[READ_FILE, SHELL_EXEC]);
}

#[test]
fn full_category_covers_all_known_tools() {
  // @full と KNOWN_TOOLS の差分を BTreeSet で比較し、
  // missing / extra を失敗メッセージに出す
}
```

既存 `ai/tests/tool_names_sync.rs` は「リテラル指定で KNOWN_TOOLS を受け付ける」検証のまま維持する。

## 0002 / 0003 / 0004 との関係

| ドキュメント | 関係 |
|-------------|------|
| 0002 | カテゴリ表の仕様正本 |
| 0003 | ツール名定数の aibe 正本化済み |
| 0004 | ToolName 型化後も本テストは有効 |

## 未確定・残リスク

- 案 B/C 採用時は 0002 の「カテゴリは ai のみ」を改定する必要あり
- カテゴリへの **分類判断** はテストでは検証しない（メンテナの手順依存）
