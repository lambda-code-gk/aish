# ビルド環境最適化計画（実行タスク詳細）

状態: **実施済み**（2026-07-04）。feature ブランチ `feature/optimize-build-environment` 上で実行。

作成日: 2026-07-04。同日にセルフレビュー済み（T1 の 2 か所管理を撤回し toml 単独正本に修正、
T4 を docs/testing.md 正本のワークスペース全体コマンドに修正、T3 に `save-if` 追加、
T2 に Cargo.lock ガード追加）。調査の前提数値は「§1 現状評価」に記録した。
数値が古くなっていたら再計測してから着手する。

---

## 0. 実行者への前提・制約（必読）

### 守るべき不変条件（最適化しても壊さない）

1. `./scripts/verify.sh` を **単一の完了ゲート** として維持する（分割しても最終的に 1 コマンドで全部走る）
2. **ローカル既定は直列**（`CARGO_BUILD_JOBS=1` / `cargo test -j 1`）。本計画の並列化は **CI 環境のみ** を対象とする（ローカルは RAM 7.6GiB で OOM リスクあり）
3. `aibe-client` テストの分離実行と `--test-threads=1` は維持（mock aibe の socket / プロセス競合対策）
4. クレート境界（`aish ↛ aibe`、`ai ↛ LLM 直叩き`）と `check-architecture.sh` の検査対象を変えない
5. Codex sandbox は `workspace-write` + network off を維持（`check-codex-tooling.sh` が退行検査する）
6. `git commit` / `push` はユーザー明示時のみ。ただし本タスクは feature ブランチ実装なので **WIP commit は可**（`.cursor/rules/05-git-workflow.mdc`）

### プロセス上の注意

- 本計画は **インフラ変更** であり新機能ではない。パック構成（0045）は **適用外**（optional 機能束ではなく、composition root に触れない。設計書を起こす場合はその旨 1 行書く）
- `scripts/spec-acceptance.toml` への AC 追加は不要（機能 AC ではないため）。ただし
  `check-docs-consistency.sh` / `check-codex-tooling.sh` が緑であることは各タスクの完了条件に含む
- 各タスク完了時は `./scripts/verify-targeted.sh` 相当の最小検証 → 全タスク完了時に `./scripts/verify.sh` を 1 回
- shell script を変更したら `bash -n <script>` を必ず通す（`docs/testing.md` の表に準拠）
- **CI の変更（T2/T3/T4/T10）はローカルで完全検証できない**。push はユーザー明示が必要なので、
  yml 変更後は「push 後に Actions で確認が必要」と残リスクに明記して報告する

---

## 1. 現状評価（2026-07-04 計測）

### 計測値

| 項目 | 値 |
|------|-----|
| ローカル rustc | 1.92.0 stable（ピン止めなし） |
| ローカルマシン | 4 コア / RAM 7.6GiB（空き約 4.9GiB） |
| `target/` サイズ | **9.7GB**（debug info 無調整） |
| CI ランナー | `ubuntu-latest`（4 vCPU / 16GB RAM）だが **直列実行**（`VERIFY_PARALLEL` 未設定） |
| CI 構成 | `verify`（timeout 15min）→ `smoke-mock`（timeout 15min、`needs: verify`） |
| CI キャッシュ | `Swatinem/rust-cache@v2` を両 job で使用、**shared-key なし**（job 間でキャッシュ非共有） |
| テスト規模 | integration test 51 ファイル、spec-acceptance 236 ケース（pending 0） |

### 評価（領域別）

| 領域 | 評価 | 根拠 |
|------|------|------|
| 検証ゲート設計 | **良** | verify.sh 単一ゲート + targeted の二段階が一貫。`test-verify-targeted.sh` は fixture ベースで実 cargo を呼ばず低コスト |
| 再現性 | **弱** | `rust-toolchain.toml` なし。ローカル/CI/将来の環境で rustc 差が出うる。Cargo.lock はコミット済みだが CI に `--locked` ガードがなく、依存のサイレント更新を検出できない |
| CI 速度 | **弱** | 16GB RAM のランナーで直列ビルド・直列テスト。verify → smoke-mock でキャッシュ非共有のため再ビルド発生 |
| ビルド肥大 | **弱** | `[profile]` 未調整で target 9.7GB。リンク時間・ディスク・キャッシュ転送量に効く |
| feature 検査 | **穴** | `--no-default-features`（memory-off）ビルドが CI 未実行。basic pack 経路の退行を検出できない |
| targeted の範囲 | **中** | clippy が targeted に含まれず、完了直前の verify.sh で初めて clippy 失敗が出るパターンがある |
| エディタ/体裁 | **軽微** | rustfmt.toml / .editorconfig なし（現状 rustfmt デフォルトで実害は小さい） |
| docs 整合 | **軽微** | `.cursor/rules/00-project.mdc` の verify 内訳に `check-spec-acceptance.py` / `check-codex-tooling.sh` が未記載 |

### 訂正事項（過去の調査レポートに対する）

- 「verify.sh が毎回 `test-verify-targeted.sh` を実行するのがコスト」という指摘は **誤り**。
  同スクリプトは fixture の record-command で cargo を差し替えており、実ビルドは走らない（数秒で完了）。ホットパスから外す必要はない。

---

## 2. タスク一覧（優先度・依存順）

| ID | 優先 | タスク | 依存 | 種別 |
|----|------|--------|------|------|
| T0 | P0 | CI ベースライン計測 | なし | 計測 |
| T1 | P0 | `rust-toolchain.toml` でツールチェーン固定 | なし | 再現性 |
| T2 | P0 | CI で `VERIFY_PARALLEL=1` 有効化 + Cargo.lock ガード | T0, T1 | 速度/再現性 |
| T3 | P0 | CI キャッシュ共有（shared-key）+ 逐次改善 | T0 | 速度 |
| T4 | P1 | feature-off（`--no-default-features`）CI 検査 | T1 | 検査穴 |
| T5 | P1 | `verify-targeted.sh` に clippy を追加 | なし | フィードバック |
| T6 | P1 | Cargo profile 調整（debug info 削減） | T0 | 速度/肥大 |
| T7 | P2 | `rustfmt.toml` / `.editorconfig` の明示化 | なし | 体裁 |
| T8 | P2 | `[workspace.lints]` への lint 集約 | T5 | 保守性 |
| T9 | P2 | CI concurrency（旧 run の自動キャンセル） | なし | 速度 |
| T10 | P2 | cargo-nextest 評価スパイク（採否判断のみ） | T2, T3 | スパイク |
| T11 | P3 | docs / rules の整合修正 | T1–T6 | docs |
| T12 | P3 | pre-commit フック（**要ユーザー判断**） | なし | 任意 |

推奨実行順: T0 → T1 → (T2, T3, T9 を同一 yml 変更でまとめる) → T5 → T6 → T4 → T7 → T8 → T11 →（T10, T12 はユーザー判断）

---

## 3. タスク詳細

### T0: CI ベースライン計測（P0）

**目的**: 改善の前後比較ができないと最適化の成否を判定できない。着手前に現状値を記録する。

**手順**:

（`gh` はネットワークアクセスと GitHub 認証が必要。サンドボックス実行時は full_network 権限で実行する）

1. `gh run list --workflow ci.yml --limit 10 --json databaseId,conclusion,updatedAt` で直近の run を取得
2. 直近成功 run 3 件について `gh run view <id> --json jobs` から `verify` / `smoke-mock` 各 job の所要時間を取得
3. 本ファイルの末尾「付録 A: 計測記録」に、cold cache / warm cache を区別して記録する
4. ローカルでは計測しない（RAM 制約で verify.sh フル実行が長く、比較対象は CI のため不要）

**受け入れ条件**:
- 付録 A に verify / smoke-mock の baseline 所要時間（分単位、warm/cold 区別）が記録されている

**リスク**: なし（読み取りのみ）

---

### T1: `rust-toolchain.toml` でツールチェーン固定（P0）

**目的**: ローカル・CI・後続 AI 環境で rustc バージョンを一致させ、「ローカルで緑・CI で赤」を排除する。

**変更ファイル**:

1. **新規** `rust-toolchain.toml`（リポジトリルート）:

```toml
[toolchain]
channel = "1.92.0"
components = ["rustfmt", "clippy"]
```

（着手時点の最新 stable が 1.92.0 より進んでいたら、そのバージョンでローカル verify.sh を通した上でそちらに固定してよい）

2. `.github/workflows/ci.yml` は **無変更でよい**。根拠（dtolnay/rust-toolchain の action.yml を
   2026-07-04 に確認済み）: 同 action は `rustup toolchain install` + `rustup default` を行うだけで
   **`RUSTUP_TOOLCHAIN` 環境変数を設定しない**。そのため cargo 実行時は rustup のディレクトリ
   オーバーライド（`rust-toolchain.toml`）が default より優先され、ファイルの 1.92.0
   （components 込み）が自動インストール・使用される。バージョンの正本は toml の 1 か所になる。
   - 副作用: CI は action が入れる stable と、ファイルの 1.92.0 の **2 つの toolchain を
     ダウンロードする**（+30 秒程度）。気になる場合のみ action の `toolchain` input を
     `"1.92.0"` にしてよいが、その場合は 2 か所管理になるため、T11 の一致検査（任意）を必ず入れる
   - 検証: CI ログの最終行（`rustc --version`）と、verify job 内の cargo が使う rustc が
     1.92.0 であることを push 後に確認する

3. `docs/testing.md`: 「実行コマンド（標準）」節の近くに、toolchain 固定の存在と更新手順
   （`rust-toolchain.toml` の channel を上げて verify.sh を通す）を 3–5 行で追記

**検証**:

```bash
rustup show          # rust-toolchain.toml が反映されること
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings   # 固定バージョンで clippy が通ること
bash -n .github/workflows/ci.yml 2>/dev/null || true  # yml は yamllint 不在なら目視
./scripts/verify-targeted.sh --docs
```

**受け入れ条件**:
- `rustup show` の active toolchain が 1.92.0 になる
- clippy / fmt が固定バージョンで緑
- 更新手順が docs に存在する

**リスク**: 将来 stable の clippy 新 lint に自動追従しなくなる（意図通り。更新は明示的に行う運用になる）

---

### T2: CI で `VERIFY_PARALLEL=1` を有効化（P0）

**目的**: `ubuntu-latest`（4 vCPU / 16GB RAM）で直列ビルド・直列テストをやめ、CI 時間を短縮する。
ローカル既定（直列）は **一切変更しない**。

**背景**: `scripts/verify.sh` は `VERIFY_PARALLEL=1` で `CARGO_BUILD_JOBS` 固定を解除し、
`cargo test` の `-j 1` を外す。ただし `--test-threads=1` は verify.sh 内にハードコードされており
**並列化されない**（テスト間の HOME / socket 共有があるため、これは維持で正しい）。
つまり効果は主に **コンパイル並列化** に出る。

**変更ファイル**: `.github/workflows/ci.yml` の verify job:

```yaml
      - run: cargo metadata --locked --format-version 1 > /dev/null
      - run: ./scripts/verify.sh
        env:
          VERIFY_PARALLEL: "1"
```

`cargo metadata --locked` は **Cargo.lock ドリフトガード**（本リポジトリは Cargo.lock を
コミット済み）。lockfile が Cargo.toml と不整合なら数秒で失敗し、依存のサイレント更新を
CI で検出できる。ダウンロードは発生しない。

**検証**:
- ローカルでは `VERIFY_PARALLEL=1 VERIFY_SKIP_TEST=1 ./scripts/verify.sh` で静的検査部分だけ動作確認可
  （フル並列テストはローカル RAM 的に走らせない）
- push 後、Actions の verify job 所要時間を T0 のベースラインと比較して付録 A に記録

**受け入れ条件**:
- CI の verify job が成功し、warm cache 時の所要時間がベースラインより短縮（目安 30% 以上。
  未達でも悪化していなければ採用し、実測値を記録する）
- ローカルの既定動作（`VERIFY_PARALLEL` 未設定で直列）が不変（verify.sh の diff がないこと）

**リスク**: 並列コンパイルによる CI ランナーのメモリピーク。16GB あるので現実的リスクは低いが、
OOM で落ちた場合は `CARGO_BUILD_JOBS=2` を env で明示する fallback に切り替える。

---

### T3: CI キャッシュ共有 + 逐次改善（P0）

**目的**: `verify` と `smoke-mock` が別 job で、`Swatinem/rust-cache` の既定キーは job 名を含むため
**キャッシュが共有されず smoke-mock が再ビルドしている**。shared-key で共有する。

**変更ファイル**: `.github/workflows/ci.yml`。verify job:

```yaml
      - uses: Swatinem/rust-cache@v2
        with:
          shared-key: "ci"
          cache-on-failure: true
```

smoke-mock job（**保存はさせない**）:

```yaml
      - uses: Swatinem/rust-cache@v2
        with:
          shared-key: "ci"
          save-if: "false"
```

`save-if: "false"` の理由: shared-key を共有すると両 job が同じキーへ保存を試みて競合する。
verify のビルド成果物（workspace 全体 + clippy）は smoke-mock（aibe / ai のみ）の上位集合
なので、**verify だけが保存し、smoke-mock は復元専用** にするのが正しい構成。
なお cache は run をまたいで共有されるもので、同一 run 内では smoke-mock は
「前回 run の verify キャッシュ」を使う（それでも warm 時はほぼ再ビルドなしになる）。

**補足判断**（実行者が選ぶ。推奨は案 1）:
- **案 1**: shared-key のみ（上記）。構成変更が最小
- **案 2**: smoke-mock を verify job の最終 step に統合し job を 1 本化。
  キャッシュ問題は消えるが「verify 失敗と smoke 失敗の切り分け」が job 単位でなくなる。
  現在の分離は 0014（CI smoke 安定化）の設計なので、統合するなら
  `docs/done/0014_ci-smoke-stabilization-spec.md` との整合を docs に 1 行書く

**検証**: push 後、smoke-mock job のビルド時間が warm cache でほぼゼロ（バイナリ再利用）になることを Actions ログで確認し、付録 A に記録

**受け入れ条件**:
- smoke-mock job の warm 時所要時間がベースライン比で短縮
- `cache-on-failure: true` により、verify 失敗時もキャッシュが保存される

**リスク**: shared-key 共有でキャッシュサイズが増える可能性。GitHub のキャッシュ上限（10GB/repo）に
近づいたら `target/` 削減（T6）とあわせて様子を見る。

---

### T4: feature-off（`--no-default-features`）CI 検査（P1）

**目的**: `aibe` / `ai` は `default = ["memory"]` で、memory-off（basic pack）ビルドは
`docs/testing.md` に手順があるのに CI 未実行。Pack Composition（0045/0038）の
fail-closed 経路が退行しても検出できない穴を塞ぐ。

**変更ファイル**:

1. `.github/workflows/ci.yml` に job 追加（verify と並走させる。`needs` は付けない）:

```yaml
  feature-off:
    name: feature-off
    runs-on: ubuntu-latest
    timeout-minutes: 15
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@master
        with:
          toolchain: "1.92.0"
      - uses: Swatinem/rust-cache@v2
        with:
          shared-key: "feature-off"
          cache-on-failure: true
      - run: cargo build --workspace --no-default-features
      - run: cargo test --workspace --no-default-features -- --test-threads=1
```

   ※ コマンドは `docs/testing.md`（95–96 行、241 行付近の feature matrix 表）の正本と一致させた。
   0038 Phase D の受け入れ条件がこの workspace 全体コマンドで通っている
   （memory 統合テストは `#[cfg(feature = "memory")]` で default build 限定）ため、
   パッケージを絞る必要はない。`dtolnay/rust-toolchain@stable` のままでよい
   （T1 参照: rust-toolchain.toml が優先される）。
   ※ `aibe-client` の直列分離が必要なら verify.sh と同様に
   `--workspace --exclude aibe-client` + 別 step 実行に分ける
   （事前確認で socket 競合による失敗が出た場合のみ）。

2. `docs/testing.md`: 「CI と smoke の役割分担」表に feature-off job の行を追加

**事前確認（重要）**: ローカルで一度だけ次を直列実行し、そもそも緑になるか確認する。
赤の場合は **本タスクを中断してユーザーに報告**（basic pack の退行が既に存在する可能性があり、
修正は別タスクとして切り出す）:

```bash
cargo build --workspace --no-default-features -j 1
cargo test --workspace --no-default-features -j 1 -- --test-threads=1
```

**受け入れ条件**:
- ローカルで上記 3 コマンドが緑
- CI に feature-off job が存在し、docs/testing.md と整合
- `./scripts/verify-targeted.sh --docs` が緑

**リスク**: memory-off 時のみ出る clippy/警告。`cargo check` は `-D warnings` を付けていないので
まず通し、警告ゼロ化は必要なら別タスクにする。

---

### T5: `verify-targeted.sh` に clippy を追加（P1）

**目的**: 現在 targeted は `fmt → check → test` で clippy を含まず、完了直前の verify.sh で
初めて clippy 失敗が発覚する後戻りがある。パッケージ単位の clippy を targeted に入れる。

**変更ファイル**:

1. `scripts/verify-targeted.sh`: パッケージ検証パスで `cargo check -p <pkg>` の**代わりに**
   `cargo clippy -p <pkg> -- -D warnings` を実行する（clippy は check を内包するため、
   両方走らせると二度手間になる。置き換えが正しい）。
   環境変数フック（`VERIFY_TARGETED_CARGO`）経由の呼び出し構造は維持する。

2. `scripts/test-verify-targeted.sh`: 期待コマンド列のアサーションを更新。例:

```
cargo fmt --all -- --check
cargo clippy -p aibe -- -D warnings
cargo test -p aibe --test agent_turn_loop -j 1
```

   全 assert_log（`--package` を使う全ケース）を書き換えること。1 つでも漏れると
   verify.sh の step 2 で落ちる。

3. `docs/testing.md`: 「パッケージ検証では `cargo fmt`、対象パッケージの `cargo check`、…」の
   記述を clippy に更新（この行は本文 §二段階検証 にある）

**検証**:

```bash
bash -n scripts/verify-targeted.sh
bash -n scripts/test-verify-targeted.sh
./scripts/test-verify-targeted.sh          # fixture テストが緑
./scripts/verify-targeted.sh --package aibe-protocol   # 実パッケージで動作確認（最小クレート）
./scripts/verify-targeted.sh --docs
```

**受け入れ条件**:
- fixture テスト緑、実行例が clippy を含む
- docs/testing.md の記述が実装と一致（`check-docs-consistency.sh` 緑）

**リスク**: clippy は check よりわずかに遅い。ただし warm cache では差は小さく、
後戻り削減の利益が上回る。体感で問題が出たら `--fast`（check のみ）フラグ追加を検討事項として残す。

**既知のギャップ（本タスクのスコープ外・記録のみ)**: verify.sh の
`cargo clippy --workspace -- -D warnings` は `--all-targets` を付けておらず、
**テストコードは clippy 対象外**。targeted も同じ挙動に揃える（一貫性優先）。
`--all-targets` 化は既存テストコードに新規警告が出る可能性があるため、
やるなら別タスクとして切り出し、verify.sh と targeted を同時に変更する。

---

### T6: Cargo profile 調整 — debug info 削減（P1）

**目的**: `target/` 9.7GB の主因は dev/test プロファイルの full debug info。
リンク時間・ディスク・CI キャッシュ転送量を削減する。

**変更ファイル**: ルート `Cargo.toml` に追記:

```toml
[profile.dev]
debug = "line-tables-only"

[profile.dev.package."*"]
debug = false
```

- `line-tables-only`: バックトレースの行番号は保持され、デバッガでの変数展開だけが失われる。
  このプロジェクトはテスト駆動でデバッガ利用が薄い想定のため妥当。もし実行者が
  デバッガ利用の形跡（docs/manual 等）を見つけたら `debug = 1` に緩める
- 依存クレート（`.package."*"`）は debug 情報自体不要。`"*"` ワイルドカードは
  **ワークスペースメンバーには適用されない**（cargo の profile override 仕様）ため、
  自クレートの行番号は `[profile.dev]` の line-tables-only が生きる。
  この前提は下記検証の「バックトレースに行番号」確認で機械的に裏を取ること
  （もし行番号が消えていたら `"*"` の適用範囲の理解が誤っているので、
  `.package."*"` 行を削除して debug 削減は members のみで妥協する）

**検証**:

```bash
cargo clean
cargo build --workspace -j 1        # ローカルは直列を維持
du -sh target                        # 9.7GB からの削減幅を付録 A に記録
cargo test -p aibe-protocol -j 1     # 最小クレートでテスト動作確認
```

その後、通常の開発フローで panic 時のバックトレースに行番号が出ることを任意のテスト失敗で確認。

**受け入れ条件**:
- `target/` サイズが有意に削減（目安 30% 以上、実測を記録）
- `./scripts/verify.sh` が緑（フル実行。時間がかかるため本タスク完了時のみ）
- パニックバックトレースに行番号が保持される

**リスク**: デバッガ（gdb/lldb）での変数インスペクションが劣化する。
docs/testing.md か本ファイルに「デバッガを本格的に使う場合は `debug = 1` に戻す」と 1 行残す。

---

### T7: `rustfmt.toml` / `.editorconfig` の明示化（P2）

**目的**: 現状は rustfmt デフォルトに暗黙依存。デフォルト運用を **明示** し、
非 Rust ファイル（md / toml / yml / sh）の体裁を .editorconfig で固定する。

**変更ファイル**:

1. **新規** `rustfmt.toml`:

```toml
edition = "2021"
```

（意図的にこれだけにする。スタイルは rustfmt デフォルトを正とし、既存コードの再フォーマット diff を発生させない）

2. **新規** `.editorconfig`:

```ini
root = true

[*]
charset = utf-8
end_of_line = lf
insert_final_newline = true
trim_trailing_whitespace = true

[*.rs]
indent_style = space
indent_size = 4

[*.{toml,yml,yaml,json,md,sh}]
indent_style = space
indent_size = 2

[*.md]
trim_trailing_whitespace = false
```

**検証**:

```bash
cargo fmt --all -- --check   # 差分ゼロ（rustfmt.toml がデフォルト挙動を変えないこと）
git diff --stat               # 既存ファイルへの変更が発生していないこと
```

**受け入れ条件**: `cargo fmt --check` が差分ゼロのまま緑

**リスク**: なし（挙動を変えない明示化のみ）。`rustfmt.toml` に edition 以外を足すと
既存コード全体に diff が出るので **足さない**。

---

### T8: `[workspace.lints]` への lint 集約（P2）

**目的**: 現在 clippy 方針は「CLI の `-D warnings`」のみで、エディタ（rust-analyzer)上では
warning のまま見える。workspace lints に集約すると設定が 1 か所になり、エディタと CI の
表示が一致する。

**変更ファイル**:

1. ルート `Cargo.toml`:

```toml
[workspace.lints.rust]
warnings = "deny"
```

2. 全 6 クレートの `Cargo.toml` に追記:

```toml
[lints]
workspace = true
```

3. `scripts/verify.sh` / `verify-targeted.sh` / `docs/testing.md` の
   `-D warnings` は **残してよい**（二重指定は無害で、スクリプト単体でも意図が読める）

**注意**: `warnings = "deny"` を lints に入れると **通常の `cargo build` でも warning がエラー化**
する。開発中の一時的 warning（未使用 import 等）で build が止まるのが煩わしい場合は、
この案を採らず「`[workspace.lints.clippy]` で個別 lint のみ集約」に縮小してよい。
**採否は実行時にユーザーへ 1 度確認する**（開発体験に直結するため）。

**検証**:

```bash
cargo clippy --workspace -- -D warnings
cargo build --workspace -j 1
./scripts/verify.sh   # 本タスク完了時
```

**受け入れ条件**: verify.sh 緑、6 クレートすべてに `[lints] workspace = true` がある

**リスク**: 上記の warning エラー化による開発中の煩雑さ。ユーザー確認を挟むこと。

---

### T9: CI concurrency 設定（P2）

**目的**: 同一 PR への連続 push で旧 run が走り続けるのを止め、Actions のキュー消費を削減する。

**変更ファイル**: `.github/workflows/ci.yml` のトップレベルに追加:

```yaml
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: ${{ github.ref != 'refs/heads/main' }}
```

（main への push は cancel しない: main の履歴は全 commit 検証を保つ）

**検証**: yml 構文確認 + push 後に PR へ 2 連続 push して旧 run がキャンセルされることを確認

**受け入れ条件**: PR 上で旧 run が自動キャンセルされる

**リスク**: なし

---

### T10: cargo-nextest 評価スパイク（P2 / 採否判断のみ）

**目的**: nextest はテストごとにプロセス分離するため、「テスト間の HOME / socket 競合で
`--test-threads=1` を強制している」現状制約を、プロセス分離 + 並列実行で緩和できる
**可能性** がある。ただし外部ツール依存が増えるため、計測して採否を判断する（導入を約束しない）。

**手順**:

1. ローカルに `cargo binstall cargo-nextest`（または `cargo install --locked cargo-nextest`）
2. 直列比較: `time cargo nextest run --workspace --exclude aibe-client -j 1` vs
   既存 `time cargo test --workspace --exclude aibe-client -j 1 -- --test-threads=1`
3. 並列挙動確認（**CI 相当環境が理想。ローカルでやるなら RAM を監視しながら** `-j 2` まで）:
   `cargo nextest run -p aibe -j 2` 等でテスト失敗が出ないか
4. `aibe-client` は mock aibe spawn があるため、nextest でも直列指定
   （`--test-threads 1` 相当の profile 設定）が必要かを確認
5. 結果を本ファイル付録 B に記録し、**採用する場合は別タスクとして切り出す**
   （verify.sh の変更、CI への nextest インストール step、docs/testing.md 更新が必要になるため）

**受け入れ条件**: 付録 B に計測結果と採否判断（理由付き）が記録されている。コード変更はしない。

**リスク**: なし（評価のみ。リポジトリへの変更は付録追記だけ）

---

### T11: docs / rules の整合修正（P3）

**目的**: 本計画の変更を docs に同期し、既知の軽微な不整合を直す。

**変更内容**:

1. `.cursor/rules/00-project.mdc`: verify.sh の内訳記載
   `（内訳: fmt / clippy / test / check-architecture.sh / check-docs-consistency.sh）` に
   `check-spec-acceptance.py` と `check-codex-tooling.sh` を追記（AGENTS.md とは既に一部整合、
   両方を verify.sh の実体と一致させる）
2. `docs/testing.md`: T2/T3/T4/T5/T6 で触れた各節の最終確認（各タスク内で更新済みのはずだが、
   全タスク完了後に通しで読み直す）
3. **条件付き**: T1 で ci.yml 側にもバージョンを明示した場合（既定では不要）のみ、
   `scripts/check-docs-consistency.sh` に「`rust-toolchain.toml` の channel と ci.yml の
   toolchain 指定の一致」検査を追加する。T1 既定（toml のみが正本、ci.yml 無変更）なら
   本項はスキップ
4. AGENTS.md は verify.sh の内訳が既に正しいか確認し、必要なら同期

**検証**:

```bash
./scripts/verify-targeted.sh --docs
bash -n scripts/check-docs-consistency.sh
./scripts/verify.sh   # 全タスクの最終ゲート
```

**受け入れ条件**: verify.sh 全体が緑。rules / AGENTS.md / testing.md の verify 内訳が実体と一致

---

### T12: pre-commit フック（P3 / **要ユーザー判断・既定は見送り**）

**背景**: 現在フックなし。プロジェクトルールは「commit はユーザー明示時のみ」で、
コミット頻度が低い運用のためフックの利益は限定的。また AI エージェントの WIP commit を
フックで遅くする副作用がある。

**実行条件**: **ユーザーが明示的に希望した場合のみ** 実施。希望がなければ本タスクはスキップし、
完了報告に「T12 は見送り（要ユーザー判断）」と書く。

**実施する場合の設計**（軽量に留める）:

1. **新規** `.githooks/pre-commit`（`cargo fmt --all -- --check` のみ。clippy/test は入れない — WIP commit を止めないため）
2. 有効化は各自 opt-in: `git config core.hooksPath .githooks`（自動設定しない）
3. `docs/testing.md` に opt-in 手順を追記

---

## 4. 実行しないと決めたこと（検討済み・却下）

後続 AI が再検討して蒸し返さないよう、却下理由を残す。

| 項目 | 却下理由 |
|------|----------|
| ローカル既定の並列化 | ローカルは RAM 7.6GiB / 空き 4.9GiB。OOM 実績があるからこその現行設計。CI のみ並列化する |
| `test-verify-targeted.sh` のホットパス除外 | fixture ベースで実コストが数秒。除外の利益がない（§1 訂正事項参照） |
| sccache / リモートキャッシュ導入 | 個人開発 + Swatinem cache で十分。運用コストが利益を上回る |
| mold/lld リンカ強制 | 環境依存（要 apt install）が増える。T6 の debug info 削減で先にリンク負荷を下げ、なお遅ければ将来検討 |
| CI の macOS / Windows matrix | プロジェクトは Unix 専用（Windows スコープ外）。macOS は現状ユーザー環境にない |
| smoke-mock の並列化 | mock aibe の起動・socket 待ちが本質時間で、並列化の余地が小さい。T3 のキャッシュ共有で十分 |
| `--test-threads` の並列化（cargo test のまま） | テストが HOME / socket / env を共有する設計。プロセス分離なしの並列化は flaky 化リスクが高い。可能性は T10（nextest）で評価 |

## 5. 完了の定義（本計画全体）

1. T0–T9, T11 が各タスクの受け入れ条件を満たす（T10 は付録 B 記録、T12 はユーザー判断）
2. `./scripts/verify.sh` が緑
3. CI（push 後）の verify / smoke-mock / feature-off が緑 — **push はユーザー明示が必要**なので、
   未 push の場合は残リスクとして報告する
4. 付録 A に before/after の計測値が記録されている
5. 本ファイルを `docs/todo/` から削除または「実施済み」に更新し、変更内容は
   `docs/testing.md` 等の正本 docs に反映されている（todo は正本にしない）

## 付録 A: 計測記録（実行者が記入）

| 計測項目 | before | after | 備考 |
|----------|--------|-------|------|
| CI verify（warm） | 53s（run 27305627137, 2026-06-10） | （push 後に記入） | T0 / T2。直近 main は失敗多く warm 成功 run は古い |
| CI verify（cold） | 未計測（成功 run は cache warm 前提） | （push 後に記入） | T0 / T2 |
| CI smoke-mock（warm） | 22s（run 27305627137） | （push 後に記入） | T0 / T3 |
| `target/` サイズ（clean `cargo build`） | 9.7GB | 9.7GB | T6。profile 変更後も dev ビルド単体では同等。主因は未最適化バイナリ本体（deps 6.5GB）。`incremental/`（テスト後 3.1GB）は profile.test 追加で今後抑制見込み |

## 付録 B: nextest スパイク記録（T10 実行者が記入）

**採否: 見送り（評価未実施）**

- 理由: 実行環境に `cargo-nextest` が未インストール。ネットワーク経由の `cargo install` は本タスクのスコープ外（外部ツール導入の判断をユーザーに委ねる）
- 再評価時: `docs/testing.md` に手順を追記してから別タスクで計測する
