# 0067 Recall Keybinding TTY Restore 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定（実装済み）  
> **起票**: 2026-07-18  
> **関連**: GitHub Issue #11、[`0053_ai-suggested-command-recall-spec.md`](0053_ai-suggested-command-recall-spec.md)、[`0055_minimal-human-handoff-spec.md`](0055_minimal-human-handoff-spec.md)、[`docs/feature-development-policy.md`](../feature-development-policy.md)、[`docs/testing.md`](../testing.md)

## 0. Core outcome

ユーザーが bash / zsh の対話 prompt で `Alt+.` / `Alt+,` により候補を挿入した後も、カーソル移動と履歴移動を通常どおり継続できる。

## 1. Minimum vertical slice

```text
bash / zsh の対話 prompt
→ 既存 0053 hook で Alt+. または Alt+, を入力
→ ai recall subprocess が候補を返す
→ 既存 line editor buffer と cursor を更新して再表示
→ ← / → で候補内を移動
→ ↑ / ↓ で shell history を移動
```

候補なし、`ai recall` 失敗、shortcut の連続入力でも、同じ終端条件（line editor が次のキー入力を解釈できる状態）へ戻る。`aish` Human Shell の suggested-command binding は別の候補供給元を持つ 0055 統合だが、同じキーと line editor API の回帰面なので同一 spec に含める。

## 2. Fault model

### 2.1 保証対象

標準 Fault Model に従う。単一ホスト・単一ユーザーの生存中の対話 bash / zsh で、shortcut widget の成功、候補なし、同期的な処理失敗のいずれからも line editor 入力状態へ戻ることを保証する。PTY 試験では shortcut 送信直前と widget 完了後に prompt が再び安定した時点を観測境界とし、完全な CSI cursor sequence が shell に解釈されること、および `tcgetattr` の `c_iflag` / `c_oflag` / `c_cflag` / `c_lflag` / `c_cc` が境界間で一致することを確認する。shell がキー読取中に行う一時的な raw/cbreak 切替は比較対象にしない。

### 2.2 保証対象外

- shell process、terminal emulator、`ai recall` process のクラッシュ後の自動復旧
- SSH 切断、terminal emulator の強制終了、壊れたユーザー rcfile の修復
- bash / zsh 以外の line editor
- OS や terminal emulator 固有の Meta key encoding の差を吸収する新しい key protocol
- 0053 cache の同時更新、永続形式、候補巡回規則の変更

## 3. Non-goals

- 0053 suggested-command recall の抽出、cache、巡回 UX の再設計
- shortcut、候補挿入先、既存 `Alt+.` / `Alt+,` の意味の変更
- 新しい実行主体、状態機械、永続 aggregate、crash recovery、lease、reconciler、schema migration、secondary agent loop
- reedline 内蔵 prompt editor への recall keybinding 追加（本件は shell の readline / ZLE が対象）
- terminal 全体へ無条件に `stty sane` を適用すること（ユーザー設定を破壊し得るため採用しない）
- PTY 基盤の作り直し

## 4. Complexity inventory

| 項目 | 数・内容 |
|------|----------|
| 実行主体 | 1（既存の対話 shell / line editor） |
| 状態機械 | 0 |
| 永続 aggregate | 0 |
| 外部副作用 | 1（既存 prompt buffer / cursor の更新） |
| プロセス境界 | 1（既存 shell → `ai recall` subprocess） |
| 新規基盤機構 | 0（既存 hook の lifecycle hardening と PTY 回帰試験のみ） |
| 他機能統合 | 2（0053 recall shell hooks、0055 Human Shell handoff recall binding） |

## 5. Complexity Gate

- 判定: **Yellow**
- 理由: 新規 actor、状態機械、永続 aggregate、novel mechanism はなく、既存 shell hook と一つの subprocess 境界の終了契約を修正する。一方で 0053 recall shell hooks と 0055 Human Shell handoff recall binding の 2 統合を同時に回帰させるため integrations が Yellow 閾値に達する。Red 要因はすべて `false` である
- 分割判断: 0053 の cache / UX 再設計、PTY 基盤変更、terminal emulator 固有対応は本 spec から分離する。handoff binding は同じキー・同じ line editor API の回帰なので本 spec に含める
- scope review: **approved** — handoff を別 spec にすると同一キー・同一 line editor API の修正が重複し、binding 上書き回帰を見落とすため、2 統合を一つの回帰マトリクスとして扱う
- 承認例外: 不要（Red ではない）

## 6. Complexity budget

新規実行主体 +0、状態機械 +0、永続 aggregate +0、external effect +0、process boundary +0、novel mechanism +0、integration +0（0053 / 0055 の既存 2 統合を上限とする）。新しい常駐 process、設定、protocol、永続データは追加しない。

## 7. Split triggers

次のいずれかが必要になった時点で STOP-THE-LINE とし、0067 へ追加しない。

- `ai recall` 以外の subprocess、daemon、agent loop の追加
- recall cache schema または巡回状態の変更
- line editor 状態を表す新しい状態機械や永続 snapshot
- PTY relay / handoff lifecycle 自体の変更
- 0053 / 0055 以外の機能統合、または第三の recall binding 実装
- terminal emulator 別 key decoding、key timeout、汎用 ESC parser
- crash recovery、lease / heartbeat、reconciler、journal、schema migration、exactly-once

## 8. パック構成の適用

**No** — 0045 §6 の適用候補に該当しない。これは既存 `ai` / `aish` の bash / zsh shell adapter に閉じる軽量な不具合修正であり、core service への横断割り込み、専用 RPC / CLI / turn hook 群、重い optional 依存、別 profile は追加しない。また `aish` は Pack Composition の対象外である。Active / Basic Pack、composition root、runtime toggle、Cargo feature は追加しない。

## 9. Acceptance Criteria

| ID | 条件 |
|----|------|
| `recall_keybinding_pty_vertical_e2e` | CI の実 PTY 上で bash / zsh × `Alt+.` / `Alt+,` をマトリクス実行する。成功・候補なし・cache 不在・`ai recall` 非 0・混在連続入力の各 labeled subcase で、buffer の非破壊、左右 cursor、上下 history、stable prompt 境界の termios 一致を確認する |
| `recall_subprocess_cannot_read_widget_input` | PTY を使わない shell 統合試験で、stub `ai recall` が stdin を読もうとしても EOF となり、成功・空・非 0 の全 return path が shell function 内で完結する |
| `handoff_recall_preserves_line_editor_navigation` | CI の実 PTY 上で 0055 Human Shell の bash / zsh を実行し、handoff 候補の `Alt+.` / `Alt+,` 挿入後に左右 cursor と上下 history が機能し、stable prompt 境界の termios が一致する |
| `recall_hooks_avoid_unobserved_terminal_reset` | 生成される 0053 / 0055 hook が `stty sane`、固定 termios、無条件 reset を含まず、製品コードでの限定復元は再現試験が示した属性だけを shortcut 開始時の観測値へ戻す |

### 9.1 テストレベルと CI 契約

| AC | レベル | CI 前提 |
|----|--------|---------|
| `recall_keybinding_pty_vertical_e2e` | 実 PTY E2E（1 test function 内の labeled matrix） | Ubuntu verify job が必須化済みの bash / zsh。外部 terminal emulator、実 API、ユーザー rcfile は使わない |
| `recall_subprocess_cannot_read_widget_input` | shell process 統合（non-PTY） | PATH 上の deterministic stub `ai` と一時 cache を使う |
| `handoff_recall_preserves_line_editor_navigation` | 既存 0055 PTY helper を再利用する回帰 E2E | 新しい PTY 基盤を作らず、bash / zsh の両方を実行する |
| `recall_hooks_avoid_unobserved_terminal_reset` | 生成 hook の契約試験 | 文字列検査は reset 禁止の補助ゲートに限定し、navigation 達成の代用にしない |

### 9.2 Shell 別の終了契約

| Shell | buffer / cursor | 再表示 | subprocess 境界 |
|-------|-----------------|--------|--------------------|
| bash | `READLINE_LINE` / `READLINE_POINT` を成功かつ非空候補の場合だけ更新する | `bind -x` が readline へ制御を返した後の redisplay を壊さない | recall subprocess に widget の入力ストリームを消費させず、成功・空・失敗を shell function 内で完結させる |
| zsh | `BUFFER` / `CURSOR` を成功かつ非空候補の場合だけ更新する | ZLE widget として明示的に redisplay し、widget local option を外へ漏らさない | recall subprocess に ZLE 入力を消費させず、すべての分岐で widget return する |

recall subprocess の stdin は `/dev/null` 相当へ分離し、stdout の候補だけを取得する。termios の保存・復元を製品コードへ追加できるのは、stable prompt 境界の反復可能な PTY 試験で「製品コード起因の属性差分」が特定された場合だけとする。その場合も差分が出た属性だけを shortcut 開始時に `tcgetattr` で観測した値へ戻し、`stty sane`、固定値、全属性の無条件上書きは行わない。差分を再現できない場合は stdin 分離と shell / ZLE の終了契約だけを修正する。

### 9.3 0053 / 0055 binding の所有と優先関係

- 通常の bash / zsh では 0053 の `ai complete` hook が `Alt+.` / `Alt+,` を所有し、候補巡回、cache、挿入のみという 0053 の意味論を変更しない
- `_AISH_HUMAN_SHELL=1` かつ handoff 候補がある 0055 Human Shell では、rcfile 後段の handoff binding が同じキーを意図的に所有する。候補は固定 handoff suggestion であり、0053 cache の巡回へ統合しない
- Human Shell 外、または handoff 候補がない場合は handoff binding を install せず、0053 binding を上書きしない

## 10. Deferred specs

- bash / zsh 以外の shell と line editor
- terminal emulator 固有の Meta / ESC timeout 対応
- 0053 cache / candidate navigation の仕様変更
- reedline への suggested-command recall 統合

## 11. Scope change log

| Revision | 分類 | 変更 | 理由 |
|----------|------|------|------|
| 1 | INITIAL | Issue #11 を既存 bash / zsh recall widget の入力状態復元と PTY 回帰試験に限定 | 0053 の機能再設計や新規状態管理を避け、観測された line editor regression だけを修正するため |
| 2 | BLOCKER_ORIGINAL_AC / SAFETY_WITHIN_FAULT_MODEL / REGRESSION | 0053 + 0055 の 2 統合を Yellow として承認し、PTY AC を2本へ集約、stdin 分離・termios 観測境界・binding 優先関係を固定 | AC の CI 実行可能性と観測根拠なしの terminal reset 禁止をテスト可能な契約にするため |

## 12. 根因仮説と調査方針

### 12.1 コードから確認できる事実

- `ai/src/adapters/outbound/shell_completion.rs` の bash hook は `bind -x` 内から `ai recall next|prev` を command substitution で同期実行し、非空時だけ `READLINE_LINE` / `READLINE_POINT` を更新する
- zsh hook は ZLE widget 内から同じ subprocess を同期実行し、`BUFFER` / `CURSOR` 更新後に `zle reset-prompt` を呼ぶ
- 候補なし、cache 不在、subprocess 失敗は早期 return するが、TTY 属性、keymap、次の CSI 入力を確認する自動試験はない
- `aish/src/adapters/outbound/shell_completion.rs` の handoff hook は同じ `Alt+.` / `Alt+,` を独自 widget へ束縛し、source / install 順によって 0053 binding を上書きし得る
- binding は `\e.` / `\e,` の完全列であり、矢印キーの一般的な CSI `\e[D` / `\e[C` / `\e[A` / `\e[B` を直接束縛してはいない
- bare `ai` の reedline editor はこの shell widget 経路では起動しないため、reedline 自体は直接原因ではない

### 12.2 優先仮説

1. **推測（第一仮説）**: line editor widget から起動した `ai recall` subprocess が widget の stdin / controlling TTY を継承しているため、外部 process 境界の前後で入力状態を保つ契約が不足している。subprocess stdin の分離と前後の termios / keymap 観測で検証する。
2. **推測（第二仮説）**: zsh の `zle reset-prompt`、または widget local scope / return path が通常 redisplay へ戻すには不適切である。`zle -R` を含む ZLE 推奨の redisplay と比較する。
3. **推測（第三仮説）**: `ai complete` と Human Shell wrapper の同一キー binding が install 順で上書きされ、想定と異なる widget が動作している。PTY 内で `bind -S` / `bindkey` と実行 widget を観測して切り分ける。
4. **推測（切り分け対象）**: ESC sequence の部分消費または termios 残留。現行 binding 文字列だけでは CSI 干渉を裏付けられないため、shortcut 直後に送った完全な cursor sequence と `tcgetattr` 前後差分を PTY で記録し、事実が得られた場合だけ修正対象にする。

### 12.3 実装方針

1. まず既存 PTY helper を使う回帰試験で bash / zsh の shortcut 成功・空・失敗・連続操作を再現し、buffer、cursor、history、stable prompt 境界の termios、実際の binding を観測する。
2. recall subprocess が line editor 入力を読めない境界にし、成功かつ非空の場合だけ buffer / cursor を更新する。全 return path で shell / ZLE へ制御を返す。
3. bash は readline 管理値だけ、zsh は ZLE 管理値だけを更新し、観測根拠なしに terminal 全体を reset しない。
4. `ai` hook と handoff hook に同じ lifecycle 契約を適用する。候補供給元の違いは維持する。
5. PTY 試験を主回帰とし、生成 script の文字列検査だけで AC 達成とはしない。
