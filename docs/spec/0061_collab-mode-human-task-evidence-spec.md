# 0061 Collab Mode Human Task Evidence 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定（実装済み）  
> **関連**: [`docs/feature-development-policy.md`](../feature-development-policy.md)、[`0045_pack-composition-spec.md`](0045_pack-composition-spec.md)、[`0049_aish-command-output-replay-spec.md`](0049_aish-command-output-replay-spec.md)、[`0055_minimal-human-handoff-spec.md`](0055_minimal-human-handoff-spec.md)、[`0060_collab-mode-human-task-briefing-spec.md`](0060_collab-mode-human-task-briefing-spec.md)

## 0. Core outcome

Human Shell 内で記録された command span を、ユーザーの追加入力なしで構造化 Human Task Evidence として親エージェントへ返す。

Evidence は観測事実だけを表し、要求作業の完了、成功、失敗、blocked などの状態を判定しない。

## 1. Minimum vertical slice

```text
親 collaborative agent の shell_exec
→ 0055 の既存同期 handoff で Human Shell を起動
→ ユーザーが Human Shell 内で command を実行
→ Ctrl+D または exit で Human Shell を終了
→ ai が shell_session_dir/log.jsonl の handoff range だけを読む
→ aish_replay::replay_span_views で完了済み Shell span を構造化する
→ PostHandoffObservation.human_task_evidence に格納
→ 0055 の synthetic tool result として追加入力なしに親へ返す
```

### 1.1 Protocol DTO

`aibe-protocol` に次の後方互換な DTO 拡張を行う。実フィールド追加は実装 step で行い、本設計 step では行わない。

```text
HumanTaskCommandEvidence
  index: u32
  command: String
  exit_code: Option<i32>

HumanTaskEvidence
  commands: Vec<HumanTaskCommandEvidence>
  truncated: bool

PostHandoffObservation
  human_task_evidence: Option<HumanTaskEvidence>
```

`human_task_evidence` は deserialize 時に欠落を許容する default を持ち、serialize 時は `None` を省略する。これにより旧クライアント／旧 payload との互換性を保つ。

値の意味は次で固定する。

| 値 | 意味 |
|----|------|
| `Some` かつ `commands` が非空 | Evidence の収集に成功し、完了済み Shell command span が存在した |
| `Some` かつ `commands` が空 | Evidence の収集に成功したが、対象範囲に完了済み Shell command span がなかった |
| `None` | Evidence の収集に失敗した、またはログ範囲を安全に読めず、対応する安定 code が `observation_errors` に記録された |

`exit_code` は command span に記録された観測値であり、Human Task 全体の完了状態ではない。既存 synthetic tool result の `requested_command_completion = unknown` は変更しない。

`truncated` は、指定 range の一部または command Evidence の一部を上限処理で省略したことを表す。具体的には、command 件数超過、1 command の 2 KiB 超過、command 文字列合計の 16 KiB 超過、指定 range の 8 MiB 超過による末尾 scan、または末尾 scan の読み取り開始位置調整で先頭の部分行を破棄した場合に `true` とする。

### 1.2 Log range と再利用境界

入力正本は `shell_session_dir/log.jsonl` の byte range `shell_log_start..shell_log_end` とする。`shell_log_end = None` の場合は観測時点の EOF を終端として固定して読み、範囲外の event は Evidence に混ぜない。指定 range が 8 MiB を超える場合は range の末尾 8 MiB だけを読み、`truncated = true` として部分 Evidence を成功扱いする。読み取り開始 offset が JSONL 行の途中なら、次の改行までの先頭部分行を破棄し、これも `truncated = true` とする。開始・終了 offset の逆転、ファイル範囲外、対象ファイル以外への逸脱は拒否する。8 MiB 以下の range で先頭または末尾が event 境界を壊す場合も不正 range とする。

JSONL を `aish_replay::LogEvent` として解釈した後、既存 `aish_replay::replay_span_views` を再利用する。独自の command span pairing を実装しない。`ReplaySpanView.kind` が Shell であり、かつ command end を持つ完了 span だけを採用する。Exec など非 Shell span と未完了 span は除外する。ログに記録済みの sanitized / redacted command だけを DTO に転記し、元の command を復元・再読込しない。

### 1.3 Bounded Evidence

- 最大 50 commands
- command 文字列は UTF-8 境界を保って最大 2 KiB。切り詰めた文字列の末尾には `…` を付け、`…` 自体を 2 KiB に含める
- Evidence の command 文字列合計は最大 16 KiB
- ranged reader の scan は最大 8 MiB
- 上限超過時は直近の完了 Shell commands を優先し、出力順は時系列順に戻す

件数超過で古い command を落とす、1 command を切り詰める、合計 byte 上限で古い command を落とす、range 末尾だけを scan する、またはその scan 先頭の部分行を破棄する場合は `truncated = true` とする。これらの bounded な省略は Evidence 収集失敗ではない。

### 1.4 Error contract

Evidence 収集失敗は handoff 全体を失敗させない。`human_task_evidence = None` とし、`observation_errors` に次の安定 code のいずれかを追加して、既存の cwd / git / shell tail 観測と親 agent 継続を維持する。

| code | 条件 |
|------|------|
| `human_task_evidence_log_unavailable` | session dir または `log.jsonl` を開けない、読み取れない |
| `human_task_evidence_invalid_log` | JSONL、event、span 構造が不正、または scan 上限内で安全に解析できない |
| `human_task_evidence_invalid_range` | offset が逆転、ファイル範囲外、event 境界不整合など range が不正 |

Evidence 専用エラーで既存 `observation_errors` を消さず、同じ code を重複追加しない。

### 1.5 実装境界

変更主体は `ai` と `aibe-protocol` とする。`ai` の outbound adapter に range 制限付き reader と既存 `ProcessEnvironmentObserver` への組み込みを置き、`ai` の domain に I/O 非依存の pure builder を置く。pure builder は既に parse 済みの log events から DTO を構築し、上限と選別規則を一か所で適用する。adapter は application を import しない。

`aish` の本番コードは原則変更しない。0049 のログ形式と span 記録、0055 の range 受け渡し、0060 の briefing と即時 return UX をそのまま使い、**新しい** hook、環境変数、終了後 prompt、ログ event を追加しない。ただし Evidence AC（command の正しい `exit_code` 観測）のために、既存 0049 replay hook の次のバグ修正のみを許可する。

1. bash `PROMPT_COMMAND` で `_aish_replay_precmd` を `_aish_replay_install_hooks` より先に実行し、`$?` を潰さない
2. zsh で `_aish_replay_precmd` を precmd 先頭へ置き、install hook 自身を外す
3. control FIFO への end 書き込みを、完了待ちつき background write + 短時間打ち切り（約 0.5s）の bounded wait にする。`timeout` コマンドへの依存は置かない。新 FIFO / 新 control channel は作らない。

0060 で禁止した `collab_outcome` および outcome/status/summary 入力を再導入しない。

## 2. Fault model

### 2.1 保証対象

標準 Fault Model に従う。単一プロセス生存中の handoff 終了直後に、固定された log range から bounded Evidence を best effort で収集する。Evidence 収集が失敗しても親への制御返却と他の post-handoff observation は継続する。

### 2.2 保証対象外

- crash または OS 再起動後の Evidence 再収集・再送
- ログ schema migration、破損ログの修復
- 複数プロセスからの同時収集、複数 Human Task の統合
- exactly-once、永続 queue、resume

## 3. Non-goals

- Human Task の completion status、成功／失敗／blocked の推定
- manual summary、理由、outcome の追加入力
- LLM による要約、分類、完了判定
- stdout / stderr output preview またはログ本文の Evidence 化
- Evidence の永続化、履歴、検索、resume
- side agent、別 agent loop、複数 Human Task coordination
- `aish` への**新規** hook / 環境変数 / ログ event（既存 0049 replay hook の exit_code 捕捉・bounded sync emit 修正は §1.5 で許可）
- 0060 の `collab_outcome` または関連 DTO の再導入

## 4. Complexity inventory

| 項目 | 数・内容 |
|------|----------|
| 実行主体 | 1（既存 `ai` の post-handoff observer。新規主体なし） |
| 状態機械 | 0 |
| 永続 aggregate | 0 |
| 外部副作用 | 1（既存 `shell_session_dir/log.jsonl` の bounded read） |
| プロセス境界 | 1（既存 `ai` → 親 agent の protocol 応答） |
| 新規基盤機構 | 0（既存 replay span と observer の合成） |
| 他機能統合 | 3（0049 command span、0055 human handoff、0060 briefing） |

`scripts/feature-scope.toml` の `0061` entry と一致させる。

## 5. Complexity Gate

- 判定: Yellow（承認済み）
- 理由: 新規主体・状態機械・永続化・novel mechanism はないが、既存 0049 / 0055 / 0060 の 3 機能統合が Yellow 閾値に達する
- 分割判断: completion status、manual summary、LLM summarization、persistence、resume、side agent、output previews、multiple Human Tasks は本 spec から除外する
- 承認例外: 不要（Red ではない）。`scope_review = "approved"` を registry に記録する

## 6. Complexity budget

| 項目 | 上限 |
|------|------|
| 新規実行主体 | +0 |
| 状態機械 | +0 |
| 永続 aggregate | +0 |
| external effect | +0（既存 log read のみ） |
| process boundary | +0 |
| novel mechanism | +0 |
| agent loop / side agent | +0 |
| aish hook / env / log schema | +0 |

## 7. Split triggers

次が必要になったら STOP-THE-LINE し、0061 に追加せず別 spec へ分割する。

- completion status、manual summary、LLM summarization、output preview
- Evidence の永続化、resume、schema migration、crash recovery
- side agent、二つ目の agent loop、複数 Human Task coordination
- lease、heartbeat、reconciler、exactly-once
- 新しい実行主体、状態機械、process boundary、ログ event
- 既存 0049 replay hook の exit_code/FIFO 修正を超える span semantics の再設計

## 8. パック構成の適用

**No** — 本機能は Collaborative Mode の post-handoff observation が常に同じ安全な構造化事実を返すための core 観測拡張であり、0045 §6 の optional 配備、runtime toggle、重い依存、独立 RPC / CLI には該当しない。既存 `ProcessEnvironmentObserver` と protocol DTO の一貫した契約であるため、Pack 境界、Active Pack、Basic Pack は作らない。

## 9. Acceptance Criteria

| ID | 条件 |
|----|------|
| `human_task_evidence_is_collected_automatically` | Human Shell 終了後、ユーザーへ summary や outcome の追加入力を要求せず、対象 command span から Human Task Evidence を自動収集して親への observation に含める |
| `human_task_evidence_contains_commands_and_exit_codes` | 各 Evidence が記録済み command と span の `exit_code` を構造化して保持し、exit code がない場合も推定値で補わない |
| `human_task_evidence_uses_handoff_log_range` | `shell_session_dir/log.jsonl` の `shell_log_start..shell_log_end` だけを対象とし、range が 8 MiB を超える場合は末尾 8 MiB を読み、先頭部分行を破棄して部分 Evidence を成功扱いする |
| `human_task_evidence_reuses_replay_spans` | command span の対応付けに `aish_replay::replay_span_views` を再利用し、完了済み span だけを Evidence 化する |
| `human_task_evidence_excludes_non_shell_spans` | `ReplaySpanView.kind` が Shell でない span と未完了 span を Evidence から除外する |
| `human_task_evidence_is_bounded` | 最大 50 commands、2 KiB/command、16 KiB total、8 MiB scan の全上限を超えず、文字列は UTF-8 安全に末尾 `…` 付きで切り詰める |
| `human_task_evidence_keeps_recent_commands` | command 件数、1 command、合計 byte、scan range の各上限超過またはscan先頭部分行破棄で `truncated = true` とし、直近の完了 Shell commands を優先して選択後は時系列順で返す |
| `human_task_evidence_preserves_redaction` | log / replay span に記録済みの sanitized / redacted command のみを使い、秘密値や元 command を復元しない |
| `human_task_evidence_distinguishes_empty_from_unavailable` | 収集成功で command なしは `Some(empty)`、command ありは `Some(commands)`、収集失敗は `None` として区別する |
| `human_task_evidence_failure_is_nonfatal` | 収集失敗を `human_task_evidence_log_unavailable` / `human_task_evidence_invalid_log` / `human_task_evidence_invalid_range` で記録し、handoff と他の observation を失敗させない |
| `human_task_evidence_requires_no_manual_summary` | Evidence 収集のための summary、理由、status、outcome の手入力を Human Shell 終了後に要求しない |
| `human_task_evidence_does_not_infer_completion` | command や exit code から Human Task または requested command の完了・成功・失敗を推定せず、`requested_command_completion = unknown` を維持する |
| `human_task_evidence_protocol_is_backward_compatible` | `PostHandoffObservation.human_task_evidence` は欠落 payload を受理し、`None` を省略でき、旧 payload の deserialize と既存 field の wire 表現を維持する |
| `human_task_evidence_normal_mode_regression` | 通常 shell_exec、非 Collaborative Mode、既存 shell log tail と post-handoff observation の挙動を変更しない |
| `human_task_evidence_vertical_e2e` | Human Shell 内の複数 command 実行から終了、range 読取、完了 Shell span 選別、structured Evidence の親 synthetic tool result 返却までが追加入力なしに一貫して動作する |

各 row は `scripts/spec-acceptance.toml` と 1:1 に登録する。`status = draft` の設計 step では Scope Lock をまだ固定せず、実装開始時に `locked_ac_ids` を固定する。

## 10. Deferred specs

- completion status と自動完了判定
- manual summary / outcome と LLM summarization
- stdout / stderr output previews
- Evidence の永続化、resume、履歴、検索
- side agent と複数 Human Task coordination

## 11. Scope change log

| Revision | 分類 | 変更 | 理由 |
|----------|------|------|------|
| 1 | INITIAL | command span から bounded structured Human Task Evidence を自動収集する設計を draft 登録 | 0049 / 0055 / 0060 の既存契約を再利用し、追加入力と完了推定なしに親へ観測事実を返すため |
| 6 | CONTRACT | 既存 0049 replay hook の exit_code 捕捉と bounded wait FIFO emit 修正を §1.5 で明示許可 | Evidence AC が正しい command_end.exit_code に依存するため。新 hook / 新 FIFO / 新 event は追加しない。`timeout` 非依存で読み手不在時も無限ブロックしない |
