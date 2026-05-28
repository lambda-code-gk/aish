# 0018 — 安全なツール体系と `shell_exec` 明示承認の正式指示書 — 仕様ドラフト

> **出典**: `docs/todo/chatgpt-review-4th-gen/p2-safe-tools.md`、`concerns.md` §4、`implementation-order.md` Sprint 3。  
> **状態**: **実装反映済み（v1）**

## 目的

`aish` ワークスペースでは、LLM が日常的に使う経路を `shell_exec` へ寄せるのではなく、まず **安全な読み取り系ツール** で完結できる範囲を広げる。`shell_exec` は例外経路として残すが、**明示指定・承認・監査** を前提にして、読み取り目的では使わせない。

同時に、将来の書き込み系ツールに対しては **dry-run → 承認 → 実行** の順を必須化する。ここで定めるのは、単なる UI の警告ではなく、`ai` と `aibe` の両方で強制できる運用契約である。

## 背景

現状の `ai` は `@read-only` / `@exec` / `@full` のカテゴリでツールを解決し、`shell_exec` が有効なときは stderr に警告を出す。`aibe` 側は `shell_exec` と `read_file` を持つが、`read-only` に相当するツールはまだ `read_file` 中心で、`list_dir`、`grep`、`git_diff`、`git_status` のような安全な代替が不足している。

その結果、次の問題がある。

1. 本来はファイル読み取りや差分確認で足りる作業でも、LLM が `shell_exec` を選びやすい。
2. `shell_exec` を有効化すると、実質的に任意コード実行に近づく。
3. 危険な操作に対する dry-run / approval / audit の契約が、クレート境界に沿っていない。

## スコープ

### 対象

- `aibe-protocol` の tool 名・実行結果メタデータの拡張
- `aibe` の domain/application/adapters における安全な読み取り系ツールの追加
- `aibe` の tool 実行ガード、dry-run / approval / audit の共通化
- `ai` のツールカテゴリ解決、`shell_exec` の明示承認導線、stderr 表示
- `docs/architecture.md`、`docs/testing.md`、`docs/security.md`、`docs/manual/` の更新
- `docs/0000_spec-index.md` と `docs/todo/README.md` の状態同期

### 非対象

- `aish` のシェル実行・ログ収集の機能拡張
- `aibe-client` の transport 変更
- LLM プロバイダの追加・変更
- `shell_exec` の allowlist 設定 UI 変更
- 書き込み系ツール本体（`write_file`、`replace_file`、`apply_patch`）の実装
- プロセスグループ kill など、`shell_exec` の子孫プロセス制御の再設計

## 確定した設計判断

| 項目 | 方針 |
|------|------|
| **読み取り系の優先** | `read_file` に加えて `list_dir`、`grep`、`git_diff`、`git_status` を安全な標準ツールとして追加する。読み取り目的では `shell_exec` を使わない。 |
| **`@read-only`** | `read_file`、`list_dir`、`grep`、`git_diff`、`git_status` を展開する。`shell_exec` は含めない。 |
| **`@exec`** | `shell_exec` のみを展開する。これ以外のツールは含めない。 |
| **`@full`** | 「利用可能な安全ツールの全体」を意味し、`shell_exec` は含めない。`shell_exec` は `@full` では有効化しない。 |
| **`shell_exec` の扱い** | ユーザーが `shell_exec` または `@exec` を明示した場合のみ候補に入る。`ai` は毎回警告し、`aibe` は受信した `allowed_tools`（クライアント allowlist）に `shell_exec` がない要求を拒否する。 |
| **承認の強制点** | v1 では `allowed_tools`（クライアント allowlist）を承認ソースとして `aibe` が server-side で再検証する。将来 token 化する場合も server-side enforcement を維持する。 |
| **dry-run** | 書き込み系ツールは、実行前に dry-run の差分/計画を返し、その結果に対する承認がない限り実行しない。今回は write ツール本体は追加しないが、共通契約とテストは用意する。 |
| **監査** | 危険度の高い tool request について、`requested_tools`、`risk_class`、`decision`、`approval_source`、`dry_run` の有無が追跡できること。 |
| **server-side enforcement** | `ai` の確認漏れや別クライアントからの直送を防ぐため、`aibe` 側でも policy を持つ。 |
| **read-only tools の実装責務** | `aibe` 側の adapter が持つ。`ai` は名前解決と表示、`aibe` は実行。 |

## 実装タスク

### 1. `aibe-protocol` / domain

- `ToolName` と `KNOWN_TOOLS` に安全ツールの名前を追加する。
- tool risk を表す型を追加する。少なくとも `read_only`、`dangerous_shell`、`write_like` を区別できること。
- `ExecutedToolCall` に、監査で使える最小限のメタデータを追加する。少なくとも `risk_class`、`approval_state`、`dry_run` の有無を表現できること。
- 既存の JSON 形を壊さない範囲で serde roundtrip を更新する。

### 2. `aibe` application

- `application/tool_defs.rs` に新しい読み取り系ツールの定義を追加する。
- `application/tool_round/executor.rs` の tool dispatch で、`tool_name` ごとの risk を参照できるようにする。
- dangerous tool の実行前に policy を問い合わせる共通経路を追加する。
- approval が必要な tool については、v1 では `allowed_tools` を承認ソースとして `aibe` 側で拒否/許可を判断し、`ai` 側の警告は補助に留める。
- audit sink を application boundary に用意し、dangerous request の決定を記録する。

### 3. `aibe` adapters

- `adapters/outbound/tools/` に `list_dir`、`grep`、`git_diff`、`git_status` を追加する。
- それぞれのツールは `ToolExecutionContext::base_dir` / `resolve_path` を使い、`aibe` の cwd を直接参照しない。
- `grep` はファイル内容の検索に限定し、外部 shell の任意実行に落ちないようにする。
- `git_diff` / `git_status` は git リポジトリ内でのみ動作し、読み取り結果だけを返す。
- `shell_exec` は adapter 内でも dangerous 扱いを明示し、policy を通さずに spawn しない。
- 監査は structured log か、少なくともテストで検証できるイベントとして残す。

### 4. `ai`

- `ai/src/domain/tools.rs` のカテゴリ展開を更新する。`@read-only` と `@full` は安全ツールのみを返し、`shell_exec` は含めない。
- `shell_exec` は `@exec` または literal 指定でしか有効化しない。
- dangerous tools を有効にする場合、`ai ask` は stderr に明示的な警告と確認要求を出す。
- 非対話モードでは、承認なしの dangerous tool request を開始しない。
- `stdout_presenter` と `--verbose-tools` の表示は、追加された監査メタデータに追従する。

### 5. docs / manual

- `docs/architecture.md` に新しい tool 群と risk/approval/dry-run の契約を追記する。
- `docs/testing.md` に unit / integration / manual の所在を追記する。
- `docs/security.md` に `shell_exec` の明示承認と、write-like の dry-run/approval 方針を追記する。
- `docs/manual/ai-ask-tools.md` に safe tools と dangerous tools の確認手順を追加する。

## 受け入れ条件

### 1. 安全な読み取り系ツールが追加される

- `@read-only` で `read_file`、`list_dir`、`grep`、`git_diff`、`git_status` が利用できる。
- これらのツールはファイル読み取り・差分表示・状態確認に限定される。
- 読み取り目的の実装に `shell_exec` を使う必要がないことを示す unit test がある。

### 2. `shell_exec` は明示指定時のみ有効化される

- `@full` では `shell_exec` が自動で入らない。
- `shell_exec` は `@exec` か literal 名の明示指定がなければ候補に入らない。
- `ai ask` は dangerous tools を有効化するとき、stderr に警告を出す。
- 承認なし（`allowed_tools` に `shell_exec` が含まれない）の dangerous tool request は、`aibe` 側の server-side enforcement で必ず拒否される。`ai` の拒否だけでは不十分である。

### 3. 監査・承認・dry-run の契約が実装される

- dangerous request には `risk_class`、`decision`、`approval_state` を追跡できる。
- `aibe` は dangerous request を server-side で再検証し、v1 では `allowed_tools` 由来の承認情報がない request を拒否する。
- write-like contract の dry-run は、実行本体とは別の結果として表現できる。
- approval を必要とする tool は、承認と実行の対応が追跡可能である。

### 4. クレート境界を守る

- `ai` は `aibe` 本体へ戻らず、必要な wire だけに依存する。
- `aibe` の tool 実装は `aibe` 内に閉じる。
- `check-architecture.sh` と `check-hexagonal.sh` が通ること。

### 5. docs とテストが同期する

- `docs/architecture.md`、`docs/testing.md`、`docs/security.md`、`docs/manual/ai-ask-tools.md` が実装と一致する。
- `docs/0000_spec-index.md` に本指示書が登録される。
- `docs/todo/README.md` の関連リンクと状態が更新される。

## テスト計画

| 種別 | 対象 | 期待 |
|------|------|------|
| **unit** | `aibe` の tool adapter、policy、audit metadata | `list_dir` / `grep` / `git_diff` / `git_status` の出力・境界・エラーが安定している。dangerous request の拒否と承認済み経路を分けて検証できる。 |
| **unit** | `ai/src/domain/tools.rs` | `@read-only`、`@exec`、`@full` の展開が安全方針どおりである。`shell_exec` が `@full` に含まれない。 |
| **integration** | `ai/tests/` と `aibe/tests/` | `ai ask --tools @read-only` で safe tools だけが有効になる。`shell_exec` を明示したケースでは警告と承認が必要になる。 |
| **integration** | `aibe/tests/` | `aibe` が server-side で dangerous request を拒否できる。approval 付き request は通る。 |
| **manual** | `docs/manual/ai-ask-tools.md` に手順化 | safe tools の表示、`shell_exec` の警告、拒否/承認の見え方を確認できる。 |

### テスト実装の要点

- `list_dir` と `git_status` は実ファイルシステム依存を最小化し、fixture ディレクトリで検証する。
- `grep` は固定 fixture に対する検索結果を検証する。
- `git_diff` は temporary git repo を使って diff の形を固定する。
- dangerous request のテストは、承認なし拒否と承認あり通過の 2 本を必ず分ける。
- `ai` のカテゴリ展開テストは、`KNOWN_TOOLS` とカテゴリ表のドリフトを防ぐ既存テストに統合する。

## docs 更新一覧

- `docs/architecture.md` — safe tools 群、risk class、approval/dry-run、`shell_exec` の明示承認
- `docs/testing.md` — unit / integration / manual の配置と実行観点
- `docs/security.md` — dangerous tool の承認、監査、dry-run 方針
- `docs/manual/ai-ask-tools.md` — safe tools と `shell_exec` の確認手順
- `docs/0000_spec-index.md` — 0018 を仕様ドラフトとして追加
- `docs/todo/README.md` — `chatgpt-review-4th-gen/` の次段階を 0018 に接続

## 未確定・見送り

- `shell_exec` の承認トークンの wire 形は、実装時に `aibe-protocol` へ追加する。命名は実装に合わせてよいが、server-side で検証できることは必須。
- `write_file`、`replace_file`、`apply_patch` の実装自体は本指示書の範囲外。次段階で dry-run 可能な tool として追加する。
- `grep` の内部 backend は `rg` でも `grep` でもよいが、対外的な tool 名と JSON 契約は固定する。

## 残リスク

- `shell_exec` を有効にした時点で、allowlist 範囲内の任意コマンド実行リスクは残る。
- `grep` / `git_diff` / `git_status` も、巨大リポジトリでは性能劣化の余地がある。
- approval の UX を `ai` 側だけに寄せると、別クライアントからの bypass が残る。server-side enforcement を省略しないこと。
