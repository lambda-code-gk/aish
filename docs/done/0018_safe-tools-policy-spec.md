# 0018 — safe-tools-policy の docs 同期正式指示書

> **出典**: `docs/architecture.md`、`docs/manual/ai-ask-tools.md`、`docs/todo/chatgpt-review-4th-gen/p2-safe-tools.md`。
> **状態**: **実装済み**（2026-05-29、docs 同期）

## 目的

`safe-tools-policy` の正本は `docs/architecture.md` にある一方で、検証方針は `docs/testing.md`、危険操作の扱いは `docs/security.md` に分散している。さらに、`docs/0000_spec-index.md` と `docs/todo/README.md` の文言が「何を正式指示書として読むべきか」を十分に示していない。

この指示書は、**実装仕様ではなく docs の同期基準** を固定する。対象は `safe tools`、`shell_exec` の明示承認、**将来の** write-like tools の dry-run / approval、監査の考え方である。本文で新しい機能を定義し直さず、**現時点の正本を補足するだけで新しい実行ポリシーは導入しない**。
本タスクは **docs のみ** を対象とし、コード変更・設定変更・手動実施記録の追加はしない。

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
- `docs/todo/chatgpt-review-4th-gen/p2-safe-tools.md` の案内文言調整

### 非対象

- `aibe` / `ai` / `aish` のコード変更
- 新しい tool 実装
- protocol / transport / socket 仕様変更
- `docs/manual/ai-ask-tools.md` の本文変更
- `docs/architecture.md` の設計変更
- API キー、設定値、実行コマンドの追加
- 実行済みの手動検証記録の追記

## 正本の扱い（優先順位）

この指示書（0018）は **docs 同期の正式指示書** であり、実装仕様の上位正本ではない。解釈が衝突した場合は、次の優先順位で読む。

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
| write-like tools（**未実装・将来**） | `write_file` / `replace_file` / `apply_patch` は、導入時に dry-run → approval → execute を前提に文書化する（現リポジトリに当該ツールはない） |
| server-side enforcement | client 側の警告だけに依存せず、aibe 側でも拒否できる前提で書く |
| audit | dangerous request には `risk_class` / `approval_state` / `dry_run` を追跡できることを明記する |
| 既存文言 | 既存の説明を壊さず、補足で増やす |

## 実装タスク分解

### 1. `docs/testing.md`

- 0018 の検証項目として、既存テストファイル名を明示する
- `ai/tests/tool_catalog_sync.rs`、`ai/tests/tool_names_sync.rs`、`ai/tests/ask_integration.rs` の役割を、`@read-only` / `@exec` / literal `shell_exec` の観点で明記する
- `aibe/tests/request_tool_validation.rs`、`aibe/tests/agent_turn_loop.rs`、`aibe/tests/socket_protocol.rs`、`aibe/tests/agent_turn_tools.rs` の役割を、server-side enforcement と tool result 継続の観点で明記する
- `aibe/src/adapters/outbound/tools/shell_exec.rs` 内の `#[cfg(test)]` 単体テストを、`shell_exec` の kill / reap / timeout 正本として明記する
- `docs/manual/ai-ask-tools.md` の手順がどの検証に対応するかを明示する
- このタスクでは新しいテストを追加しない。既存テストの位置づけを文書化する

### 2. `docs/security.md`

- `shell_exec` の明示承認を security の正本として再記述する
- **将来の** write-like tools の dry-run / approval / execute の順序を追記する（未実装であることを明示）
- dangerous request の監査観点を追加する
- `aibe` 側での拒否・継続の両経路を、警告文言ではなく実行規約として書く

### 3. `docs/0000_spec-index.md`

- 0018 の要約文言を `docs` 同期の正式指示書として整える
- 0018 が未実装機能ではなく、文書整備タスクであることを誤読しにくくする
- 状態表現は「仕様ドラフト」を外し、正式指示書であることが一目で分かる表現にする

### 4. `docs/todo/README.md`

- 0018 への参照文言を、P2 の docs 同期正式指示書として整える
- 既存の sprint 状態表現と矛盾しないようにする
- 参照先は `docs/done/0018_safe-tools-policy-spec.md` に統一する

### 5. `docs/todo/chatgpt-review-4th-gen/p2-safe-tools.md`

- 0018 への案内を維持しつつ、正式指示書として読む前提を明示する
- `concerns.md` のメモから 0018 へ読む流れが分かるようにする

### 6. 参照整合の確認

- `docs/architecture.md` と `docs/manual/ai-ask-tools.md` を参照し、本文の用語を合わせる
- もし manual と食い違う表現が見つかれば、先に manual との対応関係を整理してから本文を修正する

## 受け入れ条件

### 1. `docs/testing.md` が検証の正本になる

- safe-tools-policy に対応する `unit / integration / manual` の所在が、ファイル名つきで読める
- `ai` と `aibe` のどのテストが policy を担保するかが追える
- `docs/manual/ai-ask-tools.md` が manual 検証の入口として明示される
- `aibe/src/adapters/outbound/tools/shell_exec.rs` 内の `#[cfg(test)]` が `shell_exec` の kill / reap 正本として指せる

### 2. `docs/security.md` が危険操作の正本になる

- `shell_exec` が `@exec` か literal 指定でのみ許可される方針が明記される
- **将来の** write-like tools が dry-run → approval → execute を前提にする方針が、未実装であることとともに明記される
- dangerous request の監査に必要な観点が列挙される
- server-side enforcement と client-side warning の役割差が明記される

### 3. 索引と todo の文言が整合する

- `docs/0000_spec-index.md` の 0018 行が、現在の 0018 本文の性質と矛盾しない
- `docs/todo/README.md` の 0018 参照が、P2 の docs 同期正式指示書であることを示す
- `docs/todo/chatgpt-review-4th-gen/p2-safe-tools.md` の案内が、0018 を正式指示書として読む流れを壊さない

### 4. 既存の正本と矛盾しない

- `docs/architecture.md` の safe tools 方針と衝突しない
- `docs/manual/ai-ask-tools.md` の手順と用語が食い違わない
- 0018 の記述が manual より強い新ルールを勝手に作らない

### 5. 仮実装を残さない

- 「とりあえず」「未定」「あとで書く」だけの空欄を残さない
- 実装未了の機能を、完了したかのような文言で書かない
- この指示書自体が docs 同期で完結することを明示する

## テスト計画

| 種別 | 対象 | 期待 |
|------|------|------|
| **unit** | `aibe/src/adapters/outbound/tools/shell_exec.rs`（モジュール内 `#[cfg(test)]`） | `shell_exec` の timeout / kill / reap、allowlist 不一致、tool result 継続の既存契約と本文が一致する |
| **integration** | `ai/tests/tool_catalog_sync.rs`、`ai/tests/tool_names_sync.rs`、`ai/tests/ask_integration.rs`、`aibe/tests/request_tool_validation.rs`、`aibe/tests/agent_turn_loop.rs`、`aibe/tests/socket_protocol.rs`、`aibe/tests/agent_turn_tools.rs` | `@read-only` / `@exec` / `@full` の展開や safe tools の扱い、`shell_exec` の拒否と server-side enforcement の入口検証が本文の説明と一致する |
| **manual** | `docs/manual/ai-ask-tools.md` | safe tools の表示、`shell_exec` の warning、拒否 / 承認の見え方を確認できる |

### 補足

- この指示書自体は docs 同期のためのものであり、新しい runtime テストを要求しない
- ただし、本文が既存テストの場所と齟齬を起こさないことは必須である
- 参照するテスト名は実在ファイルに限定し、仮のファイル名を置かない

## docs 更新対象

- `docs/testing.md`
- `docs/security.md`
- `docs/0000_spec-index.md`
- `docs/todo/README.md`
- `docs/todo/chatgpt-review-4th-gen/p2-safe-tools.md`

## 未確定・残リスク

- `docs/manual/ai-ask-tools.md` の文言が将来変わると、`docs/testing.md` / `docs/security.md` 側も追従が必要になる
- `shell_exec` の警告文言や safe tools の表示順は、実装側の変更に合わせて再同期が必要になる
- docs 同期だけでは runtime の安全性は変わらないため、実装変更が入ると別の指示書が必要になる
- 今回は docs の同期のみであり、追加テストや挙動変更の検証は行っていない
