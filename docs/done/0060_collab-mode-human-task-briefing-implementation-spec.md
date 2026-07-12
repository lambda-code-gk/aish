# 0060 Collab Mode Human Task Briefing 実装指示書

設計書: [`docs/spec/0060_collab-mode-human-task-briefing-spec.md`](../spec/0060_collab-mode-human-task-briefing-spec.md)

> **Scope revision 4**: origin/main にない outcome schema を導入せず、Human Shell 開始時の安全な briefing と追加入力なしの即時 return UX を実装する。全 18 AC 緑化後に `status = done`（実装指示書は `docs/done/`）。

## 0. 目的

Collaborative Mode の Human Shell が開いた直後、既存の親要求と候補コマンドから、目的、handoff の固定理由、最初の候補操作、親へ戻るタイミングを厳密な固定形式で stderr に表示する。Human Shell 終了後は outcome collector や summary / 理由入力を起動せず、0055 の同期 handoff 経路でただちに親 agent へ制御を返す。

### 0.1 Scope Lock

- Feature scope registry: `scripts/feature-scope.toml`
- Scope revision: `4`
- Complexity class: Green
- Vertical slice AC ID: `human_task_briefing_renders_collaborative_mode_header`
- Locked AC IDs: 設計書 §9 および本書 §6 の18 ID（順序を含め `scripts/feature-scope.toml` と一致させる）
- パック構成: No。変更主体は対象外の `aish` であり、全 Collaborative Mode handoff に必要な core UX のため Pack 境界を作らない

実装開始前に `./scripts/check-feature-scope.py` を通す。Scope Lock 後に新しい実行主体、状態機械、永続 aggregate、external effect、process boundary、integration、protocol / env を追加する必要が生じたら実装を止め、設計書 §7 と Feature Development Policy に従う。

## 1. 非目標・禁止事項

- 新 crate / dependency、`human_task` tool、side agent、別 agent loop、Human Task domain aggregateを追加しない。
- 新しい環境変数、設定、protocol field、request / response、shell_exec schema を追加しない。`collab_outcome` と関連 wire 型も追加しない。
- briefing、task、outcome、summary、理由、履歴、resume 情報を永続化しない。
- 候補コマンドを自動実行せず、実行済み・成功済み・完了済みと推定しない。
- 親要求から handoff 理由、完了条件、status を推測しない。
- outcome / summary / 理由を対話収集せず、exit code、return marker、表示内容から outcome を合成しない。
- 0055 の同期 lifecycle、0057 の PTY cleanup、通常 shell_exec、非 Collaborative Mode、approval protocol を再設計しない。
- 汎用 renderer framework、テーマ、翻訳、幅依存折り返しへ拡張しない。

## 2. Phase 分割

0060 は単一 Phase とする。briefing renderer、stderr adapter、既存 protocol DTO の不変性、実 PTY return、通常モード回帰を一本の vertical slice として完結させる。

| Phase | 内容 | ゲート |
|-------|------|--------|
| 1 | 固定 briefing を Human Shell 開始時に表示し、終了後の追加入力なしで親へ即時 return する | 0060 の全18 ACを `pending = false`。縦断ゲートは `human_task_briefing_renders_collaborative_mode_header` |

部分的な pending 解除や、renderer 単体だけで Phase 完了とはしない。

## 3. 変更ファイル一覧と作業

| 区分 | ファイル | 作業 |
|------|----------|------|
| Human Shell 本番経路 | `aish/src/human_shell.rs` | `format_indented_parent_request` を汎用の `format_indented_block` に置換し、純粋な `render_human_task_briefing` を追加する。`print_handoff_briefing` は既存2 env の読取と renderer 結果の best-effort stderr 出力だけにする。`HANDOFF_ENV_KEYS` と起動順は維持する |
| 0060 AC | `aish/tests/0060_collab_mode_human_task_briefing.rs` | 18個の `pending!()` / `#[ignore]` skeleton を、同名の本番 API・実プロセス検証へ置換する |
| 実 PTY回帰 | `aish/tests/0055_minimal_human_handoff.rs` および既存0055/0057 PTY test | briefing の表示、Ctrl+D / `exit` 後の正常 return、cleanup の非回帰を既存 fixture / timeout で検証する。重複 E2E は増やさない |
| composition root | `ai/src/main.rs` | 終了後 collector や mapper を追加せず、termios 復元後の `HumanHandoffResult` を既存 approval result に直接載せる |
| 0059 専用実装 | `ai/src/adapters/outbound/collab_outcome.rs`、`ai/src/ports/outbound/collab_outcome.rs`、`ai/src/domain/collab_outcome.rs`、`ai/src/application/collab_outcome.rs` と各 `mod.rs` | production 参照がなくなることを確認して collector / parse / mapper と re-export を除去する。別用途が実在する場合は STOP-THE-LINE し、推測で残さない |
| Protocol DTO | `aibe-protocol/src/collaborative_handoff.rs` と関連 unit test | origin/main の `HumanHandoffResult` schema を維持し、`CollabOutcome` / `collab_outcome` が source に無いことを静的検査する |
| Protocol consumers | `ai`、`aibe`、`aibe-client` 内の `HumanHandoffResult` literal / assertion | wrapper や新規 field を足さず、既存 DTO を直接返す |
| 0059 tests / registry | `ai/tests/0059_collab_outcome_status.rs`、`ai/tests/0055_collaborative_handoff_vertical_e2e.rs`、`ai/tests/normal_shell_exec_regression.rs`、`scripts/spec-acceptance.toml` | §7 の撤回・差し替え手順に従い、collector 必須契約を成功条件として残さない |
| Scope registry | `scripts/feature-scope.toml` | 0060 revision 2 / locked18 AC を維持する。実装中の勝手な追加・削除をしない。0059 registry を変更する場合は scope checker と履歴整合を同時確認する |
| Docs | `docs/architecture.md`、`docs/manual/0059_collab-outcome-status.md`、`docs/manual/README.md`、必要なら `docs/manual/0060_collab-mode-human-task-briefing.md`、完了時 `docs/0000_spec-index.md` | required outcome 契約を optional / 省略へ更新し、開始時表示と即時 return の手動確認へ同期する。完了後だけ task を `done/` へ移して index を実装済みにする |

ファイル名は現状調査に基づく。実装時に参照箇所を `rg` で再確認し、compile 修正のために責務を拡大しない。

## 4. 関数契約

### 4.1 `render_human_task_briefing(parent_request, suggested_command)`

- 引数だけから `String` を返す純粋関数とし、env、stdin / stdout / stderr、filesystem、時刻を読まない。
- 出力は設計書 §1.2 の空行、見出し、句読点、`Alt+.` / `Alt+,`、`` `exit` `` を含む固定形式に厳密一致させる。
- 両引数は trim 後が空なら、それぞれ `No parent request summary is available.` / `No command was provided.` を使う。非空値の表示前後空白を勝手に意味変換しない。
- Objective と Suggested first action の両方を `format_indented_block` 経由で処理する。
- 固定理由と Done when を要求内容から生成しない。候補を実行済みと扱う文言を生成しない。

### 4.2 `format_indented_block(value)`

- 入力を論理改行単位に分け、各論理行を個別に `escape_for_handoff_display` 相当で無害化し、その各行へ2空白を前置して `\n` で再結合する。
- 複数行文字列全体を先に escape して改行をリテラル `\\n` へ潰してはならない。
- 行内の ESC / OSC、CR、TAB、その他 C0 を terminal 制御として解釈されない表現にする。UTF-8 の通常文字は保持する。
- 空行を含む論理行構造を保持する。末尾改行の扱いも test で固定し、renderer 内で場当たり的に補正しない。
- I/O と env 読取を持たない小さい純粋 helper とする。

### 4.3 `print_handoff_briefing()`

- `AISH_HANDOFF_PARENT_REQUEST` と `AISH_HANDOFF_SUGGESTED_COMMAND` を読み、未設定を空値として renderer に渡す。
- renderer の返値を stderr に一度だけ best-effort 出力する。出力失敗で Human Shell 起動契約を変えない。
- formatting、推測、outcome 収集、stdin 読取、永続化、候補実行を担わない。
- `run_human_shell` 内の既存位置（環境検証後、Human Shell 起動前）を維持する。

## 5. テスト計画

### 5.1 単体・プロセス統合

- renderer の完全一致 test で header、固定理由、未実行宣言、Done when、user control、Ctrl+D / `exit` をまとめて固定する。
- Objective / Suggested の通常値、未設定相当、空白のみ、複数行、空行、末尾改行、日本語を検証する。
- 両 field の各行へ ESC / CSI / OSC、BEL、CR、TAB、NUL、その他 C0 を混ぜ、実制御列が残らず論理改行だけが残ることを検証する。
- env を設定した `aish human-shell` 子プロセスまたは stderr 差し替え可能な境界で、printer が既存 env のみを読み renderer 出力を stderr に出すことを検証する。並列 test の global env 競合を避ける。
- protocol test で `CollabOutcome` / `CollabOutcomeStatus` / `collab_outcome` が collaborative_handoff.rs の本番定義に無いことを静的確認する。

### 5.2 実 PTY回帰

- 既存0055 fixture、mock aibe、bounded timeout / watchdog を再利用し、Human Shell 開始直後の stderr に固定 briefing が出ることを確認する。
- 0060 は briefing の Ctrl+D / `exit` 案内と `exit` 後の即時 return を確認する。Ctrl+D の実 PTY return は既存 0055 E2E を正本とする。
- 0057 の timeout、signal、descendant cleanup、return marker の既存 test を直列で通す。shell exit code から status を作る assertion は追加しない。

### 5.3 通常モード回帰

- `ai/tests/normal_shell_exec_regression.rs` 等で通常 shell_exec と非 Collaborative Mode の approval、実行、出力が変わらず、briefing / outcome prompt が出ないことを確認する。
- 既存 `HANDOFF_ENV_KEYS` が4個のままで、handoff env が child へ漏れない既存0055 test を維持する。

### 5.4 pending skeleton を緑にする手順

1. `aish/tests/0060_collab_mode_human_task_briefing.rs` の18関数名を維持し、`pending!()` を本番経路 assertion に置換する。
2. 純粋 renderer 系は直接 API を検証し、lifecycle / no-outcome / normal regression 系は実プロセスまたは既存 E2E helper を使う。ソース文字列検索だけのダミー test にしない。
3. 対応実装と test が緑になった後だけ `#[ignore]` を外す。全18 ACを同じ Phase で解除する。
4. 同じ変更で0060全 rowを `pending = false` にし、`./scripts/check-spec-acceptance.py` を通す。

## 6. 0060 AC と `spec-acceptance.toml` 対応

全 row は Phase 1、test 関数は ID と同名、既存 file glob は `aish/tests/0060_collab_mode_human_task_briefing.rs` を基本とする。E2E を既存ファイルへ移す場合は registry の `file_glob` も同時更新する。

| AC ID | 主な検証 | 完了時 |
|-------|----------|--------|
| `human_task_briefing_renders_collaborative_mode_header` | 厳密 header / underline / Human Task | pending→false |
| `human_task_briefing_renders_objective` | env内容と trim空 fallback | pending→false |
| `human_task_briefing_uses_fixed_reason` | 固定理由のみ、推測なし | pending→false |
| `human_task_briefing_renders_suggested_first_action` | 候補と trim空 fallback | pending→false |
| `human_task_briefing_states_command_not_executed` | 自動実行していない明示 | pending→false |
| `human_task_briefing_preserves_user_control` | edit/run/replace/ignore、Alt操作 | pending→false |
| `human_task_briefing_renders_done_when` | 固定 return timing | pending→false |
| `human_task_briefing_returns_with_ctrl_d_or_exit` | 両操作、追加入力なしのPTY return | pending→false |
| `human_task_briefing_indents_multiline_objective` | 両 field の行別2空白、改行保持 | pending→false |
| `human_task_briefing_sanitizes_ansi_and_c0` | 両 field の行別 escape | pending→false |
| `human_task_briefing_renderer_is_pure` | 引数だけから同一文字列 | pending→false |
| `human_task_briefing_printer_only_reads_env_and_stderr` | 既存2 env→renderer→stderr | pending→false |
| `human_task_briefing_has_no_outcome_selection` | 終了後 status prompt なし | pending→false |
| `human_task_briefing_has_no_summary_input` | summary / 理由入力なし | pending→false |
| `human_task_briefing_adds_no_protocol_schema` | protocol source に `CollabOutcome*` / `collab_outcome` が無い | pending→false |
| `human_task_briefing_uses_only_existing_env` | `HANDOFF_ENV_KEYS` 4個のまま | pending→false |
| `human_task_briefing_creates_no_persistent_state` | task / briefing / outcome stateなし | pending→false |
| `human_task_briefing_normal_shell_exec_regression` | 通常・非collab不変 | pending→false |

## 7. 0059 outcome 契約の撤回手順

1. `ai/src/main.rs` から outcome collector の生成・呼出しを外し、Human Shell 成功後は termios を復元して、元の `HumanHandoffResult` をそのまま approval に返す。stdin を追加で読まない。
2. `HumanHandoffResult` は origin/main の schema に戻し、`collab_outcome` field と関連 wire 型を完全に削除する。
3. 終了後 collector、port、domain parse、application mapper を追加せず、production / 有効 test に専用コードや module export がないことを確認する。
4. 0059 ACを次のように分類する。
   - 0060と正面から矛盾する collector / 必須 outcome AC（structured return、status forms、invalid retry、domain invariant、全status serialize、exit-code independence、noninteractive failure、stream I/O）は、履歴を成功条件として残さず `pending = true` に戻すか registry から削除し、対応 test を削除する。
   - launch failure で prompt なし、通常 shell_exec 不変など0060でも必要な性質は、0059の collector 前提 assertionを除き、0060の `has_no_outcome_selection` / `normal_shell_exec_regression` 等へ差し替える。
   - どちらを採るかは `check-feature-scope.py` / `check-spec-acceptance.py` が要求する done spec・locked AC の整合に合わせ、必要なら0059の feature status / `locked_ac_ids` / scope revision、spec index の「実装済み」表記も同じ変更で履歴が嘘にならないよう同期する。0059設計書や done 指示書を改竄して0060の正本にしない。
5. 0059 test 名を残して assertion を逆転させることは禁止する。0060の置換契約は0060 AC名の testで検証する。
6. registry の削除・pending化・差し替え後、0060の locked18 ACとの1:1対応を確認し、両 checkerを通す。

## 8. ドキュメント・手動検証同期

- `docs/architecture.md`: Collaborative handoff 節を、開始時 briefing、renderer / env+stderr adapter 分離、行単位 escape、protocol schema 不変、終了後即時 return に更新する。
- manual: 実 PTYでのみ確実に確認できる表示位置、Alt+./Alt+,、Ctrl+D / `exit`、終了後 prompt なし、通常モード非表示を確認するため、`docs/manual/0060_collab-mode-human-task-briefing.md` を追加するか、0059 manual を0060契約へ明示的に置換する。`docs/manual/README.md` のリンクも同期する。実施しなければ最終報告の「残リスク」に明記する。
- `docs/0000_spec-index.md`: 実装中は0060 tasks rowを維持する。全 AC が緑になった後だけ本書を `docs/done/` へ移し、0060を「設計確定（実装済み）」へ更新する。0059表記は §7 のregistry判断と一致させる。
- 設計書 `docs/spec/0060_collab-mode-human-task-briefing-spec.md` は移動しない。

## 9. 実装順序

1. `feature-scope.toml` revision 2、0060の18 AC、既存 ignored skeleton、0055/0057/0059経路を再確認し、scope checkerを通す。
2. renderer単体 testへ skeletonを置換し、`format_indented_block` と `render_human_task_briefing` を実装して固定形式、fallback、複数行、ANSI/C0を緑にする。
3. `print_handoff_briefing` を env読取＋stderrだけへ縮め、Human Shell起動前のプロセス統合 testを緑にする。
4. protocol DTOを optional化し、None省略・欠落decode・旧Some decodeを先に緑にする。
5. composition rootから0059 collector呼出しを除去し、専用domain / port / adapter / mapperと参照を整理する。
6. 0059 AC / test / registryを §7 に従って pending化・削除・0060へ差し替え、checker整合を回復する。
7. 既存fixtureでCtrl+D / `exit` の実PTY回帰、0055/0057 cleanup、通常モード回帰を直列で通す。
8. `docs/architecture.md`、manual、manual indexを実装へ同期する。
9. 18 testの `#[ignore]` と0060全 rowの pendingを同時解除し、targeted test、scope / acceptance checkerを通す。
10. 完了直前に `./scripts/verify.sh` を実行する。失敗箇所だけ修正後に再実行し、`.verify-timing-last` の summaryを報告する。
11. 全条件達成後だけ本書を `docs/done/` へ移し、indexを実装済みに更新する。

## 10. 完了条件

- [ ] 設計書 §1.2 の固定 briefing が安全に開始時 stderrへ表示される
- [ ] 0059 outcome / summary入力がなく、Ctrl+D / `exit` 後ただちに親へ戻る
- [ ] `collab_outcome` field / 型を追加せず、statusを推定しない
- [ ] 0060の18 testから `#[ignore]` が外れ、registryが全て `pending = false`
- [ ] 0055 / 0057の関連testと通常 shell_exec回帰が成功する
- [ ] architecture / manual / indexが実装状態に同期する
- [ ] `./scripts/check-feature-scope.py`、`./scripts/check-spec-acceptance.py`、`./scripts/verify.sh` が成功する
- [ ] 手動検証の実施結果、または未実施を残リスクとして報告する

## 11. 仕様との差分

なし。設計書 revision 4 の表示契約、protocol schema 不変、18 ACをそのまま実装する。
