# 0018 — safe-tools-policy の docs 同期指示書 — 仕様ドラフト

> **出典**: `docs/architecture.md`、`docs/manual/ai-ask-tools.md`、`docs/todo/chatgpt-review-4th-gen/p2-safe-tools.md`。
> **状態**: **仕様ドラフト（docs 同期用）**

## 目的

`safe-tools-policy` の正本は `docs/architecture.md` にある一方で、検証方針は `docs/testing.md`、危険操作の扱いは `docs/security.md` に分散している。さらに、`docs/0000_spec-index.md` と `docs/todo/README.md` の文言が「何を正式指示書として読むべきか」を十分に示していない。

この指示書は、**実装仕様ではなく docs の同期基準** を固定する。対象は `safe tools`、`shell_exec` の明示承認、write-like tools の dry-run / approval、監査の考え方である。本文で新しい機能を定義し直さない。

## 背景

`docs/architecture.md` には、`read_file` / `list_dir` / `grep` / `git_diff` / `git_status` を safe tools とし、`shell_exec` を `@exec` か literal 指定でのみ許可する方針が書かれている。`docs/manual/ai-ask-tools.md` には `ai ask` の手動確認手順がある。

一方で、`docs/testing.md` はテスト種別の総論が中心で、safe-tools-policy の unit / integration / manual の対応表がない。`docs/security.md` も、秘密情報・ログ・timeout/reap の説明はあるが、`shell_exec` の明示承認や future write-like tools の dry-run / approval を security の正本として明示していない。

その結果、次の誤読が起きやすい。

1. `safe tools` の位置づけが `architecture.md` と `testing.md` で揃っていない。
2. `shell_exec` の危険操作としての扱いが `security.md` で十分に言い切られていない。
3. `docs/0000_spec-index.md` と `docs/todo/README.md` の文言が、0018 を「何のための正式指示書か」まで示していない。

## スコープ

### 対象

- `docs/testing.md` の safe-tools-policy 向け追記
- `docs/security.md` の safe-tools-policy 向け追記
- `docs/0000_spec-index.md` の 0018 行の要約文言調整
- `docs/todo/README.md` の 0018 参照文言調整
- 必要なら `docs/todo/chatgpt-review-4th-gen/p2-safe-tools.md` の案内文言調整

### 非対象

- `aibe` / `ai` / `aish` のコード変更
- 新しい tool 実装
- protocol / transport / socket 仕様変更
- `docs/manual/ai-ask-tools.md` の本文変更
- `docs/architecture.md` の設計変更
- API キー、設定値、実行コマンドの追加

## 正本の扱い（優先順位）

この指示書（0018）は **docs 同期の補助指示書** であり、実装仕様の上位正本ではない。解釈が衝突した場合は、次の優先順位で読む。

| 文書 | 役割 |
|------|------|
| `docs/architecture.md` | **上位正本**。safe tools / `shell_exec` / cwd / protocol の仕様を定義する |
| `docs/testing.md` | 領域別正本（検証）。どの unit / integration / manual が policy を担保するかを定義する |
| `docs/security.md` | 領域別正本（安全）。危険操作、承認、監査、秘密情報、dry-run を定義する |
| `docs/manual/ai-ask-tools.md` | 運用手順の正本。実行手順と確認方法を定義する |

## 確定した方針

| 項目 | 方針 |
|------|------|
| safe tools の文言 | `read_file` / `list_dir` / `grep` / `git_diff` / `git_status` を safe tools として扱い、読み取り目的で `shell_exec` を使う説明は置かない |
| `shell_exec` | `@exec` または literal 指定の明示がある場合のみ dangerous tool として扱う |
| write-like tools | `write_file` / `replace_file` / `apply_patch` は dry-run → approval → execute を前提に文書化する |
| server-side enforcement | client 側の警告だけに依存せず、aibe 側でも拒否できる前提で書く |
| audit | dangerous request には `risk_class` / `approval_state` / `dry_run` を追跡できることを明記する |
| 既存文言 | 既存の説明を壊さず、補足で増やす |

## 実装タスク分解

### 1. `docs/testing.md`

- safe-tools-policy に対応する `unit / integration / manual` の所在を追加する
- `ai/tests/` と `aibe/tests/` の役割を、`@read-only` / `@exec` / literal `shell_exec` の観点で明記する
- `docs/manual/ai-ask-tools.md` の手順がどの検証に対応するかを明示する

### 2. `docs/security.md`

- `shell_exec` の明示承認を security の正本として再記述する
- write-like tools の dry-run / approval / execute の順序を追記する
- dangerous request の監査観点を追加する

### 3. `docs/0000_spec-index.md`

- 0018 の要約文言を `docs` 同期の正式指示書として整える
- 0018 が未実装機能ではなく、文書整備タスクであることを誤読しにくくする

### 4. `docs/todo/README.md`

- 0018 への参照文言を、P2 の docs 同期指示書として整える
- 既存の sprint 状態表現と矛盾しないようにする

### 5. 参照整合の確認

- `docs/architecture.md` と `docs/manual/ai-ask-tools.md` を参照し、本文の用語を合わせる
- もし manual と食い違う表現が見つかれば、先に manual との対応関係を整理してから本文を修正する

## 受け入れ条件

### 1. `docs/testing.md` が検証の正本になる

- safe-tools-policy に対応する `unit / integration / manual` の所在が、ファイル名つきで読める
- `ai` と `aibe` のどのテストが policy を担保するかが追える
- `docs/manual/ai-ask-tools.md` が manual 検証の入口として明示される

### 2. `docs/security.md` が危険操作の正本になる

- `shell_exec` が `@exec` か literal 指定でのみ許可される方針が明記される
- write-like tools が dry-run → approval → execute を前提にする方針が明記される
- dangerous request の監査に必要な観点が列挙される

### 3. 索引と todo の文言が整合する

- `docs/0000_spec-index.md` の 0018 行が、現在の 0018 本文の性質と矛盾しない
- `docs/todo/README.md` の 0018 参照が、P2 の docs 同期指示書であることを示す

### 4. 既存の正本と矛盾しない

- `docs/architecture.md` の safe tools 方針と衝突しない
- `docs/manual/ai-ask-tools.md` の手順と用語が食い違わない

### 5. 仮実装を残さない

- 「とりあえず」「未定」「あとで書く」だけの空欄を残さない
- 実装未了の機能を、完了したかのような文言で書かない

## テスト計画

| 種別 | 対象 | 期待 |
|------|------|------|
| **integration** | `ai/tests/tool_catalog_sync.rs`、`ai/tests/tool_names_sync.rs`（`tests/` 配下の統合テスト）と `aibe` の tool policy 関連 unit | `@read-only` / `@exec` / `@full` の展開や safe tools の扱いが、本文の説明と一致する |
| **integration** | `ai/tests/ask_integration.rs`、`aibe/tests/request_tool_validation.rs`、`aibe/tests/agent_turn_loop.rs`、`aibe/tests/socket_protocol.rs` | `shell_exec` の拒否、warning 表示、server-side enforcement の入口検証が追える（承認済み通過は追加テストで補完対象） |
| **manual** | `docs/manual/ai-ask-tools.md` | safe tools の表示、`shell_exec` の warning、拒否 / 承認の見え方を確認できる |

### 補足

- この指示書自体は docs 同期のためのものであり、新しい runtime テストを要求しない
- ただし、本文が既存テストの場所と齟齬を起こさないことは必須である

## docs 更新対象

- `docs/testing.md`
- `docs/security.md`
- `docs/0000_spec-index.md`
- `docs/todo/README.md`
- 必要なら `docs/todo/chatgpt-review-4th-gen/p2-safe-tools.md`

## 未確定・残リスク

- `docs/manual/ai-ask-tools.md` の文言が将来変わると、`docs/testing.md` / `docs/security.md` 側も追従が必要になる
- `shell_exec` の警告文言や safe tools の表示順は、実装側の変更に合わせて再同期が必要になる
- docs 同期だけでは runtime の安全性は変わらないため、実装変更が入ると別の指示書が必要になる
