# AGENTS.md - AI開発エージェント向けプロジェクト知識ベース

## ⚠️ 作業前の必須確認

- コード編集・追加・修正の**前**にこのAGENTS.mdを読む
- **作業前**: `./test_integration.sh` で全テスト成功を確認
- **作業後**: `./test_integration.sh` で既存機能が壊れていないことを確認

---

## アーキテクチャ（逆流禁止の判断基準）

### 1. 依存方向（一方向のみ許可）

```
  CLI（main + cli） → usecase → ports（outbound） ← adapters（実装）
                              ↑
                         wiring がここで「adapter を usecase に注入」
```

- **CLI**: `main.rs` と `cli/`。引数解析・用法表示・エラー表示・終了コード。`parse_args()` で `Config` を取得し、**wiring で組み立てた App の Run だけを呼ぶ**。
- **usecase**: アプリの手続き（`AiUseCase`, `TaskUseCase`, `ShellUseCase` 等）。**port（trait）経由でのみ** I/O や外界に触れる。
- **ports**: インターフェース定義。usecase は「outbound」の trait にのみ依存する。adapter はその trait を実装する。
- **adapters**: 具体実装（FS・プロセス・LLM・承認・タスク実行等）。**wiring 以外から生成・参照されない**。
- **wiring**: 唯一の「composition root」。adapter を生成し、usecase に trait として渡す。main は「コマンド分岐」と「wiring の呼び出し」のみ行う。

許可される依存の向きだけを書く：

- `main` → `cli`, `wiring`, `ports::inbound`
- `wiring` → `adapter`, `usecase`, `ports::outbound`（および common）
- `usecase` → `ports::outbound`, `domain`, `common` のみ
- `adapter` → `ports::outbound`（実装）、`common` 等

### 2. usecase 層の禁止事項（明文化）

usecase モジュール（`core/ai/src/usecase/`, `core/aish/src/usecase/`）では以下を**禁止**する。

- **adapter を import しない**  
  `use crate::adapter::*` や `use crate::adapter::StdTaskRunner` 等は書かない。必要なのは port（trait）だけ。
- **cli に依存しない**  
  `use crate::cli::*` は禁止。Config 等は main が解釈し、usecase には「必要な値」だけを引数で渡す。
- **std::env を直接読まない**  
  環境変数・カレントディレクトリは adapter や cli で読み、usecase には `SessionDir` 等の値として注入する。
- **stdout / stderr に直接出力しない**  
  `println!` / `eprintln!` / `std::io::stdout()` 等は usecase では使わない。表示は port（例: `EventSinkFactory`）や cli の責務。
- **wiring に依存しない**  
  `use crate::wiring::*` は禁止。usecase は「trait を受け取って動く」だけであり、誰がその実装を渡すかは知らない。

### 3. wiring（composition root）の責務

- **adapter の生成は wiring のみが行う**  
  `StdTaskRunner::new(...)`, `PartSessionStorage::new(...)` 等、具象アダプタの `new` / ファクトリは `wiring.rs` 内だけに書く。
- **usecase は trait（port）だけを受け取る**  
  `AiUseCase::new(fs, history_loader, response_saver, ...)` のように、引数はすべて `Arc<dyn SomePort>` などの trait 型。wiring が adapter を `Arc<dyn SomePort>` にしたうえで渡す。
- **main の役割はコマンド分岐と wiring 呼び出しのみ**  
  `let config = parse_args()?;` → `let app = wire_ai();`（または `wire_aish()`）→ `Runner { app }.run(config)` のような流れにする。ビジネスロジックや I/O の詳細は main に書かない。

### 4. Inbound / Outbound port の役割

- **Inbound port（ドライバ → アプリ）**  
  呼び出し側（main）がアプリを実行するためのインターフェース。例: `UseCaseRunner::run(&self, config: Config) -> Result<i32, Error>`。main は `config_to_command(config)` で Command にし、`match` で分岐したうえで、各 usecase や `app.run_query` を呼ぶ。usecase は **inbound を実装しない**（main 側の `Runner` が実装する）。
- **Outbound port（アプリ → 外界）**  
  usecase が「ファイル」「プロセス」「LLM」「承認」「タスク実行」等を使うための trait。例: `SessionHistoryLoader`, `TaskRunner`, `ToolApproval`, `EventSinkFactory`。usecase はこれらの **trait にのみ依存**し、実装（adapter）は wiring が注入する。

### 5. 実装時のチェックリスト

コードを書いたら、以下を確認する。

- [ ] このコードは **usecase から adapter を参照していないか？**（`grep -r "crate::adapter" core/ai/src/usecase core/aish/src/usecase` が空であること）
- [ ] **stdout / stderr を usecase で触っていないか？**（`println!` / `eprintln!` / `std::io::stdout` 等が usecase に無いこと）
- [ ] **env を usecase で直接読んでいないか？**（`std::env::var` / `std::env::current_dir` 等が usecase に無いこと）
- [ ] **usecase が cli や wiring に依存していないか？**（`use crate::cli` / `use crate::wiring` が usecase に無いこと）
- [ ] **adapter の new / 生成は wiring にだけあるか？**（main や usecase から adapter を `new` していないこと）
- [ ] **main は「parse_args → wire → Runner.run」以外のロジックを持っていないか？**

---

## プロジェクト概要・構造

- **AISH**: CUI 自動化フレームワーク（LLM 連携）。シェルスクリプトから Rust への刷新中。
- **core/common**: `ai` / `aish` 共通。エラー型、session、LLM ドライバ・プロバイダ、Part ID、Port trait（FileSystem, Process, Clock 等）と標準実装、Tool trait / ToolRegistry。**ai 専用・aish 専用のユースケースは置かない。**  
  - Outbound の trait のうち **Tool** と **LlmProvider** は、ドメイン型（ToolContext, Message 等）との循環参照を避けるため、それぞれ `common::tool` と `common::llm::provider` に定義し、`common::ports::outbound` から re-export している。その他の outbound trait は `ports/outbound` に定義。
- **core/ai**: `ai` コマンド。main → cli → wiring → UseCaseRunner。usecase: `app.rs`（AiUseCase）, `task.rs`（TaskUseCase）, `agent_loop.rs`。adapter: sinks, task, part_session_storage, approval, tools 等。
- **core/aish**: `aish` コマンド。main → cli → wiring → UseCaseRunner。usecase: shell, truncate_console_log, clear, sysq 等。adapter: shell, terminal, platform, logfmt, sysq 等。

**システムプロンプト（sysq）**
- 格納場所（優先度 低→高）: グローバル `$AISH_HOME/config/system.d` または `~/.config/aish/system.d`、ユーザー `~/.aish/system.d`、プロジェクト `.aish/system.d`（カレントから遡って探索）。1ファイル1機能（拡張子除く相対パスが ID）。
- 有効/無効: 各スコープの `system.d/enabled` に 1 行 1 ID で列挙されたものが有効。`aish sysq list` で一覧と有効状態表示、`aish sysq enable id [id...]` / `aish sysq disable id [id...]` で切り替え。
- **ai** 実行時: `-S/--system` 未指定のとき、上記で有効な sysq を優先順位でマージし結合した文字列を system instruction として使用。指定時はその文字列をそのまま使用。

ビルド・テストはプロジェクトルートで `./build.sh`, `./test.sh`, `./test_integration.sh`。個別は `cd core/ai && cargo test` 等。

---

## 開発方針（要約）

- **TDD**: 失敗するテストを先に書く → 通す最小実装 → リファクタ。テスト省略禁止。
- **エラー**: usecase 内は `Result<T, common::error::Error>`。CLI 境界で `exit_code()` / `is_usage()` により終了コード・用法表示を決定。
- **common 肥大化防止**: 2 crate 以上で共有され安定したものだけ common に置く。ai 専用・aish 専用は各 crate の adapter / usecase に置く。OS 副作用のある具象ツール実装は `core/*/adapter/` に置く。

---

## 禁止事項

- テストを省略して実装を進めること
- **usecase から adapter / cli / wiring を参照すること**、**usecase で std::env / stdout / stderr を直接使うこと**
- この AGENTS.md を読まずに作業を開始すること

---

## 参照

- 結合テスト: `./test_integration.sh`（作業前後に必須）
- 単体テスト: `./test.sh`
- 既知のバグ: `BUGS.md`
- サブプロジェクト: `tools/aish-capture/AGENTS.md` 等

## 更新履歴

- **2026年2月**: common の port & adapter 整理。adapter から port の re-export を削除し、usecase は `common::ports::outbound` から trait を参照。StdIdGenerator を adapter に移動。Tool / LlmProvider が ports 外に定義されている理由を明記。
- **2026年2月**: システムプロンプト（sysq）を追加。common に `system_prompt`（Scope・マージ）、EnvResolver に `current_dir` / `resolve_global_system_d_dir` / `resolve_user_system_d_dir` を追加。aish に `sysq list` / `sysq enable` / `sysq disable`、ai に `-S` 未指定時の sysq 解決（ResolveSystemInstruction ポート）を実装。結合テストに sysq list を追加。
- **2026年2月**: アーキテクチャを「逆流防止」の判断基準として整理。依存方向・usecase 禁止事項・wiring 責務・inbound/outbound・実装時チェックリストを明文化。長さを抑え実務で参照しやすい形に変更。
- **2026年1月**: common / ai / aish の状態・モジュール・CLI を現状に合わせて見直し。
