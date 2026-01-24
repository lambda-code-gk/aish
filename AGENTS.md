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
- **現在の状態**: シェルスクリプトからRustへの刷新作業中

## プロジェクト構造

### コア実装（Rust刷新中）

#### `core/common/`
- **目的**: `ai`と`aish`コマンドの共通処理を提供するライブラリ
- **ディレクトリ**: `core/common/`
- **ビルド方法**: `cd core/common && cargo build --release`
- **テスト実行**: `cd core/common && cargo test`
- **状態**: 初期プロジェクト作成済み
- **提供機能**:
  - エラーハンドリング（`error`モジュール）
  - 共通ユーティリティ（`util`モジュール）

#### `core/ai/`
- **目的**: `ai`コマンドのRust実装
- **ディレクトリ**: `core/ai/`
- **ビルド方法**: `cd core/ai && cargo build --release`
- **テスト実行**: `cd core/ai && cargo test`
- **依存関係**: `core/common`
- **状態**: 初期プロジェクト作成済み、実装待ち

#### `core/aish/`
- **目的**: `aish`コマンドのRust実装
- **ディレクトリ**: `core/aish/`
- **ビルド方法**: `cd core/aish && cargo build --release`
- **テスト実行**: `cd core/aish && cargo test`
- **依存関係**: `core/common`
- **状態**: 初期プロジェクト作成済み、実装待ち

### 既存ツール（Rust実装済み）

#### `tools/aish-capture/`
- **目的**: `script`コマンドの代替となる軽量なPTYキャプチャツール（Rust実装）
- **詳細ドキュメント**: [`tools/aish-capture/AGENTS.md`](tools/aish-capture/AGENTS.md)

#### `tools/aish-script/`
- **目的**: expect機能を提供するスクリプト実行ツール（Rust実装）
- **機能**: JSONLログファイルを監視し、パターンマッチングに基づいてFIFO経由で入力を送信

#### `tools/aish-render/`
- **目的**: ターミナル出力のレンダリングツール（Rust実装）

### 旧実装（参考用）

- **ディレクトリ**: `old_impl/`
- **内容**: シェルスクリプトベースの旧実装（参考用として保持）

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
   - 例: 引数解析は`clap`を使わず`std::env::args()`を使用、JSON解析が必要な場合のみ`serde_json`を使用
   - 依存関係を追加する前に、標準ライブラリで実装できないか検討する

4. **エラーハンドリングの統一**
   - エラーは`Result<T, (String, i32)>`形式で統一する
     - `T`: 成功時の戻り値（関数によって異なる）
     - `String`: エラーメッセージ（ユーザーフレンドリーな内容）
     - `i32`: 終了コード（64: 引数不正、74: I/Oエラー、70: システムエラーなど）
   - エラーメッセージは明確で、問題の原因と解決方法を示す
   - 終了コードは標準的な値を使用する（BSD exit codesを参考）

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
   └── app.rs        # メインロジック
   ```

#### 実装例

**引数解析の例**:
```rust
// src/args.rs
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

pub fn parse_args() -> Result<Config, (String, i32)> {
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
use args::parse_args;
use app::run_app;

pub fn run() -> Result<i32, (String, i32)> {
    let config = parse_args()?;
    run_app(config)
}
```

```rust
// src/app.rs
use crate::args::Config;

pub fn run_app(config: Config) -> Result<i32, (String, i32)> {
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
- `core/ai`と`core/aish`のバイナリをビルド
- ビルドしたバイナリが正常に実行できることを確認
- 基本的な動作確認

### プロジェクト全体の単体テスト

```bash
# プロジェクトルートから実行
./test.sh
```

このスクリプトは以下のテストを実行します：
- `core/ai`のRustテスト（`cargo test`）
- `core/aish`のRustテスト（`cargo test`）

### 結合テスト

```bash
# リリースビルドで結合テストを実行
./test_integration.sh

# デバッグビルドで結合テストを実行
BUILD_MODE=debug ./test_integration.sh
```

結合テストは以下を確認します：
- バイナリが正常にビルドできること
- ビルドしたバイナリが実行できること
- 基本的な動作が正常であること

### 個別プロジェクトのテスト

```bash
# aiプロジェクト
cd core/ai
cargo test

# aishプロジェクト
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

このスクリプトは以下のバイナリをビルドし、`_aish/bin/`に配置します：
- `tools/aish-capture`
- `tools/aish-render`
- `tools/aish-script`
- `tools/leakscan`
- `core/ai`
- `core/aish`

### コア実装（Rust）の個別ビルド

```bash
# aiコマンド
cd core/ai
cargo build --release

# aishコマンド
cd core/aish
cargo build --release
```

## デバッグ方法

開発用のディレクトリで作業している際は以下の手順で実行すると開発環境のファイルを参照するようになります。

```bash
export AISH_HOME="$PROJECT_ROOT/old_impl/_aish"
./old_impl/aish
```

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
- **設計ドキュメント**: `plan/*.md`（該当する場合）

## 更新履歴

- **2025年1月**: プロジェクトルートにAGENTS.mdを作成
  - `core/ai`と`core/aish`のRustプロジェクト作成
  - `src/`ディレクトリを`core/`にリネーム
  - テストフレームワークの組み込み
  - プロジェクト全体のテストスクリプト（`test.sh`）作成
  - `build.sh`に`ai`と`aish`コマンドのビルド項目を追加
  - 結合テストスクリプト（`test_integration.sh`）作成
  - 作業前後の確認手順を明記
  - Rust実装の設計原則を一般化してルールとして追加（関心の分離、設定の構造化、依存関係の最小化、エラーハンドリングの統一、テスト容易性）

