# 実装指示書テンプレート（docs/tasks/）

> コピーして `00xx_<topic>-implementation-spec.md` として使う。設計書 `docs/spec/00xx_*-spec.md` と同じ番号。

## 0. 目的

（設計書へのリンクと、何を実装するか 1 段落）

## 0.1 Scope Lock

- Feature scope registry: `scripts/feature-scope.toml`
- Scope revision:
- Complexity class: Green / Yellow / Red
- Vertical slice AC ID:
- Locked AC IDs:

## 1. Phase 分割

| Phase | 内容 | ゲート（`scripts/spec-acceptance.toml`） |
|-------|------|------------------------------------------|
| 1 | 最小 Vertical Slice E2E | `pending = false` になるまで Phase 2 に進まない |
| 2 | 異常系・他機能統合・hardening | |

**Vertical Slice Gate**: Phase 1 成功前に、追加 integration / schema migration / crash recovery / 汎用 framework 化 / 性能最適化を実装してはならない。

**実装中の禁止事項**: 新しい実行主体、状態機械、永続 aggregate、外部副作用、クラッシュ復旧が必要になった場合、そのまま追加実装してはならない。`feature-scope.toml` と設計書を更新し、Complexity Gate を再判定すること（[`docs/feature-development-policy.md`](../feature-development-policy.md)）。

## 2. 受け入れ条件

設計書 § 受け入れ条件を表に落とす。**各 row を `scripts/spec-acceptance.toml` に登録** する。

| ID | 条件 | テスト関数 | pending |
|----|------|------------|---------|
| | | | true / false |

未到達の AC は **Rust テストを `#[ignore]` 付きで先に追加** してから実装に入る。

## 3. 完了条件

1. 全 Phase の `spec-acceptance.toml` が `pending = false`
2. `./scripts/verify.sh` 成功
3. 該当 `docs/` 同期
4. 本ファイルを `docs/done/` へ移動し `0000_spec-index.md` 更新（**上記 1 の後のみ**）

## 4. 仕様との差分（意図的に縮小する場合のみ）

- （なければ「なし」）

**禁止**: 黙って algorithm を置き換える。縮小するなら本節と spec 追記のいずれかで可視化する。
