# 0036 — `shell_exec` 承認 UX 拡張 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **起票**: 2026-06-13  
> **関連**: [0023_shell-exec-approval-hardening-spec.md](../done/0023_shell-exec-approval-hardening-spec.md)、[0027_ai-ux-spec.md](0027_ai-ux-spec.md)、[0028_ai-ux-gap-closure-spec.md](0028_ai-ux-gap-closure-spec.md)、[0029_ai-ux-polish-spec.md](0029_ai-ux-polish-spec.md)、[0030_ai-smart-entry-spec.md](0030_ai-smart-entry-spec.md)、[architecture.md](../architecture.md)、[security.md](../security.md)、[testing.md](../testing.md)

## 目的

`ai` から `shell_exec` を使うときの承認体験を、**安全性を維持したまま** 実用的にする。

現状の `shell_exec_approval = "ask"` は、実行ごとに `Execute? [y/N]` を出すため、日常の反復操作でノイズが大きい。そこで本書は次を導入する。

1. `0030` で未実装の「2 段プロンプト」を、session 許可と記憶スコープに分離して実装可能な形へ定義する
2. `[y]es / [n]o / [a]lways-this-session / [c]ommand-only` の承認 UI を導入する
3. `read-only` / `mutating` / `destructive` の risk tier を導入し、tier ごとに自動承認可否を変える
4. `[tools.shell_exec.auto_approve_patterns]` による pattern ベース自動承認を、session 許可後のみ有効にする
5. `ai chat` では session 初回だけ session 許可を取り、その後は tier と記憶スコープで制御する
6. 既存 `--yes-exec` を温存しつつ、`never` と non-TTY fail-closed を最上位に維持する
7. 監査は既存 `ExecutedToolCall` の `approval_state` / `decision` / `approval_source` を拡張して表現し、wire を増やさない

## 背景

`0023` で `shell_exec` 承認 UI の hardening は入ったが、UI は毎回の yes/no を前提にしており、反復操作の体験が悪い。

`0028` と `0029` で `--yes-exec` と session 限定 cache は実装済みだが、cache の粒度は command+args 完全一致に寄っている。`0030` では「session shell 許可 + 同一コマンド自動承認記憶」という方向だけが残り、具体的な UX は未定義だった。

本書はその空白を埋め、危険操作の抑制と実用性の両立を図る。

## 0023 / 0027 / 0028 / 0029 / 0030 との関係

### 0023 との関係

- 非対話 stdin の fail-closed は維持する
- `command` / `args` の制御文字 escape 表示は維持する
- 承認 UI の入力経路は `ai` 側の責務のままにする

### 0027 / 0028 / 0029 との関係

- `--yes-exec` は廃止しない
- `shell_exec_approval = "never"` は最上位の拒否であり、`--yes-exec` でも越えない
- session 限定 cache の考え方は引き継ぐ
- `history` / `local history` の記録方針は変えない

### 0030 との関係

- `ai chat` の session 初回のみ session 許可を取る、という UX を具体化する
- `shell_exec` の昇格は `route_turn` ではなく `ai` の承認 UX で処理する
- `AI_SESSION_ID` / `AISH_SESSION_DIR` の共有モデルはそのまま使う

## 非目標

- `shell_exec` を safe tool 化すること
- `shell_exec` の allowlist を廃止すること
- `aibe` に first-class shell policy engine を増やすこと
- destructive tier を session 許可で自動承認すること
- `aish` に shell 承認ロジックを移すこと
- Windows 対応

## 機能仕様

### 1. 2 段プロンプトの意味

本書でいう「2 段プロンプト」は、UI を必ず 2 回出すという意味ではない。**承認判断を 2 つの独立した軸に分ける** という意味である。

1. **session shell 許可**: この session で `shell_exec` を使ってよいか
2. **記憶スコープ**: この承認をどこまで再利用するか

UI 実装は 1 回の prompt にまとめてもよいが、内部状態は分離する。

#### 内部状態

- `session_shell_allowed`: その session で shell を使ってよいか
- `remember_scope`: `none` / `exact_invocation` / `command_name`
- `tier`: `read_only` / `mutating` / `destructive`

#### 期待する挙動

- `session_shell_allowed = false` の場合は、いかなる自動承認も起きない
- `session_shell_allowed = true` でも、tier と pattern 条件を満たさないものは prompt を出す
- `remember_scope` は session 内だけ有効で、永続化しない

### 2. 承認 UI

`shell_exec_approval = "ask"` のとき、`ai` は承認 UI を次の 4 選択で出す。

- `[y]es`
- `[n]o`
- `[a]lways-this-session`
- `[c]ommand-only`

#### 意味

- `y`: 今回だけ実行する。記憶しない
- `n`: 拒否する。記憶しない
- `a`: `exact_invocation` を session 内に記憶する
- `c`: `command_name` を session 内に記憶する。ただし再利用は同一 tier 内に限り、`mutating` / `destructive` へ横断しない

#### 補足

- `a` は `command + args` の完全一致を session 内に再利用する
- `c` は command 名を鍵にするが、再利用時は tier を再評価し、`read_only` 以外は prompt を維持する
- どちらも destructive tier には適用しない
- UI 文言は `0023` の escape 表示を維持し、raw ANSI を見せない

### 3. risk tier

`shell_exec` は次の 3 tier に分類する。

| tier | 例 | 承認方針 |
|------|----|---------|
| `read_only` | `ls`, `git status`, `git diff` | session 許可後は原則自動承認。pattern でも可 |
| `mutating` | `git add`, `cargo test`, `sed -i` 相当, 生成物更新 | session 許可後でも初回は prompt、記憶または pattern がある場合のみ自動承認 |
| `destructive` | `rm -rf`, `git push --force`, `git reset --hard` 相当 | 毎回 prompt。session 許可、記憶、pattern のいずれでも自動承認しない |

#### 分類原則

- 分類は `command` と `args` の structured argv で行う
- shell string の見た目では判定しない
- `git` のように同じ command でも subcommand で tier が変わる
- 出力先がローカル生成でも、build script や proc macro を実行し得るコマンドは read_only に入れない
- destructive 判定は保守的に行い、曖昧なら上位 tier に倒す
- `cargo check` / `cargo test` のように build script や proc macro を実行し得るコマンドは、既定では `mutating` に倒す。read_only 扱いにする場合は、プロジェクト単位で明示的に安全性を確認した allowlist を別途必要とする

### 4. pattern ベース auto-approve

`[tools.shell_exec.auto_approve_patterns]` を導入し、session 許可後のみ pattern ベース自動承認を有効にする。

#### ルール

- pattern は session 許可が先に成立している場合だけ評価する
- pattern は structured argv に対して評価する
- shell 文字列そのものではなく、`command` と `args` の正規化済み表現に対して評価する
- destructive tier には pattern を適用しない
- pattern に一致した場合のみ `approval_source` に pattern 名を残す
- 正規化は client / server で共通化した canonical form を使い、正規化に失敗した場合は自動承認しない

#### 設定の考え方

本書では構文の細部は固定しないが、少なくとも次の 2 系統を持つ。

- `read_only`
- `mutating`

`read_only` は read-only tier の自動承認候補、`mutating` は mutating tier の自動承認候補として扱う。

### 5. `ai chat` の挙動

`ai chat` では、session 初回の `shell_exec` にだけ session 許可の判断を要求する。

#### 仕様

- `chat` の session 内で最初に `shell_exec` が必要になったとき、session shell 許可を求める
- 許可後は tier に従って扱う
- read-only は session 許可後に自動承認できる
- mutating は session 許可後でも初回は prompt を維持し、`a` / `c` / pattern がある場合にのみ再利用する
- destructive は常に prompt を出す

#### session の境界

- `ai` が既存の `AISH_SESSION_DIR` を持つ場合は、その session スコープを使う
- `AISH_SESSION_DIR` が無い場合は process ローカルの session スコープに落とす
- session 許可は user の明示操作でのみ付与される

### 6. `--yes-exec` との関係

`--yes-exec` は session 限定の承認記憶を有効にする既存フラグとして残す。

#### 優先順位

1. `shell_exec_approval = "never"`
2. non-TTY fail-closed
3. destructive tier の毎回 prompt
4. CLI 明示値 / preset / `--yes-exec`
5. session 許可と記憶スコープ
6. pattern ベース auto-approve

#### 仕様

- `never` は最上位であり、`--yes-exec` でも越えない
- `--yes-exec` は `ask` でのみ有効
- `always` は prompt を出さないので `--yes-exec` の有無は意味を持たない
- non-TTY は read する前に deny する
- destructive tier は `--yes-exec` のキャッシュ対象外

### 7. aibe プロトコル変更の要否

**結論: 新しい top-level wire DTO は不要だが、`ShellExecApproval` の wire には `approval_origin` を追加する必要がある。**

理由:

- 承認の主体は `ai` の UI と session cache
- `aibe` は既存の `shell_exec` 実行と監査だけを担えばよい
- ただし `aibe` が `approval_source=...` を正しく記録するには、`ai` が「user / session cache / pattern」のどれで承認したかを 1 bit 以上の追加情報として送る必要がある
- `approval_origin` を `ShellExecApproval` に追加すれば、`ExecutedToolCall` の audit 文字列は server 側で一貫して生成できる
- pattern も session cache も client-side の policy で完結するが、audit provenance は server へ伝播しないと追跡できない

必要なのは、`ExecutedToolCall` の `approval_source` 文字列を server 側で統一生成できるようにするための最小限の wire 追加であり、承認結果そのものを新しい top-level DTO に分離することではない。

### 8. 監査

`tool_calls` には少なくとも次を残す。

- `risk_class`
- `approval_state`
- `decision`
- `approval_source`
- `approval_origin`（`ShellExecApproval` 側の wire 追加。server が `approval_source` を再構成するために使う）

#### 期待する `approval_source`

- `shell_exec_approval=never`
- `shell_exec_approval=ask;ui=y`
- `shell_exec_approval=ask;ui=a;scope=exact_invocation`
- `shell_exec_approval=ask;ui=c;scope=command_name`
- `shell_exec_approval=ask;cache=session`
- `shell_exec_approval=ask;pattern=read_only`
- `shell_exec_approval=ask;pattern=mutating`

#### `decision` の考え方

- `executed`
- `rejected_by_user`
- `rejected_by_policy`
- `rejected_by_tier`
- `auto_approved_session`
- `auto_approved_pattern`
- `approval_unavailable`

`decision` は短く、検索しやすい固定語彙に寄せる。

## セキュリティ不変条件

1. `never` は最上位の拒否であり、`--yes-exec` でも越えない
2. non-TTY は fail-closed で、prompt を出す前に拒否する
3. destructive tier は毎回 prompt であり、自動承認しない
4. pattern ベース auto-approve は session 許可後のみ有効
5. session 許可と記憶は session スコープから外に漏らさない
6. `aibe` は承認 UI を持たず、`ai` の判断を信用しすぎない。最終的な allowlist と実行制御は server 側で保持する
7. 監査に残る文字列は、表示安全化済みの command/args 由来に限る

## 非目標の再確認

- `shell_exec` を無条件に自動実行すること
- session 許可を永続化すること
- destructive tier の例外を作ること
- `shell_exec` の出力や引数をより多く表示すること
- `aibe` を UI レイヤーにすること

## 受け入れ条件

### 機能

1. `shell_exec_approval = "ask"` で `[y]es / [n]o / [a]lways-this-session / [c]ommand-only` が動く
2. read-only tier は session 許可後に自動承認される
3. mutating tier は session 許可後でも初回 prompt を維持できる
4. destructive tier は毎回 prompt される
5. pattern ベース auto-approve は session 許可後のみ有効である
6. `--yes-exec` は `never` を越えない
7. non-TTY は fail-closed である
8. `ai chat` は session 初回のみ session 許可を要求する
9. `tool_calls` に `approval_state` / `decision` / `approval_source` が残る

### 変更範囲

1. 新しい top-level aibe wire DTO を追加しない
2. `aibe-protocol` の変更は `ShellExecApproval` の provenance 追加と `ExecutedToolCall` の audit 文字列拡張に留める
3. `./scripts/verify.sh` が通ること

## テスト方針

| 種別 | 内容 | 正本 |
|------|------|------|
| unit | tier classifier、pattern matcher、session cache decision、audit 文字列生成 | `ai` / `aibe-protocol` |
| integration | `ai` の承認 UI と `shell_exec` 実行前分岐 | `ai/tests/*` |
| integration | `ai chat` の session 初回だけ session 許可を求める経路 | `ai/tests/*` |
| integration | `--yes-exec` の `never` 優先と non-TTY fail-closed | `ai/tests/yes_exec_integration.rs` 近傍 |
| unit | `ShellExecApproval` の `approval_origin` serialize/deserialize | `aibe-protocol` |
| unit | `ExecutedToolCall` の audit roundtrip | `aibe-protocol` |
| manual | 端末上で `y/n/a/c` と tier の見え方を確認 | `docs/manual/ai-ask-tools.md` |

## 実装フェーズ

### Phase 1: UI + session cache

- `y/n/a/c` の UI を追加する
- session 許可の状態を session-local に保持する
- 既存 `YesExecCache` を拡張して `exact_invocation` と `command_name` を扱えるようにする
- non-TTY fail-closed を維持する

### Phase 2: tier

- `read_only` / `mutating` / `destructive` の classifier を導入する
- destructive は毎回 prompt にする
- mutating は記憶と pattern の条件を満たした場合だけ自動承認する

### Phase 3: pattern config

- `[tools.shell_exec.auto_approve_patterns]` を導入する
- pattern は session 許可後のみ評価する
- audit に pattern 名を残す

## 未確定・推測・指示外

- `auto_approve_patterns` の TOML 構文は本書で厳密には固定していない。**推測**としては、`read_only` / `mutating` の 2 系統配列にするのが最も実装しやすい。
- `a` と `c` の内部キャッシュキーの厳密な表現は本書では固定していない。**推測**としては、`a` を command+args 完全一致、`c` を command 名 + tier とするのが自然である。
- `destructive` の境界は保守的に倒す前提だが、`git` 系や `cargo` 系の細分類は実装時に追加調整が必要になる可能性がある。

## 残リスク

- 手動検証では、TTY と non-TTY を切り替えて fail-closed を確認する必要がある
- `pattern` は便利だが、広くしすぎると誤承認の温床になる
- audit 文字列が増えるため、ログ検索の正規化を後続実装で意識する必要がある
