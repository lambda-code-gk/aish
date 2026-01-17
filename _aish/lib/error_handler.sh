#!/usr/bin/env bash

# 統一されたエラーハンドリングライブラリ
# エラーレベル、スタックトレース、構造化エラー出力を提供

# エラーレベルの定義（既に定義されている場合はスキップ）
if [ -z "${ERROR_LEVEL_FATAL+x}" ]; then
    readonly ERROR_LEVEL_FATAL=0    # 致命的エラー（即座に終了）
    readonly ERROR_LEVEL_ERROR=1    # エラー（処理を中断）
    readonly ERROR_LEVEL_WARN=2     # 警告（処理を継続）
    readonly ERROR_LEVEL_INFO=3     # 情報
    readonly ERROR_LEVEL_DEBUG=4    # デバッグ
fi

# エラーレベルの文字列表現
_error_level_to_string() {
    case "$1" in
        $ERROR_LEVEL_FATAL) echo "FATAL" ;;
        $ERROR_LEVEL_ERROR) echo "ERROR" ;;
        $ERROR_LEVEL_WARN)  echo "WARN"  ;;
        $ERROR_LEVEL_INFO)  echo "INFO"  ;;
        $ERROR_LEVEL_DEBUG) echo "DEBUG" ;;
        *)                  echo "UNKNOWN" ;;
    esac
}

# エラーレベルの色付け
_error_level_to_color() {
    case "$1" in
        $ERROR_LEVEL_FATAL) echo "\033[1;31m" ;;  # 赤（太字）
        $ERROR_LEVEL_ERROR) echo "\033[0;31m" ;;  # 赤
        $ERROR_LEVEL_WARN)  echo "\033[1;33m" ;;  # 黄（太字）
        $ERROR_LEVEL_INFO)  echo "\033[0;36m" ;;  # シアン
        $ERROR_LEVEL_DEBUG) echo "\033[0;90m" ;;  # グレー
        *)                  echo "\033[0m" ;;      # リセット
    esac
}

# スタックトレースの取得（呼び出し元の情報）
_get_stack_trace() {
    local depth=${1:-1}
    local frame=0
    local result=""
    
    # 呼び出し元の情報を取得（最大10フレーム）
    while [ $frame -lt 10 ] && caller $((frame + depth)) >/dev/null 2>&1; do
        local line_info=$(caller $((frame + depth)) 2>/dev/null)
        if [ -n "$line_info" ]; then
            local line_num=$(echo "$line_info" | awk '{print $1}')
            local func_name=$(echo "$line_info" | awk '{print $2}')
            local file_path=$(echo "$line_info" | awk '{print $3}')
            local file_name=$(basename "$file_path" 2>/dev/null || echo "$file_path")
            
            if [ -z "$result" ]; then
                result="  at $func_name ($file_name:$line_num)"
            else
                result="$result\n  at $func_name ($file_name:$line_num)"
            fi
        fi
        frame=$((frame + 1))
    done
    
    echo -e "$result"
}

# 構造化エラーログの出力
_log_error_structured() {
    local level="$1"
    local message="$2"
    local context="${3:-}"
    local exit_code="${4:-}"
    
    local timestamp=$(date -u +%Y-%m-%dT%H:%M:%S.%NZ 2>/dev/null || date +%Y-%m-%dT%H:%M:%S)
    local level_str=$(_error_level_to_string "$level")
    local stack_trace=$(_get_stack_trace 2)
    
    # 構造化JSONログの作成
    local log_entry
    log_entry=$(jq -n \
        --arg timestamp "$timestamp" \
        --arg level "$level_str" \
        --arg message "$message" \
        --arg context "$context" \
        --arg stack_trace "$stack_trace" \
        --arg exit_code "${exit_code:-null}" \
        --arg session "${AISH_SESSION:-unknown}" \
        '{
            timestamp: $timestamp,
            level: $level,
            message: $message,
            context: (if $context != "" then $context else null end),
            stack_trace: (if $stack_trace != "" then $stack_trace else null end),
            exit_code: (if $exit_code != "null" then ($exit_code | tonumber) else null end),
            session: $session
        }' 2>/dev/null) || log_entry=""
    
    # セッションログファイルに記録（存在する場合、かつlog_entryが作成できた場合）
    if [ -n "$AISH_SESSION" ] && [ -d "$AISH_SESSION" ] && [ -n "$log_entry" ]; then
        local error_log="${AISH_SESSION}/error.log"
        echo "$log_entry" >> "$error_log" 2>/dev/null || true
    fi
    
    # デバッグモードの場合は詳細情報も出力
    if [ "${AISH_DEBUG:-false}" = "true" ]; then
        echo "$log_entry" | jq '.' >&2
    fi
}

# エラーメッセージの出力（ユーザー向け）
_log_error_user() {
    local level="$1"
    local message="$2"
    local color=$(_error_level_to_color "$level")
    local reset="\033[0m"
    local level_str=$(_error_level_to_string "$level")
    
    # エラーレベルに応じたプレフィックス
    case "$level" in
        $ERROR_LEVEL_FATAL)
            echo -e "${color}❌ FATAL: $message${reset}" >&2
            ;;
        $ERROR_LEVEL_ERROR)
            echo -e "${color}✗ ERROR: $message${reset}" >&2
            ;;
        $ERROR_LEVEL_WARN)
            echo -e "${color}⚠ WARN: $message${reset}" >&2
            ;;
        $ERROR_LEVEL_INFO)
            echo -e "${color}ℹ INFO: $message${reset}" >&2
            ;;
        $ERROR_LEVEL_DEBUG)
            if [ "${AISH_DEBUG:-false}" = "true" ]; then
                echo -e "${color}🐛 DEBUG: $message${reset}" >&2
            fi
            ;;
    esac
}

# 統一されたエラー出力関数
# 引数:
#   $1: エラーレベル (ERROR_LEVEL_*)
#   $2: エラーメッセージ
#   $3: コンテキスト情報（オプション、JSON形式推奨）
#   $4: 終了コード（オプション、FATAL/ERRORの場合のみ使用）
#   $5: 追加情報（オプション）
error_log() {
    local level="$1"
    local message="$2"
    local context="${3:-}"
    local exit_code="${4:-}"
    local extra="${5:-}"
    
    # 構造化ログの出力
    _log_error_structured "$level" "$message" "$context" "$exit_code"
    
    # ユーザー向けメッセージの出力
    _log_error_user "$level" "$message"
    
    # 追加情報がある場合は出力
    if [ -n "$extra" ]; then
        echo "$extra" >&2
    fi
    
    # スタックトレースの表示（デバッグモードまたはFATAL/ERRORの場合）
    if [ "${AISH_DEBUG:-false}" = "true" ] || [ "$level" -le $ERROR_LEVEL_ERROR ]; then
        local stack_trace=$(_get_stack_trace 2)
        if [ -n "$stack_trace" ]; then
            echo -e "\033[0;90mStack trace:$stack_trace\033[0m" >&2
        fi
    fi
}

# 便利関数: 致命的エラー
error_fatal() {
    local message="$1"
    local context="${2:-}"
    local exit_code="${3:-1}"
    
    error_log $ERROR_LEVEL_FATAL "$message" "$context" "$exit_code"
    exit "$exit_code"
}

# 便利関数: エラー
error_error() {
    local message="$1"
    local context="${2:-}"
    local exit_code="${3:-}"
    
    error_log $ERROR_LEVEL_ERROR "$message" "$context" "$exit_code"
    return 1
}

# 便利関数: 警告
error_warn() {
    local message="$1"
    local context="${2:-}"
    
    error_log $ERROR_LEVEL_WARN "$message" "$context"
    return 0
}

# 便利関数: 情報
error_info() {
    local message="$1"
    local context="${2:-}"
    
    error_log $ERROR_LEVEL_INFO "$message" "$context"
    return 0
}

# 便利関数: デバッグ
error_debug() {
    local message="$1"
    local context="${2:-}"
    
    error_log $ERROR_LEVEL_DEBUG "$message" "$context"
    return 0
}


# エラーハンドリングの初期化
error_handler_init() {
    # エラーログファイルの初期化（セッションが存在する場合）
    if [ -n "$AISH_SESSION" ] && [ -d "$AISH_SESSION" ]; then
        local error_log="${AISH_SESSION}/error.log"
        # ファイルが存在しない場合は作成（ヘッダー付き）
        if [ ! -f "$error_log" ]; then
            echo "[]" > "$error_log"
        fi
    fi
}

