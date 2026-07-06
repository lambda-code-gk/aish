#!/usr/bin/env python3
"""Generate minimal feature-scope fixture spec markdown."""

from __future__ import annotations

SPEC_BODY = """# {spec} Test Feature

## 0. Core outcome

{outcome}

## 1. Minimum vertical slice

```text
input -> output
```

## 2. Fault model

### 2.1 保証対象

標準 Fault Model に従う。

### 2.2 保証対象外

クラッシュ復旧。

## 3. Non-goals

自動推論。

## 4. Complexity inventory

| 項目 | 数・内容 |
|------|----------|
| 実行主体 | {actors} |

## 5. Complexity Gate

- 判定: {gate}
- 理由: fixture
- 分割判断: {split}
- 承認例外: {exception}

## 6. Complexity budget

状態機械 +0。

## 7. Split triggers

crash recovery が必要になったら分割。

## 8. パック構成の適用

No — fixture。

## 9. Acceptance Criteria

| ID | 条件 |
|----|------|
| {ac_id} | fixture AC |

## 10. Deferred specs

なし。

## 11. Scope change log

| Revision | 分類 | 変更 | 理由 |
|----------|------|------|------|
| 1 | INITIAL | | |
"""


def render_spec(
    spec: str,
    *,
    outcome: str = "Fixture feature outcome.",
    actors: int = 1,
    gate: str = "Green",
    split: str = "単一 spec",
    exception: str = "",
    ac_id: str = "fixture_ac",
) -> str:
    return SPEC_BODY.format(
        spec=spec,
        outcome=outcome,
        actors=actors,
        gate=gate,
        split=split,
        exception=exception,
        ac_id=ac_id,
    )
