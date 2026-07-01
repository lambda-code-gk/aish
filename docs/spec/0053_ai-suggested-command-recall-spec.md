# 0053 — `ai` 提案コマンド再呼び出し 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **起票**: 2026-07-01  
> **関連**: [0027_ai-ux-spec.md](0027_ai-ux-spec.md)、[0047_ai-interactive-prompt-input-spec.md](0047_ai-interactive-prompt-input-spec.md)、[0049_aish-command-output-replay-spec.md](0049_aish-command-output-replay-spec.md)、[0050_client-provided-replay-tool-spec.md](0050_client-provided-replay-tool-spec.md)、[0045_pack-composition-spec.md](0045_pack-composition-spec.md)、[architecture.md](../architecture.md)、[security.md](../security.md)、[testing.md](../testing.md)

## 0. 目的

`ai` が assistant によって提案した shell コマンドを、shell prompt 上で「直近の提案をもう一度呼び出す」体験として再利用できるようにする。

狙いは次のとおり。

1. `ai` の final assistant message から shell コマンド候補を抽出する
2. 抽出した候補を、bash / zsh の prompt から再挿入できるようにする
3. `aish shell` 経由でも、通常の bash / zsh へ `ai complete` を eval した経路でも、同じ recall UX を使えるようにする
4. recall は **再実行ではなく prompt への挿入** に限定し、ユーザーが編集してから実行できるようにする
5. `aibe` には触らず、`ai` が書き、`aish` が shell hook を配線する

この機能は「assistant が提案した `git commit -m ...` を、履歴検索に近い感覚で prompt 上に戻す」ためのものである。

## 1. 非目標

- `aibe` の wire protocol 変更
- `aibe` 側での提案コマンド保存
- `fish` / `tcsh` / `nushell` など bash / zsh 以外の対応
- recall を自動実行すること
- recall 候補を `Ctrl+R` の history search に直接混ぜることを v1 の正本にすること
- `aish` に LLM / assistant の責務を持ち込むこと
- assistant が提案していない任意の history を復元すること

## 2. パック構成の適用

**部分適用**

理由は 2 点ある。

1. 本機能は optional な shell UX ではあるが、`aibe` 本体や `aish` 本体を pack として差し替える話ではない。`aibe` は触らず、`ai` の recall 抽出と `aish` の shell hook 配線を通常の boundary でつなぐのが自然である。
2. ただし `ai` 側には recall manifest を読む / 書く optional boundary があり、`aish` 側にも bash / zsh hook の有効化・無効化があるため、境界だけを部分的に分けるのは妥当である。`aibe` まで含めた共通 pack は不要で、composition root を 1 か所に集約する恩恵も小さい。

したがって、本機能は **Pack Composition の完全採用ではなく、client-side / shell-side のみの部分適用** とする。

## 3. 現状と課題

### 3.1 `ai` 側

- `ai` は assistant の final content を stdout に出せる
- `--quiet` と `--format json|tsv|env` は既存である
- しかし、assistant が出した shell コマンド候補を prompt に戻すための正規の保存先がない
- つまり、`ai` は「提案する」ことはできても「提案を再呼び出しする」状態を持っていない

### 3.2 `aish` 側

- `aish shell` は bash / zsh 用の一時 rcfile を作れる
- 既存の `aish/src/adapters/outbound/shell_completion.rs` には、`ai complete` を eval する注入経路がある
- ただし、この注入は completion 中心であり、提案コマンド recall の keybinding や cache 読み込みは未定義である

### 3.3 bash / zsh の既存 UX

- bash / zsh は `READLINE_LINE` / `BUFFER` で prompt buffer を直接書き換えられる
- `history -s` で history に追加する手段もあるが、未実行の提案を persistent history に混ぜる副作用がある
- v1 は prompt への挿入を正とし、history 汚染は避けるべきである

## 4. 仕様

### 4.1 データフロー

本機能の正本は次の 5 段階である。

1. `ai` が turn 終了時の assistant content を受け取る
2. `ai` が fenced code block を走査して shell command 候補を抽出する
3. `ai` が候補を per-shell cache に保存する
4. `aish shell` または `eval "$(ai complete bash|zsh)"` が、prompt hook と keybinding をインストールする
5. ユーザーが `Alt+.` / `Alt+,` を押すと、shell hook が `ai recall next` / `ai recall prev` 経由で cache から候補を読み、prompt buffer に挿入する

この recall は **表示の再生ではなく、prompt buffer への挿入** である。実行はユーザーが Enter したときだけ起きる。shell hook は JSON cache を直接解釈せず、`ai recall next` / `ai recall prev` を呼ぶ。

### 4.2 抽出ルール

assistant content からの抽出は、final message のみを対象にする。streaming chunk からは抽出しない。

#### 4.2.1 受理する block

- triple backtick fenced code block を対象とする
- 言語タグが `bash` / `sh` / `zsh` / `shell` の block を shell candidate として受理する
- 言語タグが複数ある場合は、上記 shell tag が含まれるときだけ受理する
- unlabeled block は v1 では原則として受理しない

#### 4.2.2 正規化

候補は次の順で正規化する。

1. 先頭・末尾の空行を 1 つだけ除去する
2. 行頭の uniform な prompt prefix（`$ ` / `# ` / `> `）が全行に揃っている場合だけ落とす
3. 行末の trailing newline は 1 つだけ落とす
4. NUL を含む候補は破棄する
5. 制御文字や ANSI escape sequence は cache 保存前に除去する

#### 4.2.3 複数行

- 複数行 block は 1 つの候補として保持する
- heredoc、line continuation、`&&` 連結などは、assistant が出した通りの行構造を保つ
- 候補を勝手に split しない

#### 4.2.4 サイズ上限

- 1 候補あたりの上限は 8 KiB とする
- 上限超過候補は破棄し、`--quiet` でなければ stderr に短い理由を出す

### 4.3 キュー / 巡回 UX

#### 4.3.1 キューの単位

- 1 回の assistant turn で抽出した候補列を 1 つの queue とする
- queue は turn の出現順を保持する
- 新しい turn の queue が届いたら、その queue が active になる

#### 4.3.2 巡回（前進 / 逆戻り / ラップ）

巡回の対象は **直近の `ai` turn が生成した active queue のみ** とする。古い turn の queue は cache に残るが、キーバインドによる巡回対象には含めない。

| 操作 | キー | CLI | 動作 |
|------|------|-----|------|
| 次の候補 | `Alt+.` | `ai recall next` | 候補列を前方向へ 1 つ進め、該当テキストを prompt に挿入する |
| 前の候補 | `Alt+,` | `ai recall prev` | 候補列を後方向へ 1 つ戻し、該当テキストを prompt に挿入する |

ラップアラウンド:

- `Alt+.` で末尾候補の次を押すと、先頭候補に戻る
- `Alt+,` で先頭候補の前を押すと、末尾候補に戻る
- 候補が 1 個だけのときは、どちらのキーでも同じコマンドを挿入する

カーソルモデル:

- cache は `active_candidate_index`（次に `Alt+.` で挿入する index）と `recall_navigated`（いずれかの巡回キーが使われたか）を保持する
- 新しい turn の queue が `append` されたら、`active_candidate_index = 0`、`recall_navigated = false` に reset する
- `ai recall next` / `ai recall prev` は cache を読み書きし、stdout に挿入用テキストのみを返す

**v1 で採用しないもの**:

- active queue 消費後の古い turn への fallback（直近 turn 内ラップに置き換えた）
- ユーザーが prompt を手編集したときの自動 reset（ラップアラウンドで代替。将来必要なら別途検討）

#### 4.3.3 複数提案の扱い

- assistant が 3 個の shell block を出したら、3 個とも queue に入れる
- 初回の `Alt+.` は 1 個目の候補を挿入する
- 2 回目以降の `Alt+.` は 2 個目 → 3 個目 → 1 個目…とラップする
- `Alt+,` は逆順にラップする（初回は末尾候補から始まる）

### 4.4 キーバインド

#### 4.4.1 推奨

**`Alt+.` を前進、`Alt+,` を逆戻りの primary にする。**

理由:

- bash / zsh の Meta 系バインドとして自然である
- `.` / `,` がキーボード上で隣にあり、「提案コマンドを前後にたどる」操作として対称的である
- prompt へ **挿入** するだけで、execute 系の意味を持たない
- `READLINE_LINE` / `BUFFER` と相性が良い

実装:

- bash: `bind -x '"\e.": "_ai_recall_next"'` / `bind -x '"\e,": "_ai_recall_prev"'`
- zsh: `bindkey '\e.' _ai_recall_next` / `bindkey '\e,' _ai_recall_prev`
- 各関数は `ai recall next` / `ai recall prev` の stdout を `READLINE_LINE` / `BUFFER` へ入れる

#### 4.4.2 非推奨候補との比較

- `Ctrl+O`
  - execute / operate 系の既存意味とぶつかりやすい
  - recall の意図より「実行」へ見えやすいので v1 の primary にはしない
- `Alt+Enter`
  - `ai` 自身の editor 系 UX と紛らわしい
  - terminal によっては入力が不安定である
- `Ctrl+R` / `↑`
  - shell history search としては既存の期待値が強い
  - recall 候補を history に混ぜない限り、そのまま primary recall にするのは不自然である

#### 4.4.3 history 統合との比較

- `history -s` を primary にすると、未実行の提案コマンドが persistent history に入り、`Ctrl+R` の結果が汚れる
- `READLINE_LINE` / `BUFFER` への直接挿入なら、提案は prompt にだけ現れ、実行後の history は通常どおり shell が記録する
- v1 は **`READLINE_LINE` / `BUFFER` を正** とし、`history -s` は採用しない

### 4.5 `aish shell` と通常 bash / zsh の差分

#### 4.5.1 `aish shell`

- `aish shell` は rcfile 注入を通じて、bash / zsh へ recall hook を自動で入れる
- `aish shell` がやるのは bootstrap だけであり、recall の正本ロジックは `ai` に置く
- 既存の `AI_ASK_LOG=session` 注入と同じく、shell への配線を担当する

#### 4.5.2 通常 bash / zsh

- ユーザーが `eval "$(ai complete bash)"` または `eval "$(ai complete zsh)"` を行うと、同じ hook が入る
- `aish shell` 由来か手動 eval かで、hook の動作は変えない
- hook は idempotent であるべきで、複数回 source しても重複 binding を作らない
- bash では `trap` と `PROMPT_COMMAND`、zsh では `preexec_functions` / `precmd_functions` の登録重複を避けることを正とする
- `ai complete` が既存の completion script を出す責務と recall hook を配る責務を同じ出力経路で担う場合、hook 部分は completion 登録と副作用が衝突しないように末尾へ追加する

#### 4.5.3 bash / zsh 以外

- `fish` / その他は v1 非対象
- hook は no-op か未提供とし、main の `ai` 実行を壊さない

### 4.6 設定

`~/.config/ai/config.toml` に recall 用の設定を追加する。

```toml
[ask]
suggested_command_recall = true
suggested_command_recall_hint = true
suggested_command_recall_max_items = 8
suggested_command_recall_mirror_history = false
```

意味は次のとおり。

- `suggested_command_recall`
  - 抽出と cache 書き込みを有効化する
- `suggested_command_recall_hint`
  - stderr の短い案内文を出すかを決める
- `suggested_command_recall_max_items`
  - 1 turn あたりの保持候補数上限
- `suggested_command_recall_mirror_history`
  - `history -s` へのミラーを将来追加するための予約

v1 では `mirror_history = false` を正とし、history へのミラーはしない。

### 4.7 env

shell bootstrap が次の env を export する。

- `AI_SUGGESTION_CACHE`
  - per-shell の suggestion cache のパス
- `AI_SUGGESTED_COMMAND_RECALL`
  - `1` で recall を有効化、`0` で抑止
- `AI_SUGGESTED_COMMAND_RECALL_HINT`
  - `1` で stderr hint を出す

優先順位は次のとおり。

1. shell bootstrap が export した env
2. `~/.config/ai/config.toml`
3. hardcoded default

### 4.8 TTY 条件 / quiet / format

#### 4.8.1 TTY

- recall は interactive shell 前提である
- `stdin` / `stdout` / `stderr` のいずれかが non-TTY なら、cache 書き込みと hint は抑止する
- non-TTY では prompt hook は動かないので、recall は fail-closed にする
- ただし `ai complete` のような hook 生成コマンド自体は non-TTY であっても従来どおり completion script を出力できることを壊さない

#### 4.8.2 `--quiet`

- `--quiet` は stderr hint を抑止する
- ただし、interactive TTY であり `suggested_command_recall` が有効なら、cache 書き込み自体は維持する
- つまり quiet は「案内を黙らせる」だけで、「次回 recall の材料」を消さない

#### 4.8.3 `--format json|tsv|env`

- structured output 時は recall を無効化する
- 理由は、`--format` が automation 面を向いており、prompt UX を副作用として持ち込むべきでないためである
- したがって `--format` 指定時は hint も cache も出さない

### 4.9 stderr hint

recall が有効で候補が 1 個以上ある場合、`ai` は stderr に短い案内を出す。

例:

```text
ai: 3 suggested commands ready. Alt+. / Alt+, cycle proposals.
```

方針:

- hint は 1 行で短くする
- hint は `--quiet` で消す
- hint は `--format` で消す
- hint には shell command 本文をそのまま重ねない

### 4.10 クレート境界

| クレート | 責務 |
|---------|------|
| **ai** | assistant content からの候補抽出、cache 書き込み、stderr hint、`ai recall next` / `ai recall prev`、TTY / quiet / format 判定 |
| **aish** | rcfile 注入、shell hook の配線、`Alt+.` / `Alt+,` の binding、cache path の export |
| **aibe** | 変更なし |

禁止:

- `aibe` が提案候補を直接保存すること
- `ai` が shell hook を自分で実行すること
- `aish` が assistant content を解釈すること

## 5. 受け入れ条件

| AC | 内容 | テスト関数名案 | pending |
|----|------|----------------|---------|
| AC-01 | `ai` が final assistant content から bash / zsh fenced block を抽出し、cache に保存する | `extract_shell_candidates_from_fenced_code_blocks` | false |
| AC-02 | 複数 block が 1 turn にあっても queue 順を維持する | `preserve_suggested_command_queue_order_across_multiple_fences` | false |
| AC-03 | bash で `Alt+.` が `READLINE_LINE` に候補を挿入し、history を汚さない | `bash_alt_period_inserts_suggested_command_into_readline_line` | false |
| AC-04 | zsh で `Alt+.` が `BUFFER` に候補を挿入し、history を汚さない | `zsh_alt_period_inserts_suggested_command_into_buffer` | false |
| AC-05 | `aish shell` と `ai complete` の両方で同じ hook が入る | `aish_shell_and_ai_complete_install_the_same_recall_hook` | false |
| AC-06 | `--quiet` は hint を抑止するが、TTY では recall cache を維持する | `quiet_mode_suppresses_hint_without_disabling_recall_cache` | false |
| AC-07 | `--format json|tsv|env` は hint / cache を無効化する | `structured_output_disables_suggested_command_recall` | false |
| AC-08 | 非 TTY では recall が fail-closed になる | `non_tty_disables_suggested_command_recall` | false |
| AC-09 | unsupported shell では hook が入らないが、`ai` の通常実行は壊れない | `unsupported_shells_do_not_install_recall_hook` | false |
| AC-10 | 抽出候補の制御文字 / NUL / oversize が安全に拒否される | `reject_control_char_nul_and_oversized_suggested_commands` | false |
| AC-11 | `Alt+.` で直近 turn 内の候補が末尾から先頭へラップする | `recall_next_command_wraps_after_last_candidate` | false |
| AC-12 | `Alt+,`（`ai recall prev`）で直近 turn 内の候補が先頭から末尾へラップする | `recall_prev_command_wraps_before_first_candidate` | false |
| AC-13 | bash / zsh hook が `Alt+,` を `ai recall prev` に結ぶ | `bash_alt_period_inserts_suggested_command_into_readline_line` / `ai_complete_zsh_includes_recall_hook` | false |

## 6. セキュリティ

- recall 候補は assistant が出した **未信頼テキスト** として扱う
- recall は自動実行しない。prompt への挿入までで止める
- shell history に未実行候補をミラーしない。`history -s` は v1 で使わない
- cache は session-scoped かつ 0600 相当で保存する
- cache に assistant prose をそのまま残さず、抽出した shell candidate のみを保存する
- `NUL`、制御文字、ANSI escape sequence は cache へ入れる前に落とす
- `--format` の structured output と recall を混ぜない。automation 経路へ副作用を持ち込まない
- `aibe` には提案候補を送らない。`ai` と `aish` のローカル完結に留める

## 7. テスト方針

### 7.1 単体

- fenced code block の抽出
- shell language tag の受理 / 非受理
- prompt prefix の strip
- multiline candidate の保持
- queue の前進 / 逆戻り / ラップ（直近 turn 内）
- oversize / NUL / control char の破棄
- `quiet` / `format` / non-TTY の判定

### 7.2 統合

- bash の `READLINE_LINE` への挿入
- zsh の `BUFFER` への挿入
- `Alt+.` / `Alt+,` の keybinding
- `ai recall next` / `ai recall prev` の CLI 導通
- `aish shell` rcfile 注入で hook が有効になること
- `ai complete bash|zsh` を eval した通常 shell でも同じ hook が有効になること

### 7.3 manual

- `docs/manual/ai-ux.md` へ、提案コマンド recall の手動確認手順を追加する前提とする
- 具体的には、`ai` 実行後に stderr hint が出ること、`Alt+.` / `Alt+,` で prompt に候補が戻ること、末尾 / 先頭でラップすること、`--quiet` と `--format` で挙動が変わることを確認する

## 8. 未確定事項

### 推測

- `AI_SUGGESTION_CACHE` の具体的なファイル名は、実装時に `aish` の session dir / temp dir 規則へ寄せる必要がある
- `history -s` の opt-in ミラーは v1 では採用しないが、将来の拡張候補として残す余地はある
- 手編集開始時の巡回 reset は v1 では採用しない（ラップアラウンドで代替）
