#!/usr/bin/env bash
set -e

# プロジェクトルートを取得
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$SCRIPT_DIR"

# binフォルダを作成
BIN_DIR="$PROJECT_ROOT/bin"
mkdir -p "$BIN_DIR"

# aish-captureをビルド
echo "Building aish-capture..."
cd "$PROJECT_ROOT/tools/aish-capture"
cargo build --release

# aish-renderをビルド
echo "Building aish-render..."
cd "$PROJECT_ROOT/tools/aish-render"
cargo build --release

# ビルド成果物をbinフォルダにコピー
echo "Deploying binaries to bin/..."
cp "$PROJECT_ROOT/tools/aish-capture/target/release/aish-capture" "$BIN_DIR/"
cp "$PROJECT_ROOT/tools/aish-render/target/release/aish-render" "$BIN_DIR/"

echo "Build complete! Binaries are in $BIN_DIR/"
ls -lh "$BIN_DIR/"

