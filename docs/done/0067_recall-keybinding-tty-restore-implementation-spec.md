# 0067 Recall Keybinding TTY Restore 実装指示書

## 0. 目的

[`0067_recall-keybinding-tty-restore-spec.md`](../spec/0067_recall-keybinding-tty-restore-spec.md) を正本として、bash / zsh の `Alt+.` / `Alt+,` recall widget が候補挿入後、候補なし、cache 不在、subprocess 失敗、連続入力のすべてで shell の line editor へ制御を返し、左右 cursor と上下 history を継続できるようにする。最初に実 PTY で入力列、binding、stable prompt 境界の termios を観測して根因を確定し、観測された契約違反だけを 0053 recall hook と 0055 Human Shell handoff hook で修正する。

## 0.1 Scope Lock

- Feature scope registry: `scripts/feature-scope.toml`
- Scope revision: 2
- Complexity class: Yellow（`scope_review = "approved"`）
- Vertical slice AC ID: `recall_keybinding_pty_vertical_e2e`
- Locked AC IDs:
  - `recall_keybinding_pty_vertical_e2e`
  - `recall_subprocess_cannot_read_widget_input`
  - `handoff_recall_preserves_line_editor_navigation`
  - `recall_hooks_avoid_unobserved_terminal_reset`

locked AC は上記4件から増やさない。`BLOCKER_ORIGINAL_AC`、`REGRESSION`、`SAFETY_WITHIN_FAULT_MODEL` に該当する変更が必要でも、設計書 §11 と `scripts/feature-scope.toml` の `scope_revision` を上げ、Complexity Gate を再判定してから着手する。`NEW_REQUIREMENT`、`HARDENING`、`OUT_OF_FAULT_MODEL` は本 spec へ追加せず Deferred または別 spec とする。

## 0.2 変更対象ファイルとレイヤー

| ファイル | レイヤー / 責務 | 予定する変更 |
|----------|-----------------|--------------|
| `ai/src/adapters/outbound/shell_completion.rs` | `ai` outbound shell adapter | 0053 bash readline / zsh ZLE hook の subprocess stdin 分離、成功・空・失敗の終了契約、必要な redisplay を修正する。termios を扱う場合は PTY 観測で差分が確定した属性だけに限定する |
| `aish/src/adapters/outbound/shell_completion.rs` | `aish` outbound shell adapter | 0055 Human Shell の bash / zsh handoff recall hook に同じ line editor lifecycle 契約を適用し、handoff 候補がある場合だけ後段 binding が所有する既存優先関係を維持する |
| `ai/tests/0067_recall_keybinding_tty_restore.rs` | `ai` acceptance / shell integration / PTY E2E | 既存3件の `#[ignore]` placeholder を実試験へ置換する |
| `aish/tests/0067_recall_keybinding_tty_restore.rs` | `aish` acceptance / Human Shell PTY E2E | 既存1件の `#[ignore]` placeholder を実試験へ置換する |
| `ai/tests/0055_collaborative_handoff_vertical_e2e.rs`、`ai/tests/0057_pty_process_cleanup_hardening.rs` | 既存 PTY 試験の参照元 | `openpty`、controlling TTY、bounded wait、cleanup、termios 比較の実装パターンを踏襲する。共通化が小さく閉じない場合は既存基盤を改造しない |
| `scripts/spec-acceptance.toml` | acceptance registry | 各テストの実体が緑になった順に当該 AC の `pending` だけを解除する |
| `docs/architecture.md` | architecture 正本 | 0053 / 0055 binding 所有、subprocess stdin 境界、bash readline / zsh ZLE の終了契約を同期する |
| `docs/testing.md` | test policy | 0067 の実 PTY matrix、stable prompt / termios 観測境界、non-PTY stdin EOF 試験を追記する |
| `docs/manual/ai-ux.md` | manual smoke | recall 挿入後の cursor / history と成功・空・失敗・連続 shortcut の確認手順を追記する |
| `docs/security.md` | security | 製品コードで termios 復元を追加する場合だけ、観測値への限定復元と固定 reset 禁止を追記する。stdin 分離だけなら変更不要であることを確認する |
| `docs/0000_spec-index.md` | spec / task index | 実装中、完了時の状態と指示書の配置を同期する |

責務境界は維持する。`ai` は 0053 の候補供給 subprocess と shell hook、`aish` は 0055 handoff shell hook だけを変更し、`aish` に LLM / aibe 接続を追加しない。domain、application、protocol、永続 cache schema は変更対象外とする。

## 1. Phase 分割とゲート

`scripts/spec-acceptance.toml` 上の4 AC は Scope Lock 済みの同一 Phase 1 である。実装作業は根因を先に固定し、0053 の vertical slice が緑になる前に 0055 統合へ進まないよう、次の順に分ける。

| Phase | 内容 | ゲート / pending 解除順 |
|-------|------|-------------------------|
| 0: PTY 観測 | 既存 placeholder を観測可能な deterministic harness へ置換し、bash / zsh の binding、完全な shortcut / CSI 入力、stable prompt 前後の termios、stub subprocess の stdin を記録する。製品コードは変更しない | AC はすべて `pending = true` のまま。再現不能でも観測結果を残し、推測だけで terminal reset を追加しない |
| 1A: 0053 vertical slice | recall subprocess stdin を `/dev/null` 相当へ分離し、bash readline / zsh ZLE の全 return path と redisplay を修正する | (1) `recall_subprocess_cannot_read_widget_input`、(2) `recall_keybinding_pty_vertical_e2e`、(3) `recall_hooks_avoid_unobserved_terminal_reset` の順で、各テストから `#[ignore]` と panic を除き、単独成功後に各 `pending = false` とする。vertical slice が緑になるまで Phase 1B に進まない |
| 1B: 0055 integration | Human Shell の後段 handoff binding に同じ lifecycle 契約を適用し、候補あり / なしの所有関係と navigation を回帰させる | (4) `handoff_recall_preserves_line_editor_navigation` を緑にして `pending = false` とする。最後に4 ACをまとめて再実行する |
| 2: docs / completion | architecture、testing、manual、該当時 security を実装結果へ同期し、targeted 検証後に full verify と smoke を行う | 全4 AC が `pending = false` であること。完了後だけ本書を `docs/done/` へ移し index を「実装済み」にする |

**Vertical Slice Gate**: `recall_keybinding_pty_vertical_e2e` 成功前に、0055 以外の integration、cache / UX 再設計、PTY 基盤の共通 framework 化、terminal emulator 別 key decoder、crash recovery を実装してはならない。

## 2. 実装手順

### 2.1 根因確定のための PTY 観測

1. `ai/tests/0067_recall_keybinding_tty_restore.rs` に、既存 0055 / 0057 試験と同じ `openpty`、session / controlling TTY、bounded read / wait、子 process cleanup の形で harness を作る。外部 terminal emulator、ユーザー rcfile、実 API は使わず、一時 HOME、専用 rcfile、一時 cache、PATH 上の deterministic stub `ai` を使う。
2. bash / zsh ごとに prompt sentinel を固定し、shortcut 送信直前と widget 完了後に同じ stable prompt sentinel が観測された時点だけを境界とする。shell がキー読取中に行う一時的な raw / cbreak は比較しない。
3. shortcut は `Alt+.` (`ESC .`) / `Alt+,` (`ESC ,`)、cursor / history は完全な CSI (`ESC [ D/C/A/B`) を一まとまりで送る。出力の見た目だけでなく、cursor 位置へ marker を挿入して実行結果を読む、履歴項目を確定して読む等の shell observable で buffer / cursor / history を判定する。
4. matrix の各 subcase に `shell / shortcut / outcome` を含む label を付け、失敗時に入力 bytes、最後の bounded PTY transcript、観測 binding、termios 差分を表示する。成功、候補なし、cache 不在、`ai recall` 非0、next / prev と cursor / history の混在連続入力を1 test function内で実行する。
5. stable prompt 境界ごとに slave TTY の `tcgetattr` を取得し、`c_iflag` / `c_oflag` / `c_cflag` / `c_lflag` / `c_cc` をフィールド別に比較する。padding を含む構造体全体の byte 比較はしない。
6. bash は `bind -S`、zsh は `bindkey` と、stub が記録した実行 widget / direction を採取し、0053 と 0055 のどちらがキーを所有しているかを切り分ける。観測用ログへ環境全体、cache 内容、秘密情報を出さない。
7. non-PTY shell 統合では stub `ai recall` が stdin から1 byte読もうとし、即時 EOF を受けたことを専用ファイルへ記録する。成功かつ非空、成功かつ空、非0の各分岐で shell function が完結し、元 buffer が空 / 失敗時に非破壊であることを確認する。

### 2.2 修正の優先順

1. **subprocess stdin 境界**: 0053 bash / zsh の `ai recall next|prev` command substitution に `/dev/null` 相当を接続する。stdout の候補取得と既存 stderr 抑制は維持し、widget の入力ストリームや controlling TTY から入力を消費させない。
2. **全 return path**: cache 未設定 / 不在、空 stdout、非0 exit、非空成功のすべてを shell function / ZLE widget 内で完結させる。非空成功時だけ bash の `READLINE_LINE` / `READLINE_POINT`、zsh の `BUFFER` / `CURSOR` を更新する。
3. **line editor redisplay**: bash は `bind -x` から readline へ通常 return し、readline 管理外の terminal reset を行わない。zsh は `emulate -L zsh` の local option を漏らさず、PTY 比較に基づいて ZLE 推奨 redisplay（`zle -R` を含む候補）を選ぶ。`zle reset-prompt` の置換は観測試験で navigation が改善することを確認して決める。
4. **termios 限定復元**: 上記を直しても製品コード起因の属性差分が stable prompt 境界で反復再現する場合だけ、その属性を shortcut 開始時の `tcgetattr` 観測値へ戻す。`stty sane`、固定 termios、全属性の無条件上書きは実装しない。差分を再現できなければ termios 復元コードは追加しない。
5. **0055 handoff hook**: `aish` の bash / zsh handoff recall に同じ buffer / cursor / redisplay / return 契約を適用する。handoff 候補ありでは rcfile 後段の 0055 binding、候補なし / Human Shell 外では既存 0053 binding、という所有関係を変えない。0055 候補を 0053 cache 巡回へ統合しない。

### 2.3 AC 緑化

各 placeholder は同名テスト関数を保ったまま panic 本体を実試験へ置換する。該当テストを単独実行して緑になった時点でだけ `#[ignore]` を外し、同じ変更で `scripts/spec-acceptance.toml` の当該 `pending` を `false` にする。テスト関数名や file glob を変える必要が生じた場合は registry と同時に更新するが、locked AC ID は変えない。

## 3. 受け入れ条件とテスト計画

| ID | テスト関数 / ファイル | 実装する検証 | pending 解除 |
|----|-----------------------|--------------|--------------|
| `recall_subprocess_cannot_read_widget_input` | 同名 / `ai/tests/0067_recall_keybinding_tty_restore.rs` | non-PTY bash / zsh、deterministic stub、stdin EOF、成功・空・非0の全 return path、buffer 非破壊 | Phase 1A の最初 |
| `recall_keybinding_pty_vertical_e2e` | 同名 / `ai/tests/0067_recall_keybinding_tty_restore.rs` | 実 PTY の bash / zsh × 両 shortcut × labeled outcome matrix、完全 CSI 後の cursor / history、stable prompt termios 一致 | stdin 境界 AC の次。これを vertical gate とする |
| `recall_hooks_avoid_unobserved_terminal_reset` | 同名 / `ai/tests/0067_recall_keybinding_tty_restore.rs` | 生成される 0053 / 0055 hook に `stty sane`、固定値、無条件 reset がないこと。限定復元がある場合は観測値 / 差分属性だけを使うこと | vertical E2E 後。文字列検査だけで navigation AC を代用しない |
| `handoff_recall_preserves_line_editor_navigation` | 同名 / `aish/tests/0067_recall_keybinding_tty_restore.rs` | 実 PTY Human Shell の bash / zsh × 両 shortcut、handoff 候補挿入後の cursor / history、termios 一致、候補なし時に 0053 binding を上書きしないこと | Phase 1B の最後 |

PTY 試験は Unix 専用とし、CI で必須の bash / zsh が見つからない場合に silent skip しない。待機は deadline 付き、transcript は bounded、子 process と FD は成功・panic の両経路で cleanup する。時間依存の固定 sleep だけで prompt 安定を判定しない。テスト都合の新しい製品 API、汎用 PTY framework、第三の binding 実装は追加しない。

## 4. docs 同期

実装と同じ変更で次を同期する。

1. `docs/architecture.md`: 0053 節へ subprocess stdin EOF 境界、bash / zsh の buffer / cursor / redisplay 契約、0055 後段 binding の所有条件を追記する。termios 復元を実装した場合は復元対象属性と観測起点も記録する。
2. `docs/testing.md`: 0067 の4 AC、labeled PTY matrix、stable prompt sentinel、termios のフィールド比較、non-PTY deterministic stub を記載する。
3. `docs/manual/ai-ux.md`: bash / zsh で成功・候補なし・失敗・連続 `Alt+.` / `Alt+,` 後に左右 cursor と上下 history が動く smoke を追記する。0055 Human Shell の handoff 候補でも同じ確認を行う。
4. `docs/security.md`: termios 製品復元を追加した場合のみ、shortcut 開始時の観測値へ差分属性だけを戻すことと、`stty sane` / 固定 reset がユーザー設定を破壊するため禁止であることを追記する。追加しない場合は「該当変更なし」を完了報告に明記する。

## 5. 検証と完了条件

### 5.1 Targeted 検証

実装中は同時に複数 crate の cargo test を走らせず、次の順で直列実行する。

```bash
./scripts/verify-targeted.sh --package ai --test 0067_recall_keybinding_tty_restore
./scripts/verify-targeted.sh --package aish --test 0067_recall_keybinding_tty_restore
./scripts/verify-targeted.sh --docs --architecture
./scripts/check-spec-acceptance.py
./scripts/check-feature-scope.py
```

少なくとも上記2 acceptance binary と、各 package の変更した shell completion module の単体試験を実行する。package 全体の単体試験が必要なときは `--test` を外した `verify-targeted.sh --package <name>` を直列実行する。

### 5.2 Manual smoke

自動試験とは別に、`docs/manual/ai-ux.md` の手順で bash / zsh の通常 shell と 0055 Human Shell を確認する。

- 候補挿入は実行せず prompt buffer だけを更新する
- `Alt+.` / `Alt+,` 後に `←` / `→` で候補内を移動できる
- `↑` / `↓` で shell history を移動できる
- 候補なし、cache 不在、recall 失敗、shortcut と矢印の連続入力後も同じ操作ができる
- Human Shell では handoff 候補ありのときだけ 0055 binding が優先され、候補なしでは 0053 binding を上書きしない

手動 smoke の最終確認は人間が行う。未実施なら完了報告の「残リスク」に明記する。

### 5.3 完了定義

1. 4 AC の同名テストが実装され、`#[ignore]` がなく、`scripts/spec-acceptance.toml` の `pending = false`
2. bash / zsh × `Alt+.` / `Alt+,` の実 PTY matrix、non-PTY stdin EOF、0055 handoff 回帰、reset 禁止契約がすべて成功
3. architecture / testing / manual と、該当時 security が実装に同期
4. `./scripts/verify.sh` を完了直前に1回実行し、失敗時は該当検査だけで修正後、最後に再実行して成功
5. 最終報告へ `.verify-timing-last` の timing summary を転記
6. 全条件達成後だけ本書を `docs/done/0067_recall-keybinding-tty-restore-implementation-spec.md` へ移し、`docs/0000_spec-index.md` と `scripts/feature-scope.toml` の状態を実装済みに更新

## 6. STOP-THE-LINE

次のいずれかが必要になった時点で実装を止め、`scope_revision` と Complexity Gate を再評価する。本 spec へ黙って追加しない。

- `ai recall` 以外の subprocess、daemon、agent loop
- recall cache schema、候補巡回状態、永続 snapshot の変更
- line editor 状態を表す新しい状態機械
- PTY relay / handoff lifecycle または PTY 基盤自体の変更
- 0053 / 0055 以外の integration、第三の recall binding
- terminal emulator 別 key decoding、key timeout、汎用 ESC parser
- crash recovery、lease / heartbeat、reconciler、journal、schema migration、exactly-once
- 観測で特定できない terminal 全体の reset

## 7. Non-goals

- 0053 suggested-command の抽出、cache、巡回 UX、shortcut 意味の再設計
- bash / zsh 以外の shell / line editor、reedline 内蔵 editor への recall binding
- terminal emulator 固有の Meta encoding 対応
- 0055 handoff 候補と 0053 cache の統合
- 新しい実行主体、状態機械、永続 aggregate、設定、protocol、Pack / runtime toggle
- `stty sane`、固定 termios、全属性の無条件上書き
- PTY test infrastructure の作り直しや汎用化

## 8. 仕様との差分

なし。根因仮説は未確定であり、termios 製品復元の要否は Phase 0 の反復可能な PTY 観測結果で決める。差分を再現できない場合は、設計書どおり stdin 分離と shell / ZLE の終了契約だけを修正する。
