# aish-capture - AI開発エージェント向け知識ベース

このドキュメントは、`aish-capture`サブプロジェクトにおけるAI開発エージェントが参照すべき約束事、実行方法、実装方針を記録しています。

## プロジェクト概要

- **名前**: `aish-capture`
- **目的**: `script`コマンドの代替となる軽量なPTYキャプチャツール（Rust実装）
- **ディレクトリ**: `tools/aish-capture/`

## 環境設定

### 開発環境・前提条件

* Rust

### ビルド方法

```bash
cd tools/aish-capture
cargo build --release
```

ビルド成果物は `tools/aish-capture/target/release/aish-capture` に生成されます。

## テスト実行方法

### 基本的なテスト実行

```bash
cd tools/aish-capture
./test.sh
```

### PTYが必要なテストの実行

`aish-capture`はPTY（疑似ターミナル）を使用するため、対話的なターミナルが必要です。
非対話的な環境（CI環境など）で実行する場合は、`script`コマンドを使用してPTYをシミュレートします：

```bash
cd tools/aish-capture
script -c './test.sh' -q /dev/null
```

この方法により、すべてのテスト（PTY依存のテストを含む）を実行できます。

### テストの詳細

- テストスクリプト: `tools/aish-capture/test.sh`
- テストドキュメント: `tools/aish-capture/README_TEST.md`
- テスト用ディレクトリ: 実行時に自動生成される一時ディレクトリ（終了時に自動削除）

## 実装方針と約束事

### 依存関係の最小化

**重要な原則**: `aish-capture`は依存を最小化することが要件です。

- 原則として`libc`のみを使用
- `clap`、`serde`、`base64`などのcrateは使用しない
- base64エンコードは最小実装を同梱（`tools/aish-capture/src/util/base64.rs`）
- OS依存箇所は`src/platform/`に分離

### JSONL出力フォーマット

#### エンコード方針（2025年1月1日改訂）

`stdin`/`stdout`イベントの`data`フィールドは、以下の方針でエンコードされます：

1. **JSON-safeなテキストの場合**:
   - `enc`フィールドを**省略**（オプショナル）
   - `data`フィールドに直接テキストを保存
   - JSON-safeとは、UTF-8として有効で、`\n`、`\r`、`\t`以外の制御文字を含まないこと

2. **バイナリデータや制御文字を含む場合**:
   - `enc: "b64"`フィールドを追加
   - `data`フィールドにbase64エンコードされた文字列を保存

#### バッファリング方針（2025年1月1日追加）

UTF-8でエンコードできるテキストデータについては、入力・出力共に改行までを1行にまとめます：

1. **UTF-8テキストの場合**:
   - `TextBuffer`構造体で改行文字（`\n`または`\r\n`）が来るまでバッファリング
   - 改行が来たら、それまでのデータを1つのイベントとして書き出す
   - 複数の行が含まれる場合は、各行が独立したイベントになる

2. **バイナリデータの場合**:
   - バッファリングせず、即座に書き出す
   - base64エンコードして`enc: "b64"`フィールド付きで保存

#### 実装詳細

- 判定関数: `check_json_safe_text()` (`src/logfmt.rs`)
- バッファリング: `TextBuffer`構造体 (`src/logfmt.rs`)
  - `append()`: データを追加し、改行が揃った行を返す
  - `flush()`: EOF時に残っているデータを書き出す
- テキストデータの場合、可読性とファイルサイズの削減がメリット
- バイナリデータ（ANSIエスケープシーケンスなど）はbase64エンコードが必須

#### 例

**テキスト出力の場合**:
```json
{"v":1,"t_ms":...,"type":"stdout","n":13,"data":"hello world\r\n"}
```

**ANSIエスケープシーケンスを含む場合**:
```json
{"v":1,"t_ms":...,"type":"stdout","enc":"b64","n":14,"data":"G1szMW1SRUQbWzBtDQo="}
```

### コード構造

```
tools/aish-capture/
├── src/
│   ├── main.rs          # メインエントリーポイント、設定と実行ループ
│   ├── logfmt.rs        # JSONL形式でのログ出力
│   │                    # - write_stdin, write_stdout等
│   │                    # - TextBuffer構造体（テキストバッファリング）
│   ├── platform/        # OS依存コード
│   │   ├── mod.rs
│   │   └── unix.rs      # Unix系OS（Linux/macOS）の実装
│   └── util/
│       ├── mod.rs
│       └── base64.rs    # RFC4648 base64エンコード（最小実装）
├── test.sh              # 自動テストスクリプト
└── Cargo.toml           # Rustプロジェクト設定
```

## 開発時の注意事項

### テストの修正

テストを修正する場合、以下の点に注意：

- `enc`フィールドはオプショナルになったため、テストで必須チェックを削除
- `data`フィールドの存在確認は継続
- テキスト出力とバイナリ出力の両方のケースを考慮

### コード品質

- `unsafe`ブロックは必要最小限に
- エラーハンドリングは適切に行う
- 終了コードは仕様に従う（64: 引数不正、74: I/Oエラー、70: fork/pty失敗、1: その他）

### パフォーマンス

- base64エンコードは必要な場合のみ実行（テキストデータの場合はスキップ）
- UTF-8検証は一度だけ実行（`check_json_safe_text`で判定と変換を同時に行う）
- テキストデータは改行までバッファリングすることで、1行ごとにイベント化し可読性を向上

### バッファリング動作

- stdin/stdout用にそれぞれ`TextBuffer`インスタンスを保持
- UTF-8テキストの場合、改行文字が来るまでデータをバッファに蓄積
- 改行が来たら、バッファの内容を1つのイベントとして書き出す
- EOF時には`flush()`で残っているデータを書き出す
- バイナリデータの場合はバッファリングせず、即座に書き出す

## トラブルシューティング

### cargoが見つからない場合

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

この設定を追加してください。

### PTYテストが失敗する場合

非対話的な環境では、`script`コマンドを使用：

```bash
script -c './test.sh' -q /dev/null
```

### ビルドエラー

依存関係の問題がある場合、`Cargo.toml`を確認してください。最小依存の方針に従っているか確認します。

## 関連ドキュメント

- **設計ドキュメント**: `plan/01_improve_terminal_capture.md`（プロジェクトルート）
- **テストドキュメント**: `tools/aish-capture/README_TEST.md`
- **プロジェクトREADME**: `README.md`（プロジェクトルート）

## 更新履歴

- **2025年1月1日（後半）**: UTF-8テキストの改行までバッファリング機能を追加
  - `logfmt.rs`に`TextBuffer`構造体を追加
  - stdin/stdoutで改行までバッファリングしてから1つのイベントとして書き出すように変更
  - バイナリデータは即座に書き出す（バッファリングしない）
  - テスト9（テキストバッファリング）とテスト10（バイナリデータ即座書き出し）を追加

- **2025年1月1日（前半）**: base64エンコードを条件付きに変更（JSON-safeなテキストは直接保存）
  - `logfmt.rs`に`check_json_safe_text`関数を追加
  - `write_stdin`/`write_stdout`を修正
  - テストを`enc`フィールドオプショナルに対応
