# AGENTS.md - AI開発エージェント向けプロジェクト知識ベース

## ⚠️ 重要: 作業前の必須確認事項

**このプロジェクトで作業を開始する前に、必ずこのAGENTS.mdを最初に読んでください。**

Cursor AIやその他のAI開発エージェントは、このプロジェクトでコードを編集、追加、修正する前に、以下の手順を必ず実行してください：

1. **このAGENTS.mdを最初に読む**: プロジェクトの構造、開発方針、約束事を理解する
2. **関連するサブプロジェクトのAGENTS.mdを確認**: 作業対象のサブプロジェクトがある場合は、そのAGENTS.mdも確認する
3. **既存のコードとテストを確認**: 実装前に既存のコードベースとテスト構造を理解する
4. **作業前の確認**: 作業開始前に結合テストを実行し、現在の状態が正常であることを確認する
5. **このドキュメントの指示に従う**: 記載されている開発方針、テスト方針、実装方針に従って作業する

**作業前後の確認手順**:
- **作業前**: `./test_integration.sh`を実行し、すべてのテストが成功することを確認する
- **作業後**: `./test_integration.sh`を実行し、変更によって既存の機能が壊れていないことを確認する

**このルールを無視して作業を開始しないでください。**

---

このドキュメントは、このプロジェクトにおけるAI開発エージェント（Cursor AI等）が参照すべき約束事、実行方法、実装方針を記録しています。

## プロジェクト概要

- **メインプロジェクト**: AISH（AISH is a CUI automation framework powered by LLMs）
- **プロジェクトルート**: git worktree等で複数箇所に展開されます。pwdで確認して下さい。
- **現在の状態**: シェルスクリプトからRustへの刷新作業中。`core/ai` と `core/aish` は主要機能の実装が進んでおり、LLM連携・タスク実行・セッション・対話シェルなどが動作する。

## プロジェクト構造

### コア実装（Rust）

#### `core/common/`
- **目的**: `ai`と`aish`コマンドの共通処理を提供するライブラリ
- **ディレクトリ**: `core/common/`
- **ビルド方法**: `cd core/common && cargo build --release`
- **テスト実行**: `cd core/common && cargo test`
- **状態**: 実装済み。LLMドライバ・プロバイダ・セッション・Part IDを提供
- **依存関係**: `serde_json`, `reqwest`（LLM API用。blocking, json, rustls-tls）
- **提供機能**（層: domain / adapter / llm / msg / tool / sink 等）:
  - **error**: 統一エラー型 `Error`（thiserror による enum）。境界で `exit_code()` / `is_usage()` により終了コード・用法表示を決定
  - **domain**: ドメイン型（Newtype 等）
  - **session**: セッションディレクトリの解決・管理（AISH_HOME / XDG 準拠）。OS/env/fs 依存のため adapter 寄りだが共通のため common に配置
  - **adapter**: Port trait（FileSystem, Process, Clock, Pty, Signal 等）と標準実装（Std*）
  - **llm**: LLMドライバとプロバイダ
    - **driver**: ストリーミング対応の共通ドライバ
    - **provider**: メッセージ型・プロバイダトレイト
    - **gemini**: Gemini API（Google Search grounding 対応）
    - **gpt**: OpenAI Responses API
    - **echo**: クエリをそのまま返すテスト用
    - **factory**: ProviderType（Gemini / Gpt / Echo）、create_provider / create_driver
  - **part_id**: 8文字 base62 のPart ID生成。辞書順＝時系列。同一ms内はシーケンスで単調増加
  - **msg**: 型付きメッセージ履歴（Msg）
  - **tool**: ツール実行（Ports & Adapters）。`Tool` トレイト（name / description / parameters_schema / call）、`ToolRegistry`（register / list_definitions / call）、`ToolDef`（LLM API 用の name・description・parameters）。ツール追加時は common に `impl Tool` を追加し、ai の `run_query` 内で `registry.register(Arc::new(MyTool::new()))` するだけ。
  - **sink**: イベント Sink（表示・保存の分離）

#### `core/ai/`
- **目的**: `ai`コマンドのRust実装（LLM対話・タスク実行）
- **ディレクトリ**: `core/ai/`
- **ビルド方法**: `cd core/ai && cargo build --release`
- **テスト実行**: `cd core/ai && cargo test`
- **依存関係**: `core/common` のみ（標準ライブラリで引数解析）
- **状態**: 実装済み。クエリ対話・プロバイダ指定・タスク実行・セッション履歴の読み書きが動作
- **モジュール（層分割）**:
  - **main.rs**: CLI境界（薄い）。エントリーポイント、エラーハンドリング、usage表示
  - **cli/**: 引数解析。`args.rs`（`Config`: help, provider, task, message_args）
  - **usecase/**: アプリ用ユースケース。`app.rs`（AiUseCase, run_app）、`agent_loop.rs`（状態機械, msgs_to_provider）
  - **adapter/**: 具体実装。`sinks.rs`（StdoutSink 等）、`task.rs`（task.d 解決・実行）
- **CLI**: `-h`/`--help`, `-p`/`--provider <name>`, 位置引数は「タスク名」または「クエリ＋メッセージ」として解釈
- **タスク**: `AISH_HOME/config/task.d/` を最優先、次に `XDG_CONFIG_HOME/aish/task.d/`。`task_name.sh` または `task_name/execute` を実行
- **セッション**: 環境変数 `AISH_SESSION` でセッションディレクトリを指定。part ファイルから履歴を読み込み、応答を part ファイルに追記
- **エージェント・ツール**: LLM がツールを呼び出せる。Gemini / GPT にはツール定義（ToolDef）をペイロードで渡し、ストリームで ToolCall イベントを受けて `ToolRegistry::call` で実行し、結果を履歴に載せて再呼び出し。Echo プロバイダは「call:ツール名」や「call:ツール名 {...}」でツール呼び出しをシミュレート。新規ツール追加は「common に Tool 実装を追加 → ai の run_query で registry.register」のみ。

#### `core/aish/`
- **目的**: `aish`コマンドのRust実装（対話シェル・セッション・ログ）
- **ディレクトリ**: `core/aish/`
- **ビルド方法**: `cd core/aish && cargo build --release`
- **テスト実行**: `cd core/aish && cargo test`
- **依存関係**: `core/common`, `libc`, `base64`, `serde_json`
- **状態**: 一部実装済み。対話シェル・セッション管理・ログロールオーバー・未実装コマンドのエラー処理が動作。resume/memory/models 等は未実装
- **モジュール（層分割）**:
  - **main.rs**: CLI境界（薄い）。エントリーポイント、エラーハンドリング
  - **cli/**: 引数解析。`args.rs`（`Config`: help, session_dir, home_dir, command, command_args）
  - **usecase/**: コマンド振り分け・usage。`app.rs`（run_app、未実装コマンドは 64 で終了）
  - **adapter/**: 具体実装。`aish_adapter.rs`（platform の Pty/Signal を common にラップ）、`shell.rs`、`terminal.rs`、`logfmt.rs`、`platform/`（Unix: シグナル、SIGUSR2 等）
- **CLI**: `-h`/`--help`, `-s`/`--session-dir <dir>`, `-d`/`--home-dir <dir>`, 位置引数でコマンドと引数
- **実装済みコマンド**: なし（対話シェル）、`truncate_console_log`（コンソールバッファ・ログのロールオーバー）
- **未実装コマンド**: resume, sessions, rollout, clear, ls, rm_last, memory, models（いずれも 64 で終了し usage に記載）
- **セッション解決**: セッションディレクトリの優先順位は次のとおり。デフォルト（`-s` 未指定）では毎回ユニークなセッションIDを生成し、複数起動が同じ console/part に混ざらないようにする。`-s` 指定時のみ同一ディレクトリで再開できる。
  1. **CLI `-s/--session-dir`**: 指定ディレクトリをそのまま使用（再開用）
  2. **環境変数 AISH_HOME / `-d/--home-dir`**: その配下の `state/session/{ユニークID}` を生成
  3. **XDG_STATE_HOME**: 未設定時は `~/.local/state` の `aish/session/{ユニークID}`

### 既存ツール（Rust実装済み）

#### `tools/aish-capture/`
- **目的**: `script`コマンドの代替となる軽量なPTYキャプチャツール（Rust実装）
- **詳細ドキュメント**: [`tools/aish-capture/AGENTS.md`](tools/aish-capture/AGENTS.md)

#### `tools/aish-script/`
- **目的**: expect機能を提供するスクリプト実行ツール（Rust実装）
- **機能**: JSONLログファイルを監視し、パターンマッチングに基づいてFIFO経由で入力を送信

#### `tools/aish-render/`
- **目的**: ターミナル出力のレンダリングツール（Rust実装）

#### `tools/leakscan/`
- **目的**: 機密情報検出（Gitleaks風）。検出理由の表示など。

### 旧実装（参考用）

- **ディレクトリ**: `old_impl/`
- **内容**: シェルスクリプトベースの旧実装（参考用として保持）。開発時は `AISH_HOME` を `old_impl/_aish` に設定して利用可能。

## 開発方針

### テスト駆動開発（TDD）

**開発はテスト駆動開発（Test-Driven Development, TDD）で行ってください。**

#### テスト駆動開発の手順

1. **レッド（Red）**: 最初に失敗するテストを書く
   - 実装する機能や修正するバグに対応するテストを先に書く
   - テストは失敗することを確認する

2. **グリーン（Green）**: テストが通る最小限の実装を行う
   - テストが通るように、必要最小限のコードを実装する
   - テストが成功することを確認する

3. **リファクタリング（Refactor）**: コードを改善する
   - テストが通る状態を維持しながら、コードの品質を向上させる
   - 重複を削除し、可読性や保守性を向上させる

#### 重要な原則

- **省略しないこと**: テストを省略したり、手順を飛ばしたりしないこと
- **テストを先に書く**: 実装より先にテストを書くことを徹底する
- **気付きの記録**: 開発中に気付きや改善点があった場合は、このAGENTS.mdを更新すること

### Rust実装の方針

#### 設計の原則

**重要な原則**: 関心の分離、テスト容易性、依存関係の最小化を重視した設計を行う。

1. **関心の分離（Separation of Concerns）**
   - 各機能は独立したモジュールに分離する
   - `main.rs`はエントリーポイントとエラーハンドリングのみを担当
   - メインロジックは別モジュール（`app.rs`）に配置
   - 特定の機能（引数解析、設定読み込み、ファイル操作、ネットワーク通信など）は専用モジュールに分離
   - 例: 引数解析は`args.rs`、設定管理は`config.rs`、ファイル操作は`file.rs`など
   - 各モジュールは単一の責任を持つように設計する

2. **設定の構造化（Configuration Struct Pattern）**
   - 設定値や状態は専用の構造体（例: `Config`）に格納する
   - 構造体には`Default`トレイトを実装する
   - 設定値は構造体経由でメインロジックに渡す
   - これにより、設定の検証やデフォルト値の管理が容易になる
   - 例: 引数解析結果、設定ファイルの内容、実行時のオプション、処理対象のデータなど

3. **依存関係の最小化**
   - 外部ライブラリは必要最小限に保つ
   - 標準ライブラリ（`std`）で実装可能な機能は外部ライブラリを使わない
   - 既存ツール（`aish-capture`、`aish-script`）と同じパターンに従う
   - 例: 引数解析は`clap`を使わず`std::env::args()`を使用。JSON解析・HTTPは common で `serde_json`・`reqwest` を使用
   - 依存関係を追加する前に、標準ライブラリで実装できないか検討する

4. **エラーハンドリングの統一**
 - ドメイン・ユースケース層: `Result<T, common::error::Error>`（enum）。失敗を型で表現し、`match` でハンドリング可能
 - CLI 境界（main）: `Error::exit_code()` で終了コードに変換し、`Error::is_usage()` で用法表示の要否を判定
 - `Error` は thiserror で定義（InvalidArgument, Io, Env, Provider, Http, Json, TaskNotFound, System）。64: 用法/引数、74: I/O/HTTP、70: システム
 - コンストラクタ: `Error::invalid_argument(msg)`, `Error::io_msg(msg)`, `Error::http(msg)` 等。`From<std::io::Error>` で `?` も利用可

5. **テスト容易性（Testability）**
   - 各モジュールは独立してテスト可能な設計にする
   - 関数は純粋関数（副作用が少ない）にすることを心がける
   - テストは各モジュールに`#[cfg(test)]`ブロックを追加して記述
   - モックやスタブが不要な設計を目指す
   - 外部依存（ファイルシステム、環境変数、ネットワークなど）は可能な限り抽象化する

6. **推奨されるファイル構造**
   ```
   src/
   ├── main.rs      # エントリーポイント、エラーハンドリング
   ├── args.rs      # 引数解析（必要に応じて）
   ├── config.rs    # 設定管理（必要に応じて）
   ├── app.rs       # メインロジック
   └── task.rs      # タスク実行など（必要に応じて）
   ```


### Rustらしい設計パラダイム（Transaction Script からの脱却）

現状の実装は、`app.rs` に「引数解析 → パス解決 → I/O → プロセス起動 → シグナル → LLM → 永続化」が直列に積み上がる **トランザクションスクリプト** になりやすい。  
今後は以下の方針で、Rustの強み（型・列挙型・境界の明確化・テスト容易性）を活かした構造へ寄せる。

#### ルール（必須）

1. **型で境界を作る（Newtype / Domain Types）**
   - `String` / `PathBuf` を直接運ぶのを避け、意味のある型に包む。
   - 例: `HomeDir`, `SessionDir`, `ConsolePath`, `TaskDir`, `PartId`, `ProviderName`, `UserInput`。
   - コンストラクタで検証（存在確認/作成/正規化）を行い、以降の処理を安全にする。

2. **ユースケースを `struct` にし、依存は注入する（Ports & Adapters）**
   - `AiUseCase` / `AishUseCase` のように「やりたいこと」を表す `struct` を置く。
   - OS/外部要因（FS、環境変数、時刻、PTY、シグナル、HTTP/LLM）は trait（ポート）に閉じ込め、実装（アダプタ）を差し替え可能にする。
   - `main.rs`/`app.rs` は **配線（wiring）だけ**にし、ロジックは usecase 側へ寄せる。

3. **コマンドは `enum` 化して `match` でディスパッチする（Command Pattern）**
   - 引数解析の結果は `enum Command` に落とし、`match` で分岐する。
   - 「未実装」は `match` の分岐として明示し、曖昧な `if/else` の増殖を避ける。

4. **セッション/ログは状態機械としてモデル化する（Signals → Events）**
   - SIGUSR1/2, SIGWINCH などのシグナルは “イベント” に変換し、`Session::handle(event)` に集約する。
   - `console.txt` の flush / rollover / truncate の責務を1箇所へ集め、二重保存・順序バグを防ぐ。

5. **ストリーミングは「生成」と「消費（表示/保存）」を分離する**
   - `Chunk` 列（Iterator / Stream）として扱い、`StdoutSink` / `FileSink` のように複数 sink に fan-out できる設計にする。
   - “受信しながら表示し、最後に永続化する” でも、責務は別コンポーネントに分ける。

6. **エラーはドメイン `enum` を基本にし、終了コードは境界で付与する**
   - 原則: usecase 内は `Result<T, DomainError>`（enum）で表現し、**CLI境界で exit code に変換**する。
   - 現状は `type Error = (String, i32)` を使用しているが、今後の拡張（原因別ハンドリング/テスト）を考え、段階的に enum へ移行する。
   - 依存追加は最小化するが、`thiserror` 等は「可読性/保守性が大きく上がる」場合のみ採用してよい（採用時は理由をAGENTS.mdへ追記）。

7. **モジュールは “層” で分ける（`common` をゴミ箱にしない）**
   - 例:
     - `domain/` : 型と不変条件（Newtype、enum、ルール）
     - `usecase/`: アプリケーションの手続き（Ai/Aishの実行）
     - `adapter/`: OS/HTTP/FS/PTY/Signal/LLM の具体実装
     - `cli/` : 引数解析、表示、exit code
   - `common` は “本当に共有される安定要素” のみを置く。

   **common 肥大化防止（絶対ルール）**
   - **common に入れて良いもの**: 2つ以上の crate（ai / aish / tools）が確実に共有し、かつ安定しているもの。ドメイン型（Newtype）、汎用エラー、イベント（LlmEvent / AgentEvent）、Msg、PartId 生成、Port trait（FileSystem / Process / Clock / Pty / Signal / Tool 等）と標準実装（std_*）、LLM provider 実装（gemini / gpt / echo）。
   - **common に入れてはいけないもの**: ai だけ / aish だけにしか意味がないユースケース、CLI の都合、コマンド解釈。“近いから” という理由での util 追加（まずは core/ai または core/aish の adapter / usecase に置く）。
   - **迷ったら**: まず各バイナリ側（core/ai または core/aish）に置く。後から共有が確定したら common に昇格（逆は極力しない）。
   - **session**: 共通だが OS/env/fs に依存するため common に置くのは許可。ai/aish のユースケースロジックは common に入れない。



#### 実装例

**引数解析の例**:
```rust
// src/args.rs
use common::error::Error;

pub struct Config {
    pub profile: Option<String>,
    pub help: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            profile: None,
            help: false,
        }
    }
}

pub fn parse_args() -> Result<Config, Error> {
    let args: Vec<String> = std::env::args().collect();
    let mut config = Config::default();
    // 引数解析ロジック
    Ok(config)
}
```

**メインロジックの例**:
```rust
// src/main.rs
mod args;
mod app;
use common::error::Error;
use args::parse_args;
use app::run_app;

pub fn run() -> Result<i32, Error> {
    let config = parse_args()?;
    run_app(config)
}
```

```rust
// src/app.rs
use crate::args::Config;
use common::error::Error;

pub fn run_app(config: Config) -> Result<i32, Error> {
    // メインロジック
    Ok(0)
}
```

#### その他の方針

- **テストフレームワーク**: Rust標準の`#[cfg(test)]`モジュールを使用
- **コードスタイル**: 既存ツールのコードスタイルに合わせる

## テスト実行方法

### 作業前後の確認（必須）

**重要**: コードを変更する前後には、必ず結合テストを実行してください。

```bash
# 作業前: 現在の状態が正常であることを確認
./test_integration.sh

# 作業後: 変更によって既存の機能が壊れていないことを確認
./test_integration.sh
```

結合テストスクリプト（`test_integration.sh`）は以下を実行します：
- `core/ai` と `core/aish` のバイナリをビルド
- ai: 引数なし・不正オプションで終了コード 64 になることを確認
- aish: パイプ入力でシェルが動作すること、不正オプション・未実装コマンドで 64、デフォルトで毎回別セッション、`-s` で指定セッションになることを確認

### プロジェクト全体の単体テスト

```bash
# プロジェクトルートから実行
./test.sh
```

このスクリプトは以下のテストを実行します：
- `core/ai` の Rust テスト（`cargo test`）
- `core/aish` の Rust テスト（`cargo test`）

（`core/common` は単体では実行されず、ai / aish の依存としてそのテスト内で利用される）

### 結合テスト

```bash
# リリースビルドで結合テストを実行
./test_integration.sh

# デバッグビルドで結合テストを実行
BUILD_MODE=debug ./test_integration.sh
```

### 個別プロジェクトのテスト

```bash
# common ライブラリ
cd core/common
cargo test

# ai コマンド
cd core/ai
cargo test

# aish コマンド
cd core/aish
cargo test
```

## ビルド方法

### プロジェクト全体のビルド

```bash
# プロジェクトルートから実行（すべてのバイナリをビルド）
./build.sh

# デバッグビルドの場合
./build.sh --debug
```

このスクリプトは以下のバイナリをビルドし、`home/bin/` に配置します：
- `tools/aish-capture`
- `tools/aish-render`
- `tools/aish-script`
- `tools/leakscan`
- `core/ai`
- `core/aish`

### コア実装（Rust）の個別ビルド

```bash
# ai コマンド
cd core/ai
cargo build --release

# aish コマンド
cd core/aish
cargo build --release
```

## デバッグ方法

- **Rust の aish を開発環境で使う**: `-d` でホームディレクトリを指定する。
  ```bash
  ./home/bin/aish -d "$PROJECT_ROOT/home"
  ```
- **旧実装（シェル）でデバッグする場合**:
  ```bash
  export AISH_HOME="$PROJECT_ROOT/old_impl/_aish"
  ./old_impl/aish
  ```

## 既知の課題（バグ）

プロジェクトルートの `BUGS.md` に未解決のバグを記載している。主に `core/aish` のターミナルバッファ・ログ周り（長いテキストの端折り、カーソル移動の再現不備など）。

## 禁止事項

- テストを省略して実装を進めること
- 既存のコード構造を理解せずに変更すること
- このAGENTS.mdを読まずに作業を開始すること

## サブプロジェクトの詳細ドキュメント

各サブプロジェクトには専用のAGENTS.mdがあります。作業対象のサブプロジェクトがある場合は、必ず該当するAGENTS.mdも確認してください：

- [`tools/aish-capture/AGENTS.md`](tools/aish-capture/AGENTS.md)
- その他のサブプロジェクトのAGENTS.md（該当する場合）

## プロジェクト全体のドキュメント

- **プロジェクトREADME**: [`README.md`](README.md)
- **既知のバグ**: [`BUGS.md`](BUGS.md)
- **設計ドキュメント**: `plan/*.md`（該当する場合）

## 更新履歴

- **2026年1月**: AGENTS.md を現状に合わせて全面見直し
  - `core/common`: 提供機能を詳細化（error, session, llm, part_id）。LLM プロバイダ（Gemini / GPT / Echo）、factory、part_id の仕様を追記
  - `core/ai`: 状態を「実装済み」に更新。モジュール構成（args, app, task）、CLI・タスク・セッションの仕様を追記
  - `core/aish`: 状態を「一部実装済み」に更新。モジュール構成（logfmt, shell, terminal, platform）、実装済み/未実装コマンド、セッション解決を追記
  - エラーハンドリングを `common::error::Error` に統一する旨を明記
  - `tools/leakscan` をビルド一覧に追加
  - 結合テストの内容（ai / aish の検証項目）を具体化
  - デバッグ方法に Rust aish の `-d` を追加
  - BUGS.md への参照と「既知の課題」セクションを追加
  - 更新履歴に上記を記録
- **2025年1月**: プロジェクトルートにAGENTS.mdを作成
  - `core/ai`と`core/aish`のRustプロジェクト作成
  - `src/`ディレクトリを`core/`にリネーム
  - テストフレームワークの組み込み
  - プロジェクト全体のテストスクリプト（`test.sh`）作成
  - `build.sh`に`ai`と`aish`コマンドのビルド項目を追加
  - 結合テストスクリプト（`test_integration.sh`）作成
  - 作業前後の確認手順を明記
  - Rust実装の設計原則を一般化してルールとして追加（関心の分離、設定の構造化、依存関係の最小化、エラーハンドリングの統一、テスト容易性）
