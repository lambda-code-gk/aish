# 0027 — `ai` コマンド UX 改善 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **起票**: 2026-06-06  
> **関連**: [architecture.md](../architecture.md)、[security.md](../security.md)、[testing.md](../testing.md)、[0019_aish-session-log-integration-spec.md](../done/0019_aish-session-log-integration-spec.md)、[0020_p4-daily-use-polish-spec.md](../done/0020_p4-daily-use-polish-spec.md)、[0021_tab-completion-spec.md](../done/0021_tab-completion-spec.md)、[0022_ai-filter-spec.md](../done/0022_ai-filter-spec.md)、[0026_external-commands-spec.md](../spec/0026_external-commands-spec.md)

## 目的

`ai` を日常利用の入口として使いやすくする。現状の `ai ask [OPTIONS] <message>` を起点に、次をまとめて導入する。

1. `aish shell` 起動時の `AI_ASK_LOG=session` 自動 export
2. `ai "..."` の default subcommand 化
3. `status` / `doctor` / `ping` の診断系導線
4. `-q` / `--quiet`、`stdin` / `-f` / `-` の入力導線
5. `--preset`、`--format json|tsv|env`、`--dry-run`
6. local history、`history`、`retry` / `rerun`
7. `--session` 省略時の `AISH_SESSION_DIR` basename 自動一致
8. `--log-tail` による shell log tail 量の調整
9. `chat` REPL、`--progress`、assistant streaming、`Ctrl+C` / `--timeout`
10. `shell_exec` 承認 UX の session 限定記憶と `--yes-exec`

本書の目的は UX を足すことだが、**aibe プロトコル変更が必要なものは明示して切り分ける**。現行の `ai` / `aibe` / `aibe-client` / `aibe-protocol` の境界を壊さず、必要なところだけ wire を拡張する。

## 非目標

- `ai` から `aibe` 本体や `aish` クレートへ path 依存を追加すること
- LLM を `ai` から直接呼ぶこと
- `aish exec` の挙動変更
- `aish shell` の session layout 変更
- `aibe` の provider 増殖や first-class conversation 永続化を Phase A/B に持ち込むこと
- `history` のローカル永続化をクラウド同期すること
- `doctor` の診断結果を自動修復まで進めること

## 現状との差分

### CLI

| 現状 | 0027 後 |
|------|--------|
| `ai ask [OPTIONS] <message>` のみ | `ai <message>` を `ask` の default 扱いにする。`ai ask` も引き続き有効 |
| `ask` の options は message より前のみ | 維持。加えて `-f/--file` と `-` で入力ソースを明示できる |
| `status` / `doctor` / `ping` / `chat` / `history` / `retry` / `rerun` は存在しない | 追加する |
| `--quiet` / `--preset` / `--format` / `--dry-run` は存在しない | 追加する |
| `--session` は手入力前提 | `AISH_SESSION_DIR` があれば basename を既定値として扱う |
| `AI_ASK_LOG=session` は手動 export 前提 | `aish shell` が child shell に自動 export する |

### 設定

| 現状 | 0027 後 |
|------|--------|
| `~/.config/ai/config.toml` の `[ask]` に `tools` / `default_profile` / `filter` | これを維持しつつ、`[presets.*]` と `history_dir`、`log_tail_bytes` を追加する |
| preset はない | `[presets.*]` で `tools` / `profile` / `filter` / `log_tail_bytes` / `quiet` / `shell_exec_approval` を束ねる |
| local history はない | `~/.local/share/ai/history/` を既定 root とする履歴ストアを追加する |

### env

| 現状 | 0027 後 |
|------|--------|
| `AI_ASK_LOG=session` は `aish` 側で自動化されない | `aish shell` が child shell に `AI_ASK_LOG=session` を自動 export する |
| `AISH_SESSION_DIR` は `aish shell` 由来のみ | 維持。`ai` はこれを `--session` 省略時の既定値に使う |
| `AI_FILTER` / `AIBE_SOCKET_PATH` / `AI_LLM_PROFILE` は既存 | 維持 |

## 共通原則

### 1. 既定 subcommand

- `ai` は root だけで `ask` を表す。
- 先頭の非 flag token が既知 subcommand ならその subcommand を優先する。
- 既知 subcommand でなければ `ask` とみなし、`message` として扱う。
- `ai ask` は明示 alias として残す。

### 2. 出力契約

- 既定の `ask` は、現状どおり stdout に assistant 本文のみを出す。
- `--format json|tsv|env` が指定された場合は、stdout を機械可読な構造に切り替える。
- stderr は診断面として扱う。
- `-q/--quiet` は stderr の非エラー診断を抑制する。

### 3. 入力ソース

`ask` / `chat` / `retry` / `rerun` は、メッセージ入力を次の優先で解決する。

1. `-f/--file <PATH>` があればそのファイル
2. `-` が単独で指定されていれば stdin
3. positional message があればそれを join した文字列
4. それも無ければ stdin が pipe のときは stdin 全文

`-f` と positional message の混在、`-` と他の message token の混在は usage error とする。

### 4. ログ・履歴・セッション

- `AISH_SESSION_DIR` があるとき、`ai` は `basename(AISH_SESSION_DIR)` を implicit `--session` とみなす。
- `--session` が明示されたときは、`AISH_SESSION_DIR` の basename と一致しない限りエラーにする。
- `AI_ASK_LOG=session` は `aish shell` からの自動 export を前提にしてよいが、ユーザーが in-shell で上書きした値は尊重する。
- `history` は redacted index を出し、再実行用 payload は別の 0600 保管領域に置く。

## 仕様

## Phase A

Phase A は、既存の `ai ask` / `aish shell` を少しだけ良くする層である。**aibe プロトコル変更なし**で完結させる。

### A-1. `aish shell` 起動時の `AI_ASK_LOG=session` 自動 export

- `aish shell` は child shell の環境に `AI_ASK_LOG=session` を注入する。
- `AISH_SESSION_DIR` は既存どおり注入する。
- `AI_ASK_LOG` は `aish shell` の外側には影響しない。
- `aish exec` には影響しない。

### A-2. `ai` default subcommand

- `ai "hello"` は `ai ask "hello"` と同義にする。
- 明示的な `ai ask` は互換のため残す。
- root の既知 subcommand は `ask`, `status`, `doctor`, `ping`, `chat`, `history`, `retry`, `rerun`, `complete` を想定する。
- typo で未知 token を打った場合は `ask` に落ちる。これは UX 上の意図した挙動だが、誤爆リスクは残る。

### A-3. `status` / `doctor`

- `ai status` は local 診断を返す canonical command とする。
- `ai doctor` は `status` の alias とし、human 向け説明を少し厚くする。
- 両者は次を診断する。
  - config の解決結果
  - preset の解決結果
  - socket path
  - `AISH_SESSION_DIR` / implicit session id
  - shell log tail 解決結果
  - aibe `ping` の成否
- `doctor` は修復はしない。提示のみ。

### A-4. `-q` / `--quiet`

- `ask` / `chat` / `retry` / `rerun` / `status` / `doctor` / `ping` / `history` に受理する。
- stderr の非エラー診断を抑制する。
- 抑制対象には、tool startup line、外部コマンド warning、shell log path の表示、`filter` 警告、progress line を含める。
- `aibe` や local validation の致命的エラーは exit code と最終 error line を維持する。

### A-5. `stdin` / `-f` / `-`

- `-f/--file` は message source をファイルに切り替える。
- `-` は stdin を明示する sentinel とする。
- stdin は、非 TTY かつ明示 source が無いときの暗黙 source としても使える。
- `stdin` の扱いは `ask` / `chat` / `retry` / `rerun` に限定する。

### A-6. `--format json|tsv|env`

- `status` / `doctor` / `ping` / `history` で必須に近い機械可読 stdout を提供する。
- `ask` / `chat` / `retry` / `rerun` でも受理し、human 向け出力と structured 出力を切り替える。
- `json` は構造化オブジェクト、`tsv` は `key\tvalue` 行、`env` は shell で `eval` 可能な `KEY='value'` 形式とする。
- `ask` の `filter` は、structured 出力でも assistant 本文に対して適用する。つまり、`assistant_message.content` が filter 後の値になる。

### A-7. `--dry-run`

- `ask` / `chat` / `retry` / `rerun` に受理する。
- aibe に接続しない。
- `aibe` に送る payload の概要を stdout に出す。
- raw message、raw shell log tail、filter コマンドの中身はマスクする。
- 代わりに、source、長さ、設定解決結果、preset 展開後の tool/profile/socket などの構造だけを表示する。
- `status` / `doctor` / `ping` / `history` には不要なので受理しない。
- `--dry-run` は `--format` と併用できる。`--format` がある場合、dry-run の概要出力もその形式で返す。
- `--dry-run` は local history に記録しない。`retry` / `rerun` の payload vault も作らない。
- `--dry-run` は `AI_ASK_LOG=session` や `--session` の解決は行うが、aibe との通信はしない。

### A-8. `ping`

- `aibe` socket の生存確認だけを行う。
- `ensure_running` は呼ばない。
- 失敗時は local connection error として扱う。
- `--format` に対応し、`json` では `alive` と `socket_path` を返す。

## Phase B

Phase B は、日常利用の核となるコマンド群を固める。**aibe プロトコル変更なし**で行えるものを先に入れる。

### B-1. `--preset`

- `--preset <NAME>` は `[presets.<NAME>]` を読み込み、`ask` / `chat` / `retry` / `rerun` の既定値にマージする。
- preset が持てる値は `tools` / `profile` / `filter` / `log_tail_bytes` / `quiet` / `shell_exec_approval` とする。
- 追加で `socket` や `format` を含めるかは将来拡張の余地として残すが、Phase B の正本は上記に限定する。
- CLI の明示値 > preset > `[ask]` 既定 > hardcoded default の順で勝つ。
- preset の複数重ねがけはやらない。

### B-2. `--session` 省略時の自動一致

- `AISH_SESSION_DIR` があるときは、`--session` 省略時に basename を implicit session id とする。
- `--session` を明示した場合は、`AISH_SESSION_DIR` の basename と一致しない限りエラーにする。
- `history` の記録でも同じ implicit session id を使う。

### B-3. `--log-tail`

- `ask` / `chat` / `retry` / `rerun` で受理する。
- 単位は bytes とする。
- `0` で tail を無効にできる。
- 上限は現行の `aibe_protocol::SHELL_LOG_TAIL_MAX_BYTES` を超えない。
- 既定値は config の `log_tail_bytes`、無ければ 16 KiB。
- これを超える値は error とする。protocol ceiling を超えた切り上げはしない。

### B-4. local history

#### ストレージ

- 既定 root は `~/.local/share/ai/history/` とする。
- `~/.config/ai/config.toml` の `history_dir` で変更できる。
- logical layout は次の 2 層に分ける。
  - `index.jsonl`: redacted な一覧・検索用メタデータ
  - `payloads/<history_id>.json`: 再実行用 payload の保管領域。0600 相当の厳しい権限で作る
- `ai history` は `index.jsonl` のみを読む。
- `retry` / `rerun` は必要に応じて payload file を読む。

#### レコード

各履歴レコードは少なくとも次を持つ。

- `history_id`
- `created_at`
- `command`
- `session_id`
- `conversation_id`
- `preset`
- `profile`
- `socket_path`
- `request_kind`
- `request_summary`
- `response_kind`
- `response_summary`
- `status`

`request_summary` と `response_summary` は redacted する。raw message や raw shell log tail は `index.jsonl` に出さない。

#### `ai history`

- 既定は最近順に一覧する。
- `--limit` は既定 20。
- `--session`、`--command`、`--status` で絞り込めるようにする。
- `json` は配列、`tsv` / `env` は 1 レコード 1 行で返す。

#### `ai retry`

- `history_id` を 1 つ受け取る。
- 直前の user message と送信時の replay payload を使って、現在の既定値で再送する。
- `retry` は「同じ内容を今の設定でやり直す」意味とする。

#### `ai rerun`

- `history_id` を 1 つ受け取る。
- 保存済み payload をそのまま再送する。
- `rerun` は「同じ request envelope をそのまま再生する」意味とする。
- `retry` よりも再現性が高いが、payload vault が失われると実行不能になる。

### B-5. `ai` 内の output filter / presets / format の関係

- `filter` は assistant 本文だけに作用する。
- `presets` は filter を含められる。
- `--format` は stdout の最終表現を決める。
- `quiet` は stderr の表示量を決める。
- この 3 つは独立し、相互に上書きしない。

## Phase C

Phase C は、プロトコルと実行中 UX を本格的に拡張する。**aibe プロトコル変更が必要**であることを明示する。

### C-1. `chat` REPL

- `ai chat` は multi-turn REPL とする。
- conversation state は **client-side** で保持し、Phase C の初版では aibe 側の永続 conversation id を導入しない。
- `conversation_id` は local history と replay のための client-side 識別子とする。
- `chat` は `agent_turn` を turn ごとに送り、前回までの transcript を client で積み上げる。
- `chat` の transcript は local history に記録するが、`aibe` に会話状態を永続保存させない。

### C-2. progress 表示

- `--progress` は stderr に「今何をしているか」を逐次出す。
- 期待する phase は `thinking` / `tool_call` / `waiting_approval` / `finalizing` / `cancelling` など。
- これは現行の one-shot `AgentTurnResult` だけでは足りないため、wire に progress event を追加する。
- progress は `--quiet` で抑制される。

### C-3. assistant streaming

- assistant token / chunk を逐次表示する。
- 現行の `ClientResponse::AgentTurnResult` だけでは足りないため、`ClientResponse` に streaming event を追加する。
- `aibe` は provider が stream 対応ならそのまま delta を流し、非対応なら buffered final reply を 1 回だけ送る。
- OpenAI-compatible provider と Gemini provider の両方で、stream 対応 / 非対応を同一の final semantics に収束させる。

### C-4. turn cancel と `--timeout`

- `Ctrl+C` は active turn を cancel する。
- `--timeout` は turn 単位の deadline とする。
- clean cancel のために、wire に cancel request を追加する。
- socket close だけの best-effort fallback は許可するが、正本は cancel request とする。
- `timeout` 到達時は cancel を送ってから exit する。

### C-5. exit code semantics

- `0`: 正常終了。`AgentTurnStatus::MaxToolRounds` もここに含める。
- `2`: usage / local validation / `invalid_request`
- `3`: transport / connect / decode / timeout / cancel
- `4`: remote `provider_error`
- `5`: remote `tool_error` / `tool_timeout` / `tool_not_allowed`
- `130`: `Ctrl+C`

`max_tool_rounds` は non-fatal warning とし、exit code を 0 に保つ。  
`invalid_request` と transport failure を同じ失敗として扱わない。

### C-6. `shell_exec` 承認 UX

- `--yes-exec` は explicit opt-in とする。
- これは session 限定の自動承認記憶を有効化するフラグであり、config default にはしない。
- 記憶のスコープは current `AISH_SESSION_DIR` か、`AISH_SESSION_DIR` が無いときは current `ai` process に限る。
- `--yes-exec` は `shell_exec_approval=ask` のときだけ prompt を bypass できる。`never` は拒否を維持し、`always` はそもそも prompt を出さない。
- `--preset` で `shell_exec_approval` を与えた場合も、`--yes-exec` より `never` を優先する。
- `shell_exec` の承認 / 拒否 / 自動承認は audit に残す。
- 既存の `ShellExecApprovalPrompt` / `ShellExecApproval` 往復を基盤にしてよいが、`--yes-exec` は prompt を bypass できるようにする。

## aibe プロトコル変更が必要な項目

| 項目 | 変更要否 | 理由 |
|------|----------|------|
| `aish shell` の `AI_ASK_LOG=session` 自動 export | 変更不要 | child shell 環境注入だけで済む |
| `ai` default subcommand / `--quiet` / `--preset` / `--format` / `--dry-run` / `history` / `retry` / `rerun` | 変更不要 | client 側の CLI / local store で完結する |
| `--session` 省略時の basename 自動一致 | 変更不要 | client の path 解決だけで足りる |
| `--log-tail` | 変更不要 | client が読む tail 長を変えるだけで足りる |
| `chat` の client-side transcript | 変更不要 | 会話状態を aibe に持たせない |
| `progress` | **必要** | progress event が wire に必要 |
| assistant streaming | **必要** | delta / chunk event が wire に必要 |
| `Ctrl+C` / `--timeout` の clean cancel | **必要** | cancel request が wire に必要 |
| `--yes-exec` の session 限定記憶 | 変更不要 | client-side cache で足りる |

## クレート配置

| クレート | 責務 |
|---------|------|
| `aish` | `shell` 起動時の `AI_ASK_LOG=session` 自動 export。`aish shell` の既存 session layout は維持する |
| `ai` | default subcommand、`status` / `doctor` / `ping` / `chat` / `history` / `retry` / `rerun`、preset / format / dry-run / quiet / log-tail / replay / REPL の orchestration |
| `aibe` | progress / streaming / cancel を含む agent loop、provider からの event 化、`shell_exec` 承認応答の継続 |
| `aibe-protocol` | `Progress` / `AssistantDelta` / `CancelTurn` 等の wire DTO 追加、既存 `ClientResponse` / `ClientRequest` の拡張 |
| `aibe-client` | event stream 対応 transport、cancel 対応、`ping` / `ensure_running` の再利用 |

`ai` は `aibe` 本体に依存せず、`aibe-protocol` と `aibe-client` に閉じる。  
`aish` は `ai` / `aibe` に依存しない。

## セキュリティ

### 1. history に秘密を載せない

- `index.jsonl` には raw message や raw shell log tail を載せない。
- preview も redacted にする。
- `history_id` から再生できる payload は別の 0600 cache に置き、`ai history` の標準出力には出さない。
- manual / docs / smoke では raw payload を前提にしない。

### 2. `--dry-run` は必ずマスクする

- message body
- shell log tail
- filter command の中身
- replay payload の raw data

は表示しない。長さや source、preset 展開後の構造のみを出す。

### 3. `--yes-exec` の危険性

- `--yes-exec` は dangerous tool を黙って通すフラグである。
- preset default に入れてよいのは、ユーザーが明示的に危険性を理解した場合だけに限定する。
- `--yes-exec` を history に記録するときは、内容ではなく policy だけを記録する。

### 4. `aish shell` の自動 export

- `AI_ASK_LOG=session` の自動 export は child shell に閉じる。
- parent shell の環境や他の `aish` subcommand には広げない。

### 5. `quiet` と安全メッセージ

- `quiet` は non-actionable diagnostics を抑えるだけにする。
- 承認拒否、exit failure、invalid request、cancel、timeout は、必要な最小限の error line を残す。

## 後方互換・破壊的変更

### 互換として残すもの

- `ai ask` の明示サブコマンド
- `ask` の option-before-message ルール
- `AI_ASK_LOG=session` の既存挙動
- `AI_FILTER` / `[ask].filter`
- `AISH_SESSION_DIR` ベースの session 連携

### 破壊的変更になり得るもの

- `ai "..."` を default ask として受け付けること
- `--format` の stdout 形状変更
- `status` / `doctor` / `ping` / `history` / `retry` / `rerun` / `chat` の追加により、未知 token が ask に落ちること
- `aish shell` が `AI_ASK_LOG=session` を自動 export すること
- Phase C の progress / streaming / cancel に伴う wire 追加
- exit code の意味分けの変更

### 非破壊として扱うもの

- 既存 `aibe-client` の `ping` / `ensure_running`
- 現行の `shell_exec` 承認 wire
- `aish exec` / `aish session` / `aish complete`

## 受け入れ条件

### unit

- `ai` root が default ask と既知 subcommand を正しく分岐する
- `--quiet` が stderr の非エラー診断を抑制する
- `-f` / `-` / stdin の message source 優先順が正しい
- `--preset` が CLI > preset > config の順で解決される
- `--format json|tsv|env` の render が各 command で壊れない
- `history` の redacted index と replay payload の解決が分離されている
- `retry` と `rerun` の意味差が固定される
- `--session` 省略時に `AISH_SESSION_DIR` basename が使われる
- `--log-tail` が protocol ceiling を超えない

### integration

- `aish shell` 起動時に `AI_ASK_LOG=session` が child shell に入る
- `ai "..."` が `ai ask "..."` と同じ request を組む
- `status` / `doctor` / `ping` が aibe の起動有無を診断できる
- `dry-run` が aibe に接続しない
- `history` が local store から一覧できる
- `retry` / `rerun` が過去 record を再生できる
- `chat` が multi-turn を保てる
- `--yes-exec` が session 限定の承認記憶として働く

### smoke

- `ai ping`
- `ai status --format json`
- `ai ask --dry-run`
- `ai "hello"`
- `ai history --format tsv`
- `ai retry <history_id>`
- `ai rerun <history_id>`
- `aish shell` 内で `AI_ASK_LOG=session` が効く

### manual

- `aish shell` 内で `ai "..."` が current session log を自動参照する
- `ai status` / `ai doctor` が connection 診断を返す
- `ai ping` が socket の生存確認だけを行う
- `ai chat` が multi-turn の REPL として使える
- `Ctrl+C` が turn cancel として働く
- `--progress` が stderr に進行を出す
- `--yes-exec` の危険性と session 限定性が確認できる

## 実装フェーズ分割

**推奨は 1 本の 0027 のまま、内部を Phase A / B / C に分けること** である。  
理由は次のとおり。

1. Phase A は既存 `ai` / `aish` の延長で、独立して価値が出る
2. Phase B も local store と CLI 拡張で閉じている
3. Phase C だけが wire 変更を必要とし、そこだけを最後に切り出せば PR の境界が自然になる

したがって、設計書を `0027a` / `0027b` に分割する必要はない。  
実装の都合で PR を分けるなら、**Phase C だけ別 PR** に切るのが最も筋が良い。

## 未確定事項

- `doctor` を完全な alias にするか、`status` の human 用シノニムにするか
- `history` payload vault の GC ルール
- `--timeout` の既定値
- `--yes-exec` を `chat` のみで有効にするか、`ask` にも広げるか
- `progress` / `stream` の wire event 名とフィールド名を既存 `aibe-protocol` の命名にどう合わせるか
- provider ごとの streaming fallback を「buffered final only」にするか「synthetic delta 1 回」にするか
- exit code の最終番号を sysexits に寄せるか、現行の独自番号に寄せるか
