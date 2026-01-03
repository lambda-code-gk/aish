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

## サブプロジェクト

### aish-capture

`script`コマンドの代替となる軽量なPTYキャプチャツール（Rust実装）

- **ディレクトリ**: `tools/aish-capture/`
- **詳細ドキュメント**: [`tools/aish-capture/AGENTS.md`](tools/aish-capture/AGENTS.md)

詳細な実装方針、ビルド方法、テスト実行方法などは上記のドキュメントを参照してください。

## プロジェクト全体のドキュメント

- **プロジェクトREADME**: [`README.md`](README.md)
- **設計ドキュメント**: [`plan/01_improve_terminal_capture.md`](plan/01_improve_terminal_capture.md)

## 更新履歴

- **2025年1月1日**: AGENTS.mdを多段構造に再編成
  - ルートAGENTS.mdは概要と参照のみに変更
  - aish-captureの詳細は`tools/aish-capture/AGENTS.md`に移動