# セキュリティ

優先順位 1 の要件。実装・設定・ログの変更時はこの文書と `docs/architecture.md` を見直す。

## 脅威モデル（簡易）

| 脅威 | 影響 | 主な対策 |
|------|------|----------|
| API キー漏洩 | 課金・データ流出 | キーは aibe 設定のみ、ログ・git 禁止 |
| シェルログ漏洩 | コマンド履歴・パスワード露出 | マスク方針、ファイル権限、コンテキスト最小化 |
| ai が LLM を直叩き | キー分散・監査不能 | 依存チェック + コードレビュー |
| aish がネットワーク | 想定外のデータ送信 | aish に HTTP クライアントを入れない |
| デーモン socket | ローカル任意コード実行相当 | socket パス・パーミッション |
| aibe PID file / control CLI（0046） | ローカルユーザーの daemon 停止・再起動 | PID file は `~/.local/share/aibe/run.pid`（0600）。`stop` / `restart` は PID file 検証後に SIGTERM。remote control なし |
| 外部コマンド（CLI coding agent 含む） | ユーザー権限で任意コマンド・ファイル変更 | 設定の `command` のみ起動、`cwd` は `context.cwd`、allowlist、承認、監査、自動 git なし |

### CLI サブエージェント（0024 / 0025, 非採用）

- API キーは aibe 設定に置かず、**ホストにインストール済みの Codex / Claude Code CLI** のログイン契約に委ねる、という考え方自体は 0024 / 0025 の旧案として残す。
- ただし本番採用はしない。`artifacts` や thread 共有は AISH の正本ではない。
- CLI を first-class provider / tool にしないため、`command --version` のような存在確認を aibe の起動要件にはしない。

### 外部コマンド（0026）

- `[[external_commands]]` は `shell_exec` のテンプレートであり、AISH のポリシー外の CLI を明示登録するためのもの。
- `risk_class` は `DangerousShell` のまま扱う。`approval_state` は既存の `shell_exec_approval` に従う。
- `shell_exec_approval` の最終解決は CLI > preset > `AIBE_CONFIG`（`[tools.shell_exec].shell_exec_approval`）の順で、`--yes-exec` は実効 mode が `ask` の場合にのみ有効にする。`never` をクライアント側で上書きしてはいけない。
- `ai` 側の `shell_exec` 承認は `y / n / a / c` を使い、`approval_origin` で provenance を `aibe` に渡す。`session_shell_allowed=false` の間は cache / pattern の自動承認を許さない。
- AISH は CLI の内部 sandbox / login / tool catalog / thread を検証しない。ユーザー責任で管理する。

## 秘密情報

### 置いてよい場所

- **aibe 設定ファイル**（例: `~/.config/aibe/config.toml`）の API キー
- OS の環境変数（aibe 起動プロセスのみが読む。`ai` / `aish` からは読まない設計）

### 置いてはいけない場所

- Git リポジトリ（`.env`, `config.toml` 実体）
- `aish` ログ（平文キー、Bearer トークン）
- `ai` の設定・バイナリ埋め込み
- ソースコード内の文字列リテラル

### 例示ファイル

- `*.example.toml` や `docs/` には **プレースホルダ** のみ（`YOUR_API_KEY` 等）

## ログとコンテキスト

### aish ログ

- 記録してよい: コマンド行、stdout/stderr、終了コード、時刻、作業ディレクトリ
- 記録しない / マスクする:
  - 環境変数の値（特に `*KEY*`, `*TOKEN*`, `*SECRET*`)
  - パスワードプロンプト直後の入力
  - `.env` ファイルの内容をそのまま

### 実装済みマスク（aish）

`aish` はログ追記前に `sanitize_log_text` を通す（`aish-replay` crate、経由して `aish` の domain から re-export）。

| 適用先 | タイミング |
|--------|------------|
| `command_start` の `command` / 各 `args` | `LogEvent::command_start` 生成時（`exec` / `shell` 共通） |
| `stdout` / `stderr` の `data` | `ExecuteAndRecord` / PTY 追記時 |

| パターン | 置換 |
|---------|------|
| `sk-...`（OpenAI 形式） | `sk-[REDACTED]` |
| `Bearer ...` | `Bearer [REDACTED]` |
| `AIza...`（Google API キー形式） | `AIza[REDACTED]` |
| 環境変数名に `KEY` / `TOKEN` / `SECRET` を含む `NAME=value` | `NAME=[REDACTED]` |

`command_start` は `LogEvent::command_start` 経由でのみ安全化する。`CommandStart` の enum 直構築は使わない。

完全な秘匿ではない。パスワードプロンプト直後の入力などは今後拡張する。

### aish command output replay（`0049`）

- replay は **記録済みの redacted ログ**をそのまま出す。replay 時に `sanitize_log_text` を再適用しない（秘匿情報の復元経路を増やさない）
- `aish shell` の control channel はセッション dir 内 FIFO（`control.fifo`、0600）を使う。hook は継承 FD ではなく emit ごとの open-write-close とし、`ls` 等の子プロセスが control へ書けないようにする
- `AISH_SESSION_DIR/current_log` は canonicalize 後に session dir 配下であることを検証し、symlink escape を拒否する
- `log.jsonl` は 0600 相当で作成・追記する（既存方針の維持）

### ai → aibe の context

- 渡すのは **必要最小限** のログ tail（上限は `aibe::ShellLogTail::MAX_BYTES`。ai はこの定数のみ参照しリテラル直書きしない）
- `context.system_instruction` は `aibe_protocol::SYSTEM_INSTRUCTION_MAX_BYTES` で上限管理し、`ai` / `aibe` の両方で長すぎる値を切り詰める
- `cwd` はツール有効時に `ai` のカレントディレクトリ（絶対パス）を送り、`read_file` / `shell_exec` の相対パス解決に使う。**未送信・相対パスは aibe が `invalid_request` で拒否する**（aibe プロセス cwd へのフォールバックはしない）
- ユーザーが明示しない限り、全セッション履歴を一度に送らない設計を推奨
- `AI_SESSION_ID` は権限キーではなく会話共有キーとして扱う。`aish` が export するか、`aish` 外では `ai` が生成する。
- `route_turn` の `route_reason` は path を mask し、短く redaction した上で stderr / history / store に残す。raw のパスや shell コマンドをそのまま保存しない。

### ai local history

- `ai history` の `index.jsonl` には raw message / raw shell log tail / 秘密情報を載せない
- replay 用 payload vault は `payloads/<history_id>.json` に分離し、0600 相当の権限で保存する
- `history_id` から復元できない情報は index に置かず、再送に必要な最小限だけを vault に閉じ込める

### aibe conversation store

- `AI_SESSION_ID` ごとに `~/.local/share/aibe/conversations/<session_id>/` を分離する
- `index.jsonl` は redacted metadata のみで、full transcript は `conversations/<conversation_id>.json` に閉じる
- store 配下は 0700 / 0600 相当で作成し、他ユーザーから読めない前提を維持する

### contextual memory（0034 + 0035）

- memory はユーザーが明示保存した**背景文脈**であり、system instruction や shell コマンドとして扱わない
- **owner は `memory_space_id`**。`AI_SESSION_ID` は provenance のみで、shell log と同じ寿命で memory を消さない
- `AIBE_CONTEXT_ID` は **クライアント `ai` の context selection のみ**（サーバ `aibe` は読まない）。`ai context` で選んだ名前は path-safe な `memory_space_id` として扱う（raw path を directory 名にしない。`.` / `..` など dot のみの ID は traversal 防止のため拒否）。`session_id` も同様に path-safe を強制する。
- `aibe` が `AgentTurn` 時にのみ注入する。`ai` は wire 経由で apply/query するだけで、ローカル正本を持たない
- `idea` は on-demand のみ。**通常クエリへ常時注入しない**（明示要求・関連判定・`ai mem show` 時のみ）
- `now` は別 session から見ると stale 表示されうる（古い作業状況の誤注入を抑える）
- legacy data の lazy copy は元の session store を上書きしない
- API キー・トークンなど機密を memory に保存しない運用とする（自動 secret 検出は MVP 外）
- `--dry-run` 等で memory 全文を不用意に露出しない（`ai` memory コマンドは意図的な表示のみ）
- **capability 分離（0037 Phase 6）**: memory 操作（read/write/archive/recipe/subscribe）と shell execute は AIBE application boundary で別 capability。shell 承認 UI とは独立。v1 は local runtime のみで **remote authentication / token issue は未実装**。将来 mobile profile は shell execute を持たない設計（[manual/contextual-memory-multi-client.md](manual/contextual-memory-multi-client.md)）

### ai work（0052 Phase 3）

- Work state は `aibe` が `memory/spaces/<memory_space_id>/work-state.json` に保存し、`ai` はローカル正本を持たない。
- memory space directory / state file は `0700 / 0600`。generic memory と同じ `.lock` を使い、temp file + `sync_all` + rename で置換する。
- `memory_space_id` は既存 validator / resolver 経由だけで path に変換し、Work ID は正の整数として path component に使用しない。
- Work operation の user text は空文字・NUL・8 KiB 超過を protocol / service / persisted-state validation で拒否する。
- 壊れた JSON、未知 schema、state invariant 違反は既存 state を上書きしない。explicit Work RPC は本文や実 path を含めない分類済み error を返す。
- `push` は active を stack に積み child work を作る。`pop` は child を `Done` にして parent へ戻し、child entries を parent へ自動 merge しない。
- `switch` は `Paused / Deferred` のみ許可し、missing / `Done` / `Abandoned` / stack 非空は state を変えずに拒否する。
- `finish` は stack が空のときだけ active を `Done` にして unset し、stack 非空では拒否する。
- Work 内容はユーザーが明示保存する文脈であり、自動 redaction はしない。API key、token、password を goal / note / idea 等へ保存しない。
- Work read/write は既存 `MemoryRead / MemoryWrite` capability に従い、runtime disabled / feature-off は fail-closed とする。
- Phase 1 / Phase 2 / Phase 3 の mutation は space lock内の単一 read-modify-write で適用し、operation error 時は state を保存しない。
- active Work の通常 turn 注入は実装済み。Work block は synthetic user context として bounded に注入し、system instruction にはしない。Work 内容は自動 redaction しない。

### Smart Preprocessor observation report

- ai smart stats/recent/report は observation を read-only で扱い、読み取り DTO に宣言した既知の非 raw フィールドだけを出力する。未知フィールド、不正 JSON、不正 UTF-8 の内容をエラー本文や report に再掲しない。
- TSV / ENV / Markdown ではタブ・改行を単一行へ正規化する。report は分類精度の正解データではなく、安全な運用メトリクスの共有物として扱う。

### safe tools / dangerous tools

設計の上位正本: [architecture.md](architecture.md)。検証の所在: [testing.md](testing.md) の「0018 safe-tools-policy」。

| 区分 | ツール例 | 許可の考え方 |
|------|----------|--------------|
| **safe** | `read_file`, `list_dir`, `grep`, `git_diff`, `git_status` | `@read-only` / `@full` で既定有効。読み取り目的で `shell_exec` を使わない |
| **dangerous** | `shell_exec` | `@exec` または literal 指定が **明示** された場合のみ。それ以外は aibe が拒否する。実行前承認は aibe 設定 `[tools.shell_exec] shell_exec_approval`（`never` / `ask` / `always`、既定 `ask`）。`ask` は tier（read_only / mutating / destructive）と session 許可に応じて prompt または自動承認（0036） |
| **write-like** | `write_file`, `apply_patch` | `@edit` または literal 指定が **明示** された場合のみ。Smart Preprocessor / `@full` からは自動追加しない。`file:write` capability 必須。実行前承認は `[tools.file_write] approval`（`never` / `ask` / `always`、既定 `ask`）。`ask` では AIBE が実 diff を生成し `ai` が stderr で承認 UI を出す（0054）。non-TTY / socket disconnect は fail-closed |

- **client 側（`ai`）**: 起動時に有効ツールを `stderr` で表示。`shell_exec` / `write_file` / `apply_patch` 有効時は warning。`shell_exec_approval=ask` では実行直前に **stderr** で `Execute? [y/n/a/c]`（0036）。自動承認（session 記憶 / pattern / read_only tier / `--yes-exec` cache）時は **stderr** に `ai: shell_exec auto-approved (...): <command>` を **既定で** 出す。`--silent-exec` または `--quiet` で抑止。`shell_exec_approval=always` は承認 prompt なしだが turn 終了時に `ai: shell_exec executed (...): <command>` を stderr に出す（同上、抑止可）。`y` は今回のみ、`a` は同一 invocation を session 内記憶、`c` は command 名（同一 tier 内）を session 内記憶。`read_only` tier は session 許可後に自動承認可。`destructive` tier は毎回 prompt。`--yes-exec` は session 限定 cache を有効化（`ask` のみ、`never` は越えない）。`file_write` approval=`ask` では **stderr** で diff preview と `Apply this change? [y/N]`（0054）。write tool に session cache はない。stdin が TTY でない場合は **読む前に拒否**（shell / write 共通）。`session_shell_allowed=false` の間は cache / pattern を使わない。`command` / `args` / diff 内制御文字は escape 表示（0023 / 0054）。
- **server 側（`aibe`）**: allowlist 外・`shell_exec_approval=never` / `file_write` approval=`never`・ユーザー拒否は tool result（`status=error`）で LLM に返し turn 継続可能。client warning の有無に依存しない。write tool は SHA-256 楽観排他・journal 失敗時 write なし・raw content/patch を監査に残さない（0054）。
- **監査**: `tool_calls` に `risk_class` / `approval_state` / `approval_source` / `decision` を載せる。`ShellExecApproval` / `ToolApproval` wire の `approval_origin` を server が `approval_source` に再構成する（0036 / 0054）。write-like は `file_write_approval=<mode>[;ui=y|n]`、`decision` は `executed` / `rejected_by_user` / `approval_unavailable` / `rejected_or_failed` / `no_change` 等。
- **pattern auto-approve**: `[tools.shell_exec.auto_approve_patterns]` は session 許可後のみ評価。`destructive` tier には適用しない。正規化失敗時は fail-closed。

### `ai` の表示メタデータ

- `dry-run` / `status` / `doctor` は raw な `[ask].filter` や `AI_FILTER` の値を出さず、`enabled` / `source` / `masked` のメタデータだけを返す。assistant 本文のフィルタ文字列そのものはログや診断出力に残さない。
- manual や logs に残す説明は、本番設定や秘密情報を含めない。

### ai のツール表示

- `ai ask` の `stdout` は最終 `assistant_message.content` のみに保つ。
- `--verbose-tools` で出す `tool_calls` 詳細は `stderr` に限定する。
- `tool_calls` の引数や出力には秘密情報が含まれうるため、共有端末や記録されたログでは注意する。
- `stderr` の通知文は補助情報であり、ツール実行の監査ログの代替ではない。

### ai の output filter（`AI_FILTER` / `[ask].filter`）

- filter は **ユーザー端末上**で `/bin/sh -c` として実行される。assistant 本文が stdin に渡る。
- filter 文字列はユーザー自身が設定する。**任意のシェルコマンド**として扱い、設定ファイルや env に信頼できない第三者の入力を置かない。
- filter の stdout はターミナルにそのまま出る。機密を含む応答を加工する場合も、シェル履歴・ログへの露出に注意する。
- filter stderr はユーザー stderr に透過する。filter がハングすると `ai ask` も終了しない（タイムアウトなし）。
- `aish` は filter を export しない。`aish shell` 内で使う場合はユーザーが明示的に `export AI_FILTER=...` する。

### ai local history

- `index.jsonl` は redacted メタデータのみを置く。raw message、raw shell log tail、秘密情報を入れない
- `payloads/<history_id>.json` は replay 用 payload vault。`0600` 相当で保存し、他ユーザーから読めない前提を維持する
- `retry` / `rerun` の実行時に payload の生値を stderr に再掲しない

## 権限・プロセス

- **Unix 専用**: ファイルモード・ソケットパスは umask / `chmod` を意識する
- aibe の socket: ユーザー私用ディレクトリ配下。`bind` 時は umask `077` + `chmod 600`（foreground / デーモン共通）
- aish が起動するシェルは **ユーザー自身の権限** で動く。エージェントのツール実行はその権限を継承する — 高権限シェルでは aibe を動かさない運用を推奨
- **外部コマンドのタイムアウト**（`shell_exec` / `git_diff` / `git_status` / `git rev-parse`）: aibe は timeout 時に子プロセスへ `start_kill()` を送り、**明示 `wait()` で reap** する（共通 `run_subprocess`）。`kill_on_drop(true)` は補助的な保険であり、明示 kill/reap の代替ではない
- **`list_dir` / `grep`**: `[tools.explore]` の件数・走査上限で timeout 前のメモリ・I/O を抑制する（`docs/aibe.config.example.toml`）

## 依存関係

- `./scripts/check-architecture.sh` で禁止クレート依存を検出
- 新規 crate 追加時は LLM/HTTP 系が **aibe 以外** に入っていないか確認

## インシデント時

1. 漏洩疑いのキーはプロバイダ側で **即ローテーション**
2. ログファイル・設定のバックアップを確認し、共有範囲を切る
3. 再発防止を `docs/security.md` に追記

## レビューチェックリスト（変更時）

- [ ] 秘密が git / ログ / テスト fixture に入っていないか
- [ ] aish にネットワーク依存が増えていないか
- [ ] ai が LLM URL に直接つながないか
- [ ] 手動検証手順に本番キーを書いていないか
# Collaborative Mode Human Task（0062）

- `human_task` は Collaborative Mode だけで公開し、aibe 側も `execution_mode` を検査して forged allowlist を `tool_not_allowed` とする。
- request は未知 field、空 objective/配列要素、型不一致、正規化後 64 KiB 超を拒否する。
- 同一接続 callback は turn ID、tool call ID、prompt ID の全一致を要求し、不一致・重複・decode 失敗・待機 call 不在を別 call へ回送しない。
- `AISH_HANDOFF_TASK_JSON` は 64 KiB 以下の既知 version だけを受理し、子 shell から unset する。JSON 本文、shell log、生の端末入力を通常 log/error に含めない。
- `done` は人間から制御が戻った事実だけであり、criteria 達成や自動検証済みを意味しない。

## Human Task checkpoint（0063）

- checkpointは元ユーザー要求、task、Observation、suspend reasonを含み得る機微データである。許可fieldを表示する`ai human-task status`以外のlog/error/固定応答へ本文を複製しない。
- `SuspendTurn`でクライアントへ返すtool audit recordはhuman_taskの引数と結果本文を除去し、`--verbose-tools`や構造化出力へobjective、reason、Observationを流さない。
- `ai human-task status`の許可fieldもcontrol characterをescapeしてから表示し、保存済みobjectiveやcwdによるterminal controlを許さない。
- 保存先は`<history_dir>/human-tasks/<validated-task-id>/checkpoint.json`だけとし、directoryは作成時から0700、fileは作成時から0600、current UID、component symlink、`O_NOFOLLOW`、encode/decode 1 MiB上限をfail-closedで検査する。削除時もtask directoryとcheckpointのsymlink/owner/modeを再検査する。
- atomic更新はsame-directory temp、file fsync、rename、directory fsyncで行い、破損JSON、未知version、invariant違反、owner/mode不正を自動削除・自動修復しない。
- `<history_dir>/human-tasks/lock`は0600/current UID/symlink拒否で開き、create/resumeはactive確認からHuman Shell終端後の最終save/removeまで`flock(LOCK_EX)`を保持する。status/cancelは非ブロッキング`LOCK_EX|LOCK_NB`で取り、busy時はstatusがactive runningをbest-effort表示、cancelは`human_task_checkpoint_busy`で失敗する。これはleaseやcrash ownershipではない。
- cancelはroot lock取得後のSuspended、orphaned Running、ResultPending、stale Continuing、またはdelete失敗後のFinishedだけを削除する。`--yes`なしの非TTY・拒否・入力失敗、およびinvalid checkpointでは削除しない。有効task ID directoryのcheckpoint欠落やtemp残骸はInvalidとして保持し、aibeへagent continuationやCancelled tool resultを送らない。
- `human-task suspend`のreasonはUTF-8 4096 bytes以下かつ全Unicode control characterなし。shellはJSONを組み立てず、Rust helperがversion 1 eventを生成する。validation/control送信失敗時はshellを継続し、成功表示やSuspended checkpointを作らない。
- 0063/0064はcreate/status/cancel/resumeのroot flockによる同一ユーザー・同一ホスト上のprocess間直列化を保証する。lease、stale ownership判定、crash recovery、schema migration、複数user/host競合は保証しない。
- 0065 continuation は checkpoint の元要求・task・final_result（Evidenceを含む）を新しい aibe agent turn の入力として送るため、選択された LLM provider と conversation/history 保存の機微データ境界に入る。API key、環境変数、socket、approval cache、raw shell logはcheckpointにも継続messageにも含めない。本文やEvidenceをstderr/error/statusへ表示せず、statusはobjective/cwdと状態だけをescapeして表示する。
- continuation中は同じroot flockを保持し、別ai processの二重開始を防ぐ。aibeは`continuation_turn=true`のactive IDと`AgentTurnStatus::Ok`完了IDをprocess memoryだけで拒否する。MaxToolRoundsは完了扱いしない。永続dedup正本、aibe再起動跨ぎexactly-once、Continuingの自動crash recoveryは提供しない。
- 0066の`recover`はroot flock取得とユーザー確認後だけRunning / Continuingを既存の復帰可能状態へ保存する。PID、elapsed time、checkpoint本文からowner消失を推測せず、active ownerがlockを保持中ならbusyで無変更にする。
- invalid residueはstatus診断だけでは削除せず、`recover --force-invalid`と確認（または明示`--yes`）の組合せを要求する。force cleanupはvalidated task IDの単一entryを`.removing/`へquarantineし、`openat(O_PATH|O_NOFOLLOW)`でinodeを固定してから子file/symlinkだけを`unlinkat`する。共有path名に対する`AT_REMOVEDIR`はTOCTOUのため行わず、空のquarantine directoryが残っても差し替えdirectoryは削除しない。symlinkをfollowせず、checkpoint root外へ再帰せず、所有外directory / nested directory / 複数entryをfail-closedで拒否する。本文を表示・logせず、schema migrationやJSON修復は行わない。
