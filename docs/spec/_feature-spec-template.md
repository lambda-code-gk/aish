# 00xx Feature Name 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **関連**: [`docs/feature-development-policy.md`](../feature-development-policy.md)

## 0. Core outcome

ユーザーが最終的にできることを一文で記載する。

## 1. Minimum vertical slice

開始から終了までの最小経路を記載する。

```text
（例: 入力 → 処理 → 出力）
```

## 2. Fault model

### 2.1 保証対象

標準 Fault Model から逸脱する場合のみ記載。逸脱しない場合は「標準 Fault Model に従う」と書く。

### 2.2 保証対象外

MVP で保証しない事項を列挙する。

## 3. Non-goals

今回実装しないものを記載する。

## 4. Complexity inventory

| 項目 | 数・内容 |
|------|----------|
| 実行主体 | |
| 状態機械 | |
| 永続 aggregate | |
| 外部副作用 | |
| プロセス境界 | |
| 新規基盤機構 | |
| 他機能統合 | |

`scripts/feature-scope.toml` の数値と一致させる。

## 5. Complexity Gate

- 判定: Green / Yellow / Red
- 理由:
- 分割判断:
- 承認例外:（Red で platform / governance のみ）

## 6. Complexity budget

実装中に追加可能な上限を記載する（例: 状態機械 +0、永続 aggregate +0）。

## 7. Split triggers

何が必要になったら別 spec へ分けるかを記載する。

## 8. パック構成の適用

Yes / No / 部分適用と理由を記載する（[0045](0045_pack-composition-spec.md) §6 参照）。

## 9. Acceptance Criteria

| ID | 条件 |
|----|------|
| | |

各 row を `scripts/spec-acceptance.toml` に登録する。

## 10. Deferred specs

別 spec へ送る候補を記載する。なければ「なし」。

## 11. Scope change log

| Revision | 分類 | 変更 | 理由 |
|----------|------|------|------|
| 1 | INITIAL | | |
