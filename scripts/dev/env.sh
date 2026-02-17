#!/usr/bin/env bash
# 開発時用: 実行時データを .sandbox/xdg/* に隔離し、PATH に dist/bin を追加する。
# 使い方: source scripts/dev/env.sh （プロジェクトルートで）
# または: . scripts/dev/env.sh

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SANDBOX_XDG="$PROJECT_ROOT/.sandbox/xdg"

mkdir -p "$SANDBOX_XDG/config" "$SANDBOX_XDG/data" "$SANDBOX_XDG/state" "$SANDBOX_XDG/cache"

export XDG_CONFIG_HOME="$SANDBOX_XDG/config"
export XDG_DATA_HOME="$SANDBOX_XDG/data"
export XDG_STATE_HOME="$SANDBOX_XDG/state"
export XDG_CACHE_HOME="$SANDBOX_XDG/cache"
export PATH="$PROJECT_ROOT/dist/bin:$PATH"

# 開発用サンドボックスではデフォルトで「root モード」（AISH_HOME 配下）を使わない。
# 既に AISH_HOME が設定されている環境であっても、ここで明示的に解除する。
unset AISH_HOME

# テンプレ展開時に repo 内の assets/defaults を参照する
export AISH_DEFAULTS_DIR="$PROJECT_ROOT/assets/defaults"

echo "[dev] XDG_* and PATH set for sandbox: $SANDBOX_XDG"
echo "[dev] AISH_DEFAULTS_DIR=$AISH_DEFAULTS_DIR"
