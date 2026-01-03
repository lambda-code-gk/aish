# AGENTS.md - AI開発エージェント向けプロジェクト知識ベース

このドキュメントは、このプロジェクトにおけるAI開発エージェント（Cursor AI等）が参照すべき約束事、実行方法、実装方針を記録しています。

## プロジェクト概要

- **メインプロジェクト**: AISH（AISH is a CUI automation framework powered by LLMs）
- **プロジェクトルート**: `/home/honda/labo/aish_make_rust_dev`

## デバッグ方法
開発用のディレクトリで作業している際は以下の手順で実行すると開発環境のファイルを参照するようになります。

```bash
export AISH_HOME="$PROJECT_ROOT/_aish"
./aish
```

## 開発方針

### テスト駆動開発（TDD）

開発はテスト駆動開発（Test-Driven Development, TDD）で行ってください。

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

## サブプロジェクト

### aish-capture

`script`コマンドの代替となる軽量なPTYキャプチャツール（Rust実装）

- **ディレクトリ**: `tools/aish-capture/`
- **詳細ドキュメント**: [`tools/aish-capture/AGENTS.md`](tools/aish-capture/AGENTS.md)

詳細な実装方針、ビルド方法、テスト実行方法などは上記のドキュメントを参照してください。

### aish-script

expect機能を提供するスクリプト実行ツール（Rust実装）

- **ディレクトリ**: `tools/aish-script/`
- **目的**: JSONLログファイルを監視し、パターンマッチングに基づいてFIFO経由で入力を送信する
- **機能**:
  - DSLパーサー（文字列マッチング、複数ルール対応）
  - JSONLログファイル読み取り（ANSIエスケープシーケンス対応）
  - FIFOへの入力送信
  - スクリプトファイル読み込み
- **ビルド方法**: `cd tools/aish-script && cargo build --release`
- **テスト実行**: `cd tools/aish-script && ./test.sh`

## プロジェクト全体のドキュメント

- **プロジェクトREADME**: [`README.md`](README.md)
- **設計ドキュメント**: [`plan/01_improve_terminal_capture.md`](plan/01_improve_terminal_capture.md)

## 更新履歴

- **2025年1月1日**: AGENTS.mdを多段構造に再編成
  - ルートAGENTS.mdは概要と参照のみに変更
  - aish-captureの詳細は`tools/aish-capture/AGENTS.md`に移動
- **2025年1月3日**: aish-scriptサブプロジェクトの情報を追加
  - Phase 2の基本実装完了を反映