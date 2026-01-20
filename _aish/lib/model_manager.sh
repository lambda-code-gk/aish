#!/usr/bin/env bash

# モデル管理ライブラリ
# プロバイダごとの対応モデル一覧を表示する機能を提供します

# エラーハンドリングライブラリを読み込む
. "$AISH_HOME/lib/error_handler.sh"

# プロバイダファイルからSUPPORTED_MODELS配列を抽出
# 引数: provider - プロバイダ名（例: "gpt", "gemini"）
# 戻り値: SUPPORTED_MODELS配列の内容（改行区切り）
function _extract_supported_models
{
    local provider="$1"
    local provider_file="$AISH_HOME/ai.$provider"
    
    if [ ! -f "$provider_file" ]; then
        return 1
    fi
    
    # SUPPORTED_MODELS配列を抽出
    # awkを使って、SUPPORTED_MODELS=( から ) までの間を抽出（コメント行は除外）
    awk '
        /^SUPPORTED_MODELS=\(/ { in_array=1; next }
        in_array && /^\)/ { exit }
        in_array {
            # コメント行をスキップ（行頭の空白の後、#で始まる行）
            if (/^[[:space:]]*#/) next
            # 文字列リテラルを抽出
            if (match($0, /"[^"]*"/)) {
                model = substr($0, RSTART+1, RLENGTH-2);
                if (model != "") print model;
            }
        }
    ' "$provider_file"
}

# 指定されたプロバイダの対応モデル一覧を表示
# 引数: provider - プロバイダ名（例: "gpt", "gemini"）
function list_supported_models
{
    local provider="$1"
    
    if [ -z "$provider" ]; then
        error_error "Provider name is required"
        return 1
    fi
    
    local supported_models
    supported_models=$(_extract_supported_models "$provider")
    
    if [ $? -ne 0 ] || [ -z "$supported_models" ]; then
        error_error "Failed to extract supported models for provider: $provider"
        return 1
    fi
    
    if [ -z "$supported_models" ]; then
        echo "No supported models found for provider: $provider"
        return 0
    fi
    
    echo "Supported models for $provider:"
    echo "$supported_models" | while IFS= read -r model; do
        if [ ! -z "$model" ]; then
            echo "  - $model"
        fi
    done
}

# 全プロバイダの対応モデル一覧を表示
function list_all_supported_models
{
    local providers=("gpt" "gemini" "ollama")
    local found=false
    
    for provider in "${providers[@]}"; do
        if [ -f "$AISH_HOME/ai.$provider" ]; then
            found=true
            list_supported_models "$provider"
            echo ""
        fi
    done
    
    if [ "$found" = false ]; then
        error_error "No provider files found"
        return 1
    fi
}

# 指定されたプロバイダのAPIから利用可能なモデル一覧を取得して表示
# 引数: provider - プロバイダ名（例: "gpt", "gemini"）
function list_available_models
{
    local provider="$1"
    
    if [ -z "$provider" ]; then
        error_error "Provider name is required"
        return 1
    fi
    
    local provider_file="$AISH_HOME/ai.$provider"
    
    if [ ! -f "$provider_file" ]; then
        error_error "Provider file not found: $provider_file"
        return 1
    fi
    
    # AISH_SESSIONが必要な場合があるので、一時的なディレクトリを作成
    if [ -z "$AISH_SESSION" ]; then
        export AISH_SESSION=$(mktemp -d)
    fi
    
    # プロバイダファイルを読み込む（サブシェルで実行して環境を汚染しないようにする）
    # _provider_list_available_models関数を呼び出すため、最小限のライブラリのみ読み込む
    local available_models
    available_models=$(
        export AISH_HOME
        export AISH_SESSION
        # エラー出力関数が必要な場合があるので、agent_approve.shを読み込む
        . "$AISH_HOME/lib/agent_approve.sh" 2>/dev/null || true
        # プロバイダファイルを読み込む（_provider_list_available_models関数を取得）
        # tool_helper.shは読み込まない（_provider_list_available_modelsには不要）
        . "$provider_file" 2>/dev/null || true
        # _provider_list_available_models関数を呼び出す
        _provider_list_available_models 2>&1
    )
    
    if [ $? -ne 0 ] || [ -z "$available_models" ]; then
        error_error "Failed to fetch available models for provider: $provider"
        return 1
    fi
    
    echo "Available models for $provider (from API):"
    echo "$available_models" | while IFS= read -r model; do
        if [ ! -z "$model" ]; then
            echo "  - $model"
        fi
    done
}

# 指定されたプロバイダの未対応モデル一覧を表示
# 未対応モデル = APIから取得可能だが、SUPPORTED_MODELSに含まれていないモデル
# 引数: provider - プロバイダ名（例: "gpt", "gemini"）
function list_unsupported_models
{
    local provider="$1"
    
    if [ -z "$provider" ]; then
        error_error "Provider name is required"
        return 1
    fi
    
    # 対応モデル一覧を取得
    local supported_models
    supported_models=$(_extract_supported_models "$provider")
    
    if [ $? -ne 0 ]; then
        error_error "Failed to extract supported models for provider: $provider"
        return 1
    fi
    
    # 利用可能モデル一覧を取得
    local provider_file="$AISH_HOME/ai.$provider"
    
    if [ -z "$AISH_SESSION" ]; then
        export AISH_SESSION=$(mktemp -d)
    fi
    
    local available_models
    available_models=$(
        export AISH_HOME
        export AISH_SESSION
        # エラー出力関数が必要な場合があるので、agent_approve.shを読み込む
        . "$AISH_HOME/lib/agent_approve.sh" 2>/dev/null || true
        # プロバイダファイルを読み込む（_provider_list_available_models関数を取得）
        # tool_helper.shは読み込まない（_provider_list_available_modelsには不要）
        . "$provider_file" 2>/dev/null || true
        # _provider_list_available_models関数を呼び出す
        _provider_list_available_models 2>&1
    )
    
    if [ $? -ne 0 ] || [ -z "$available_models" ]; then
        error_error "Failed to fetch available models for provider: $provider"
        return 1
    fi
    
    # 対応モデルリストを作成
    local supported_list
    supported_list=$(echo "$supported_models" | sort)
    
    # 未対応モデルを抽出（利用可能だが対応リストに含まれていない）
    echo "Unsupported models for $provider (available from API but not in SUPPORTED_MODELS):"
    local found=false
    echo "$available_models" | while IFS= read -r model; do
        if [ ! -z "$model" ] && ! echo "$supported_list" | grep -Fxq "$model" 2>/dev/null; then
            echo "  - $model"
            found=true
        fi
    done
    
    if [ "$found" = false ]; then
        echo "  (none)"
    fi
}

