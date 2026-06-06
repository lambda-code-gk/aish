# 0029 — `ai` UX 仕上げ（yes-exec 検証・history GC）設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **起票**: 2026-06-06  
> **関連**: [0027_ai-ux-spec.md](0027_ai-ux-spec.md)、[0028_ai-ux-gap-closure-spec.md](0028_ai-ux-gap-closure-spec.md)、[security.md](../security.md)、[testing.md](../testing.md)

## 目的

0027/0028 完了後に残る **高優先度** の運用品質ギャップを閉じる。

1. `--yes-exec` が `shell_exec_approval=ask` のときだけ効くことの **自動検証**
2. local history payload/index の **GC（件数上限）**
3. mock 可能な範囲での **streaming delta** 検証（aibe 側）

## 0027/0028 との関係

- 0028 で `--yes-exec` / `YesExecCache` / preset 優先順位は実装済みだが、統合テストが未カバー
- 0028 で history GC は非スコープ。0027 未確定事項を本書で確定する
- `request_messages` による chat replay は 0028 follow-up で実装済み

## 非目標

- typo 時 did-you-mean
- 実 LLM provider への手動 streaming 確認
- aibe first-class conversation 永続化
- doctor/status alias の再設計
- yes-exec キャッシュ自体の GC

## 現状

- `--yes-exec` は `yes_exec_effective = yes_exec && shell_exec_approval == Some("ask")` で gate される
- 非 TTY stdin では `prompt_shell_exec_approval` が fail-closed（0023）
- `YesExecCache` は session 別 JSON（0600）に command+args キーを保存
- history は `index.jsonl` + `payloads/*.json` が無制限に増える
- mock LLM は `complete_streaming` 既定実装（synthetic delta 1 回）のみ

## 機能仕様

### 1. `--yes-exec` 検証

#### 契約（再掲）

- `--yes-exec` は **opt-in**。config 既定にはしない
- `shell_exec_approval=ask` のときだけ session 限定キャッシュが有効
- `never` / `always` / 未設定（aibe 側既定）では `--yes-exec` だけでは prompt bypass しない
- preset の `shell_exec_approval` は aibe config より CLI preset で上書き可能（0028 優先順位）

#### 自動テストで固定するケース

| ケース | 期待 |
|--------|------|
| キャッシュ seed 済み + `--yes-exec` + `ask` | 非 TTY でも approval=true（UI を通らない） |
| キャッシュ空 + `--yes-exec` + 非 TTY | fail-closed（denied） |
| preset `shell_exec_approval=never` + `--yes-exec` | `yes_exec_effective=false`（キャッシュ未使用） |
| `chat --yes-exec` | ask と同じ `execute_turn` 経路（1 ケースで代表確認可） |

### 2. history GC

#### ポリシー（0027 未確定の解決）

| 項目 | 値 |
|------|-----|
| 設定キー | `history_max_entries`（`~/.config/ai/config.toml` トップレベル） |
| 既定値 | **500** |
| `0` | GC 無効（テスト・デバッグ用） |
| 削除単位 | index 行 + 対応 payload ファイル |
| 削除順 | `created_at_ms` が古い順（同時刻は `history_id` で tie-break） |
| 実行タイミング | `record_turn` 成功直後 |
| yes-exec キャッシュ | 対象外 |

#### 実装方針

- `LocalHistoryStore::prune_to_max(n)` で index 再書き込み + payload 削除
- index 再書き込みは temp ファイル + rename（同一ディレクトリ）
- payload 削除失敗は best-effort（index 整合を優先し stderr ログは出さない。テストで検証）

### 3. streaming delta（mock）

- aibe に **テスト専用** LLM で `complete_streaming` が複数 delta を emit するケースを追加
- `agent_turn` が `AssistantStreaming` を複数回 forward することを aibe 統合テストで固定
- 実 API / ai E2E は manual（0028 残リスク）のまま

## セキュリティ

- GC で削除する payload に user_message / shell_log_tail が含まれる。削除はローカル disk 上のみ
- `--yes-exec` テストは echo 等 safe command のみ。実 shell_exec 実行は mock server が final を返すだけでも可
- history_max_entries は DoS 的 disk 増加を抑える。上限変更は user config の責任

## テスト方針

| 種別 | 内容 |
|------|------|
| unit | `prune_to_max`、yes_exec_effective 相当 |
| integration | `ai/tests/yes_exec_integration.rs` |
| aibe integration | multi-delta streaming mock |
| smoke | 変更なし（既存 chat dry-run 維持） |

## 受け入れ条件

- [x] `--yes-exec` + seeded cache が非 TTY で auto-approve する integration テストが pass
- [x] `--yes-exec` + empty cache + 非 TTY が denied する integration テストが pass
- [x] preset `never` + `--yes-exec` が cache を使わない integration テストが pass
- [x] `history_max_entries=3` で 4 件記録後に最古 1 件の payload が消える
- [x] aibe テストで multi-delta streaming が 2 回以上 forward される
- [x] `./scripts/verify.sh` と `./scripts/smoke-mock.sh` が pass

## 未確定事項

- なし（本書で GC 500 件を採用。将来 `--history-gc` CLI は非スコープ）
