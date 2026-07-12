# 0061 Collab Mode Human Task Evidence 実装指示書

設計書: [`docs/spec/0061_collab-mode-human-task-evidence-spec.md`](../spec/0061_collab-mode-human-task-evidence-spec.md)

> **Scope revision 1（実装開始前）**: 本書作成時点では `status = "draft"` のまま Scope Lock しない。Cursor 実装者は最初の作業として revision を増やし、全15 ACを lock してから本番コードへ着手する。

## 0. 目的と前提

Human Shell 内で記録された handoff byte range の完了済み Shell command span を、ユーザーの summary / outcome 追加入力なしに bounded な構造化 Evidence として親 collaborative agent へ返す。Evidence は観測事実だけを保持し、Human Task や requested command の完了・成功・失敗・blocked を推定しない。

既存機能は次の責務を正本として再利用する。

- 0049: `log.jsonl` の event schema、sanitized / redacted command、`aish_replay::replay_span_views` による span 対応付け。
- 0055: 同期 Human Handoff、`shell_session_dir` と `shell_log_start..shell_log_end` の受け渡し、synthetic tool result。
- 0057: PTY timeout / signal / descendant cleanup と return marker。Evidence 追加で lifecycle を変更しない。
- 0060: Human Shell 開始時 briefing と、Ctrl+D / `exit` 後に outcome / summary を聞かず即時 return する UX。

パック構成は **No**。全 Collaborative Mode handoff に必要な core post-handoff observation であり、optional 配備・runtime toggle・独立 RPC / CLI ではない。Pack trait、Active / Basic Pack、composition toggle は追加しない。

### 0.1 Scope Lock（実装開始時の最初の作業）

`scripts/feature-scope.toml` の0061 entryを、実装コードへ触る前に次のように更新する。

- `status = "locked"`
- `scope_revision = 2`（現在値から `+1`。開始時点で既に revision が進んでいれば、その値からさらに `+1`）
- `scope_change_reason` に「implementation scope lock」相当の理由を記録
- `locked_ac_ids` に設計書 §9 の AC1–15を同じ順序で全て記録
- Complexity class は Yellow、`scope_review = "approved"`、`vertical_slice_ac_id = "human_task_evidence_vertical_e2e"`、complexity inventory は変更しない

その直後に `./scripts/check-feature-scope.py` を通す。ACの追加・削除・名称変更、integration や complexity inventory の変更が必要なら本番実装を開始せず STOP-THE-LINE とする。

## 1. Phase 分割

0061 は単一 Phase とする。DTOだけ、readerだけ、pure builderだけを完了扱いにせず、Human Shell の command 実行から親 synthetic tool result までを一本の vertical slice として完結させる。

| Phase | 内容 | ゲート |
|-------|------|--------|
| 1 | backward-compatible DTO、bounded range reader、pure Evidence builder、observer統合、実PTY E2E、回帰・docs同期 | 0061の全15 ACを同じ変更で `pending = false`。縦断ゲートは `human_task_evidence_vertical_e2e` |

実装中は ignored skeleton を小刻みに本物の assertion へ置換して targeted test を回してよいが、`scripts/spec-acceptance.toml` の pending と `#[ignore]` は全15 ACの本番経路が緑になるまで解除しない。

## 2. 変更ファイルと責務

| 区分 | ファイル | 作業 |
|------|----------|------|
| Protocol DTO | `aibe-protocol/src/collaborative_handoff.rs` | `HumanTaskCommandEvidence`、`HumanTaskEvidence`、`PostHandoffObservation.human_task_evidence` を追加する。旧payload decode、`None`省略、既存field wire表現の backward compatibility testを同ファイルへ置く |
| Protocol export | `aibe-protocol/src/lib.rs` | 新DTOを既存 collaborative handoff 型と同じ経路で re-exportする。新moduleや別protocolを作らない |
| Range I/O adapter | `ai/src/adapters/outbound/replay_source.rs` | `load_replay_events_in_range` と range reader固有の結果 / error表現を追加し、byte range検証、8 MiB末尾scan、部分行処理、JSONL parseだけを担う |
| Pure builder | `ai/src/domain/human_task_evidence.rs`（新規）と `ai/src/domain/mod.rs` | parse済み `LogEvent` と readerのtruncation情報から `replay_span_views` を呼び、Shell完了spanの選別、上限、index、DTO構築を純粋に行う。adapters→application 禁止のため domain に置く |
| Observer adapter | `ai/src/adapters/outbound/human_handoff.rs` | `ProcessEnvironmentObserver` から `shell_session_dir/log.jsonl` の ranged readerとpure builderを接続する。失敗codeを既存 `observation_errors` に重複なく追加し、他観測を継続する |
| Adapter export | `ai/src/adapters/outbound/mod.rs` | observer内部またはtestで必要な最小限だけ exportする。既存 `load_replay_events` 利用者を壊さない |
| Ports | `ai/src/ports/outbound/human_handoff.rs` 等 | **原則 signature変更なし**。既存 `EnvironmentObserver::observe(cwd, start, end, session_dir)` と `ShellTranscriptReader` を維持する。Evidence専用I/O portやprotocol wrapperを増やさない |
| Composition root | `ai/src/main.rs` | DTO field追加に伴う構築子・import等のcompile修正だけ。range parsing、Evidence構築、error mapping、prompt、collectorを置かない |
| 0061 AC | `ai/tests/0061_collab_mode_human_task_evidence.rs` | 既存15個の ignored / pending skeletonを同名の本番API・実プロセス assertionへ置換する |
| 既存回帰 | `ai/tests/0055_minimal_human_handoff.rs`、`ai/tests/0055_collaborative_handoff_vertical_e2e.rs`、`ai/tests/normal_shell_exec_regression.rs`、必要な0057 PTY test | observerの既存観測、synthetic result、requested completion unknown、通常モード、PTY cleanupの非回帰を確認する。fixtureを再利用する |
| Registry | `scripts/feature-scope.toml`、`scripts/spec-acceptance.toml` | 開始時Scope Lock、完了時15 ACの pending解除。ACとtest関数を1:1で維持する |
| Docs | `docs/architecture.md`、`docs/manual/0061_collab-mode-human-task-evidence.md`、`docs/manual/README.md`、`docs/0000_spec-index.md` | protocol / reader / builder / observer境界、実PTY手動確認、進捗状態を同期する |

`aish` の本番コードは原則変更しない。ログ event schema、環境変数、**新しい** hook は追加しない。test fixture の利用を除き `aish` に 0061 固有ロジックを入れない。

ただし設計書 §1.5（scope revision 6 以降）で許可した、既存 0049 replay hook の次の限定修正は実装してよい。

1. bash `PROMPT_COMMAND` で `_aish_replay_precmd` を install より先に実行し `$?` を保持する
2. zsh で `_aish_replay_precmd` を precmd 先頭へ置き、install hook 自身を外す
3. control FIFO emit を fast-path（sleep なし spin）+ 停滞時のみ watchdog にする（新 FIFO は作らない）
4. `handle_control_line` で `pending_end` 中の次 `start` を `queued_start` へ格納する（高速連続入力の span 欠落防止）

これらを超える span semantics 再設計や新規 hook が必要なら STOP-THE-LINE。

## 3. Protocol 契約

### 3.1 DTO

`aibe-protocol/src/collaborative_handoff.rs` に次を追加する。

```rust
pub struct HumanTaskCommandEvidence {
    pub index: u32,
    pub command: String,
    pub exit_code: Option<i32>,
}

pub struct HumanTaskEvidence {
    pub commands: Vec<HumanTaskCommandEvidence>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub truncated: bool,
}

pub struct PostHandoffObservation {
    // existing fields unchanged
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub human_task_evidence: Option<HumanTaskEvidence>,
}
```

`is_false` は `fn is_false(value: &bool) -> bool { !*value }` 相当のmodule-local serde helperとし、汎用utility crateへ広げない。DTOは既存型に合わせ `Debug, Clone, PartialEq, Eq, Serialize, Deserialize` を deriveする。

意味は固定する。

- `Some` + non-empty: 収集成功かつ完了済み Shell commandあり。
- `Some` + empty: 収集成功したが対象rangeに完了済み Shell commandなし。
- `None`: 収集失敗。対応する安定codeが `observation_errors` にある。
- `exit_code = None`: spanに観測値がない。推定で補完しない。
- `truncated = false` は省略可能、欠落時はfalse。`human_task_evidence = None` もserialize時に省略し、欠落fieldをdecodeできる。

旧 `PostHandoffObservation` payloadのdecode、`None` field省略、`Some(empty)` / `Some(commands)` round trip、既存field名と値のwire表現不変をprotocol unit testで固定する。

### 3.2 定数

上限と安定error codeをmagic number / 散在文字列にしない。責務の所有元へ定数を置く。

- range reader: `MAX_EVIDENCE_SCAN_BYTES = 8 * 1024 * 1024`
- pure builder: `MAX_EVIDENCE_COMMANDS = 50`、`MAX_EVIDENCE_COMMAND_BYTES = 2 * 1024`、`MAX_EVIDENCE_TOTAL_COMMAND_BYTES = 16 * 1024`
- observer/error mapping: `human_task_evidence_log_unavailable`、`human_task_evidence_invalid_log`、`human_task_evidence_invalid_range`

公開範囲は本番利用とintegration testに必要な最小限にする。protocol DTO以外を新しいwire契約にしない。

## 4. Evidence 抽出・上限・失敗契約

### 4.1 Ranged reader

`load_replay_events_in_range(path, start, end)` は filesystem I/Oだけを担い、parse済み `Vec<LogEvent>` と reader起因の `truncated` を返せる形にする。

1. 正本pathは observer が組み立てた `shell_session_dir.join("log.jsonl")` のみ。ユーザー指定の別path、symlink追跡によるsession dir外逸脱、別ファイルfallbackを許可しない。安全な containment / file検証が既存境界だけで満たせない場合は推測実装せず STOP-THE-LINE。
2. `end = None` はopen後の観測時点EOFを一度取得して固定する。読みながら伸びたeventを混ぜない。
3. `start > end`、`start > EOF`、`end > EOF` は invalid range。空range (`start == end`) は成功して空events。
4. rangeが8 MiBを超える場合は `effective_start = end - 8 MiB` とし、末尾だけ読む。この時点で `truncated = true`。
5. `effective_start > start` または非zero startが行途中なら次の改行までを捨てる。部分行を捨てたら `truncated = true`。単に行境界上にある非zero startは捨てない。
6. 固定endを越えて読まない。8 MiB以下の指定rangeでstartまたはendがJSONL event境界を壊す場合は invalid range。末尾scanで先頭部分行を捨てた後も、残る完全行だけをparseする。
7. 空白行は既存 reader同様に無視してよい。完全行のI/O失敗は unavailable、JSON / `LogEvent` decode失敗は invalid logへ分類可能なtyped errorで返す。

range offsetはbyte単位であり、文字indexへ変換しない。ファイル全体を先に読み込んでからsliceする実装は禁止する。

### 4.2 Pure builder

builderは filesystem、env、process、時刻を読まず、parse済みeventsとreaderのtruncation flagだけから `HumanTaskEvidence` または構造化されたbuild errorを返す。

1. command pairingを独自実装せず `aish_replay::replay_span_views` を必ず呼ぶ。span構造エラーは invalid logへ上げる。
2. `ReplaySpanView.kind == Shell` かつ command endを持つ完了spanだけを採用する。Exec等の非Shell span、未完了spanは除外する。
3. commandはreplay spanが保持する記録済み sanitized / redacted値だけをコピーする。argv、process、history、元入力から復元しない。
4. `exit_code` はspanの観測値をそのまま `Option<i32>` へ写す。
5. 各Evidenceの `index` は最終選択後の時系列順で0から連番とし、落とした古いspanの元indexやlog offsetを公開しない。50件を超える場合は直近50件を残す。
6. 1 commandはUTF-8境界を保って最大2048 bytes。超過時は末尾を `…` に置換し、ellipsisのUTF-8 bytesも2048 bytes内に含める。
7. command bytes合計は最大16384 bytes。1件ごとの切詰め後、合計超過なら古いcommandから落として直近を優先する。選択後は時系列順へ戻す。
8. 件数落ち、個別切詰め、合計上限による落ち、reader末尾scan、reader先頭部分行破棄のいずれかで `truncated = true`。対象spanがない正常ケースは `Some(HumanTaskEvidence { commands: [], truncated: reader_truncated })`。

### 4.3 Observer と failure code

`ProcessEnvironmentObserver::observe` は既存cwd / git / shell tail観測を維持し、その結果へEvidenceを合成する。

- session dir欠落、`log.jsonl` open/read不可、対象ファイル逸脱: `human_task_evidence_log_unavailable`
- JSONL / event decode、replay span構造不正、安全にparse不能: `human_task_evidence_invalid_log`
- offset逆転、EOF外、8 MiB以下のevent境界不整合: `human_task_evidence_invalid_range`
- Evidence失敗時は `human_task_evidence = None`。handoff result自体をerrorにせず、cwd / git / shell tailを返す。
- 既存 `observation_errors` を保持し、同一codeは一度だけ追加する。Evidence用codeで既存shell tail errorを置換しない。
- Evidence成功時はemptyでも必ず `Some`。`shell_session_dir = None` は unavailableとして扱う。
- `requested_command_completion = Unknown`、`execution_outcome`、human shell exit codeの既存意味を変更しない。

## 5. 実装順序

1. **Scope Lock**: §0.1どおり `feature-scope.toml` をrevisionアップ、`status = locked`、全15 AC lockへ更新し、scope checkerを通す。現行の0061 ignored skeletonとacceptance registryの1:1対応も確認する。
2. **Protocol first**: DTO、`is_false`、re-export、backward compatibility unit testを追加する。全 `PostHandoffObservation` literalをcompile修正するが、この段階で `main.rs` にロジックを置かない。
3. **Range reader**: byte境界、EOF固定、8 MiB tail scan、部分行、invalid range / invalid log / unavailableのtyped testを先に追加して実装する。既存 `load_replay_events` は維持する。
4. **Pure builder**: replay span再利用、Shell/完了選別、exit code、UTF-8切詰め、50件 / 16 KiB、recent優先、redaction保持をtable-driven testで緑にする。
5. **Observer integration**: ranged reader→builderを `ProcessEnvironmentObserver` に接続し、Some(empty) / None、stable code、dedup、既存観測継続をtestする。port signatureは維持する。
6. **Vertical E2E / regression**: 既存0055/0057 fixtureとmock aibeを再利用し、実PTY内の複数command→exit→range→Shell span→structured Evidence→親synthetic resultを追加入力なしで検証する。通常 / 非collabとcleanupも回帰確認する。
7. **Docs・registry**: architecture、manual、manual indexを同期する。全15本番testが緑になった後だけ `#[ignore]` を外し、全0061 rowを同時に `pending = false` にする。
8. **最終検証**: targeted検証、scope / acceptance checkerの後、`./scripts/verify.sh` を1回実行する。失敗箇所だけ修正して再実行し、`.verify-timing-last` の timing summaryを報告する。
9. **完了処理**: 全条件達成後だけ本書を `docs/done/` へ移し、indexを「設計確定（実装済み）」へ更新する。`feature-scope.toml` の `status = "done"` とrevision / reasonも同じ変更で同期する。

## 6. テスト仕様と AC 対応

既存 `ai/tests/0061_collab_mode_human_task_evidence.rs` の関数名を維持する。単一関数に全assertionを詰めず、protocolの詳細はprotocol unit test、reader / builderの境界値は各module unit test、0061 integration testはACの本番経路を代表して検証する。ソース文字列検索だけ、DTO直組みだけ、mock builderだけでACを緑にしない。

| AC ID / test関数 | 主な検証層と内容 |
|-------------------|------------------|
| `human_task_evidence_is_collected_automatically` | observer統合: session rangeからEvidenceを作り、追加stdinなしでobservationへ格納 |
| `human_task_evidence_contains_commands_and_exit_codes` | builder + integration: command、Some / None exit codeを推定なしで転記 |
| `human_task_evidence_uses_handoff_log_range` | range reader: range外event除外、None→固定EOF、8 MiB tail、部分行discard |
| `human_task_evidence_reuses_replay_spans` | builder: 0049 fixtureを `replay_span_views` と同じ結果へ通す。独自pairingで通る人工DTO testにしない |
| `human_task_evidence_excludes_non_shell_spans` | builder: Shell完了、Shell未完了、Exec完了の混在fixture |
| `human_task_evidence_is_bounded` | reader / builder境界値: 50 / 51件、2048 bytes前後、16384 bytes前後、8 MiB前後、日本語UTF-8 |
| `human_task_evidence_keeps_recent_commands` | builder: 古いcommandを落とし直近を時系列順で返す。各truncation原因でflag=true |
| `human_task_evidence_preserves_redaction` | 既存sanitize済みlog fixtureを投入し、秘密の原値がDTO / serialized resultに出ないことを負のassertionで確認 |
| `human_task_evidence_distinguishes_empty_from_unavailable` | observer + protocol: Some(non-empty)、Some(empty)、None+error codeの3状態 |
| `human_task_evidence_failure_is_nonfatal` | observer: unavailable / invalid log / invalid range全code、dedup、cwd/git/tail保持、handoff継続 |
| `human_task_evidence_requires_no_manual_summary` | 実PTY: `exit` 後にsummary / reason / status / outcome promptなしでbounded timeout内に親へ戻る |
| `human_task_evidence_does_not_infer_completion` | E2E serialized result: command exit codeが0 / nonzeroでも `requested_command_completion = unknown` |
| `human_task_evidence_protocol_is_backward_compatible` | protocol unit: old payload decode、None省略、false省略/default、既存field wire不変、Some round trip |
| `human_task_evidence_normal_mode_regression` | 既存通常shell_exec / 非Collaborative ModeにEvidence収集・prompt・挙動変更なし |
| `human_task_evidence_vertical_e2e` | 実PTY + mock aibe: 複数command、Human Shell終了、range reader、完了Shell選別、synthetic resultまで縦断 |

追加で既存0055 observer testを更新し、正常log fixtureを渡さない既存ケースがEvidence追加だけで壊れないよう期待値を明示する。0057 timeout / signal / descendant cleanupは直列かつbounded timeoutで実行し、Evidence I/Oがcleanupを遅延・阻害しないことを確認する。

## 7. ドキュメント更新

- `docs/architecture.md`: Collaborative handoff protocol DTO、`PostHandoffObservation` の3状態、range reader（固定EOF / 8 MiB）、pure builder、observer合成、stable error code、requested completion unknownを同期する。
- `docs/manual/0061_collab-mode-human-task-evidence.md`: 実PTYで複数command（成功・非zeroを含む）を実行し、Ctrl+D / `exit` 後に追加入力なしで戻ること、親へcommand / exit codeが構造化返却されること、range外command・秘密原値が混ざらないことを確認する。破損log / 巨大rangeは自動testを正本とし、危険な手動改変を必須にしない。
- `docs/manual/README.md`: 0061 manualへのリンクと対象を追記する。
- `docs/0000_spec-index.md`: 実装中はtasks rowを維持する。全AC緑化・verify成功後だけ本書をdoneへ移し、spec / implementation両rowを実装済みに更新する。
- 設計書は `docs/spec/` に残し、実装都合で契約を書き換えない。矛盾を発見したら設計書を優先し STOP-THE-LINE。

## 8. STOP-THE-LINE 条件

次のいずれかが必要・発見された時点で実装を止め、分類、scope revision更新、Complexity Gate再判定、別spec分割を行う。

- completion / success / failure / blocked、manual summary、outcome、LLM要約、output previewの追加または推定
- Evidence永続化、履歴、検索、resume、crash recovery、schema migration、exactly-once
- side agent、二つ目のagent loop、複数Human Task coordination、lease / heartbeat / reconciler
- 新しい実行主体、状態機械、process boundary、外部副作用、ログevent、環境変数、hook
- `aish` 本番コードへの**新規** hook / 環境変数 / ログ event、または既存 0049 replay hook の exit_code/FIFO/`queued_start` 修正を超える span semantics 再設計
- `ai/src/ports` のsignature変更やEvidence専用portが不可避になった場合
- 8 MiB bounded readでは安全なrange / JSONL境界検証ができない、またはsession dir外逸脱防止に新しい基盤契約が必要な場合
- AC追加・削除・意味変更、Yellow inventory / integrationsの変更
- 既存0055 / 0057 / 0060契約と両立しないことが判明した場合

発見事項は `BLOCKER_ORIGINAL_AC` / `REGRESSION` / `SAFETY_WITHIN_FAULT_MODEL` / `NEW_REQUIREMENT` / `HARDENING` / `OUT_OF_FAULT_MODEL` に分類する。後3分類を0061へ便乗実装しない。

## 9. 完了条件チェックリスト

- [ ] 実装開始時に0061を `status = locked`、scope revisionを+1、全15 ACを順序どおりlockし、scope checkerが成功した
- [ ] protocol DTOが欠落decode、None / false省略、既存wire表現を維持する
- [ ] range readerが指定rangeだけを最大8 MiB読み、EOF固定・部分行・不正rangeを契約どおり扱う
- [ ] pure builderが `replay_span_views` を再利用し、完了Shell spanだけをbounded Evidenceへ変換する
- [ ] 50 commands、2 KiB / command、16 KiB total、8 MiB scanと全truncation条件が境界値testで固定された
- [ ] sanitized / redacted commandだけを返し、秘密原値を復元・出力しない
- [ ] Some(non-empty) / Some(empty) / None+stable codeを区別し、Evidence失敗でもhandoffと他観測が継続する
- [ ] `requested_command_completion = unknown` を維持し、Human Task outcomeを推定しない
- [ ] 実PTY vertical E2Eが追加stdinなしに親synthetic tool resultまで成功する
- [ ] 通常 / 非Collaborative Mode、0055同期handoff、0057 cleanup、0060即時returnに回帰がない
- [ ] `aish` への新規 hook / port signature / `ai/src/main.rs` の責務拡張をしていない（既存 0049 hook の限定修正は設計書 §1.5 の範囲内）
- [ ] 15 AC testの `#[ignore]` が外れ、registryの全rowが `pending = false`
- [ ] architecture、0061 manual、manual README、indexが実装状態に同期した
- [ ] targeted test、`check-feature-scope.py`、`check-spec-acceptance.py` が成功した
- [ ] `./scripts/verify.sh` が成功し、`.verify-timing-last` のsummaryを最終報告へ転記した
- [ ] 手動PTY検証の実施結果、または未実施を「残リスク」に記載した
- [ ] 全条件達成後だけ本書を `docs/done/` へ移し、feature status / indexをdoneへ更新した

## 10. 仕様との差分

なし。設計書の bounded Evidence、失敗非致命、完了推定なし、既存0049 / 0055 / 0057 / 0060再利用をそのまま実装する。
