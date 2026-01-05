#!/usr/bin/env bash
set -e

# デバッグビルドかどうかを判定
BUILD_MODE="release"
TARGET_DIR="release"

if [[ "$1" == "--debug" ]] || [[ "$1" == "-d" ]]; then
    BUILD_MODE="debug"
    TARGET_DIR="debug"
    echo "Building in DEBUG mode..."
else
    echo "Building in RELEASE mode..."
fi

# プロジェクトルートを取得
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$SCRIPT_DIR"

# binフォルダを作成
BIN_DIR="$PROJECT_ROOT/_aish/bin"
mkdir -p "$BIN_DIR"

# ビルドコマンドを決定
if [[ "$BUILD_MODE" == "debug" ]]; then
    BUILD_CMD="cargo build"
else
    BUILD_CMD="cargo build --release"
fi

# aish-captureをビルド
echo "Building aish-capture..."
cd "$PROJECT_ROOT/tools/aish-capture"
$BUILD_CMD

# aish-renderをビルド
echo "Building aish-render..."
cd "$PROJECT_ROOT/tools/aish-render"
$BUILD_CMD

# aish-scriptをビルド
echo "Building aish-script..."
cd "$PROJECT_ROOT/tools/aish-script"
$BUILD_CMD

# ビルド成果物をbinフォルダにコピー
echo "Deploying binaries to _aish/bin/..."
cp "$PROJECT_ROOT/tools/aish-capture/target/$TARGET_DIR/aish-capture" "$BIN_DIR/"
cp "$PROJECT_ROOT/tools/aish-render/target/$TARGET_DIR/aish-render" "$BIN_DIR/"
cp "$PROJECT_ROOT/tools/aish-script/target/$TARGET_DIR/aish-script" "$BIN_DIR/"

echo "Build complete! Binaries are in $BIN_DIR/"
ls -lh "$BIN_DIR/"

