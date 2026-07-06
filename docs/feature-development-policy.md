# Feature Development Policy

新規機能（spec 番号 `0056` 以降）の設計・実装における複雑性とスコープ管理の標準方針。

正本の分担:

| 用途 | 正本 |
|------|------|
| 設計内容 | `docs/spec/00xx_*-spec.md` |
| 機械判定用の複雑性情報 | `scripts/feature-scope.toml` |
| 受け入れ条件とテスト対応 | `scripts/spec-acceptance.toml` |
| 進行中の実装指示 | `docs/tasks/00xx_*-implementation-spec.md` |

## Core outcome

一つの spec で達成するユーザー成果は **一文** で表現する。

例:

```text
親エージェントの ShellExec を人間の対話シェルへ委譲し、
シェル終了後に同じ親エージェントへ結果を返す。
```

複数の「〜し、さらに〜し、また〜する」が必要な場合は **分割を検討** する。

## Minimum vertical slice

開始から終了までを通す、最小の一本の実行経路を定義する。

例:

```text
親 agent
→ ShellExec 要求
→ human shell
→ Ctrl+D
→ synthetic tool result
→ 親 agent 継続
```

Phase 1 はこの経路の E2E を必須とする。詳細は実装指示書テンプレートを参照。

## MVP のデフォルト Fault Model

明記がない限り、新規 feature の MVP は次を **保証しない**。

- プロセスクラッシュ後の自動再開
- OS 再起動後の再開
- 複数プロセスからの同時操作
- 複数ホスト
- exactly-once
- 旧 schema migration
- 他機能との自動統合

**標準 Fault Model:**

```text
単一ホスト・単一ユーザー・正常なプロセス生存中に機能する。
処理中にプロセスが失われた場合、その操作は失敗とし、
ユーザーが元の操作を再実行する。
```

この範囲を超える保証は、理由と追加 spec を必要とする。

## One Novelty Rule

一つの feature spec で許可する大きな新規機構は **原則一つまで** とする。

| 許可 | 分割が必要 |
|------|------------|
| 新しい PTY handoff + 既存 ShellExec への接続 | 新しい PTY handoff + 新しい side agent + Work 統合 + durable recovery |

## Scope Lock

実装開始時に Acceptance Criteria を固定する（`scripts/feature-scope.toml` の `locked_ac_ids`）。

実装開始後に発見された事項は次へ分類する。

| 分類 | 現在の spec をブロックできるか |
|------|------------------------------|
| `BLOCKER_ORIGINAL_AC` | はい |
| `REGRESSION` | はい |
| `SAFETY_WITHIN_FAULT_MODEL` | はい |
| `NEW_REQUIREMENT` | いいえ（別 spec / Deferred） |
| `HARDENING` | いいえ |
| `OUT_OF_FAULT_MODEL` | いいえ |

## Stop-the-line

実装中に以下が必要になった場合、**そのまま追加実装してはならない**。

- 新しい実行主体
- 二つ目の状態機械
- 二つ目の永続正本
- 新しい agent loop
- lease / heartbeat
- reconciler
- journal（永続化目的）
- schema migration
- idempotency key
- 外部副作用の結果不明状態
- クラッシュ後の自動再開

手順:

1. 実装を停止
2. `feature-scope.toml` を更新（`scope_revision` を増やす）
3. Complexity Gate を再判定
4. MVP から削除できるか検討
5. 必要なら別 spec へ分割

詳細: [`.cursor/rules/47-feature-scope.mdc`](../.cursor/rules/47-feature-scope.mdc)

## Complexity Gate

機械判定は `scripts/check-feature-scope.py` が行う。閾値は `scripts/feature-scope.toml` の `[policy]` を参照。

| 判定 | 意味 |
|------|------|
| **Green** | 通常の実装へ進める |
| **Yellow** | `scope_review = "approved"` と設計書の Complexity Gate 節が必須 |
| **Red** | 通常 feature は拒否。`platform` / `governance` は承認例外が必要 |

## 関連ドキュメント

- 設計書テンプレート: [`docs/spec/_feature-spec-template.md`](spec/_feature-spec-template.md)
- 実装指示テンプレート: [`docs/tasks/_implementation-spec-template.md`](tasks/_implementation-spec-template.md)
- 0056 設計書: [`docs/spec/0056_feature-scope-governance-spec.md`](spec/0056_feature-scope-governance-spec.md)
