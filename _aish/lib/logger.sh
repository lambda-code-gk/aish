#!/usr/bin/env bash

# 構造化ログライブラリ
# JSON形式の構造化ログを提供し、ログレベルの管理とフィルタリングをサポート

# ログレベルの定義（既に定義されている場合はスキップ）
if [ -z "${LOG_LEVEL_TRACE+x}" ]; then
    readonly LOG_LEVEL_TRACE=0
    readonly LOG_LEVEL_DEBUG=1
    readonly LOG_LEVEL_INFO=2
    readonly LOG_LEVEL_WARN=3
    readonly LOG_LEVEL_ERROR=4
    readonly LOG_LEVEL_FATAL=5

    # デフォルトのログレベル（環境変数で上書き可能）
    readonly DEFAULT_LOG_LEVEL=$LOG_LEVEL_INFO
fi

# ログレベルの文字列表現
_log_level_to_string() {
    case "$1" in
        $LOG_LEVEL_TRACE) echo "TRACE" ;;
        $LOG_LEVEL_DEBUG) echo "DEBUG" ;;
        $LOG_LEVEL_INFO)  echo "INFO"  ;;
        $LOG_LEVEL_WARN)  echo "WARN"  ;;
        $LOG_LEVEL_ERROR) echo "ERROR" ;;
        $LOG_LEVEL_FATAL) echo "FATAL" ;;
        *)                echo "UNKNOWN" ;;
    esac
}

# 文字列からログレベルを取得
_log_level_from_string() {
    case "$1" in
        TRACE|trace) echo $LOG_LEVEL_TRACE ;;
        DEBUG|debug) echo $LOG_LEVEL_DEBUG ;;
        INFO|info)   echo $LOG_LEVEL_INFO  ;;
        WARN|warn)   echo $LOG_LEVEL_WARN  ;;
        ERROR|error) echo $LOG_LEVEL_ERROR ;;
        FATAL|fatal) echo $LOG_LEVEL_FATAL ;;
        *)           echo $DEFAULT_LOG_LEVEL ;;
    esac
}

# 現在のログレベルを取得
_get_log_level() {
    local level_str="${AISH_LOG_LEVEL:-INFO}"
    _log_level_from_string "$level_str"
}

# ログレベルチェック（指定されたレベルが現在のログレベル以上か）
_should_log() {
    local level="$1"
    local current_level=$(_get_log_level)
    [ "$level" -ge "$current_level" ]
}

# 構造化ログエントリの作成
_create_log_entry() {
    local level="$1"
    local message="$2"
    local component="${3:-}"
    local context="${4:-}"
    local metadata="${5:-}"
    
    local timestamp=$(date -u +%Y-%m-%dT%H:%M:%S.%NZ 2>/dev/null || date +%Y-%m-%dT%H:%M:%S)
    local level_str=$(_log_level_to_string "$level")
    
    # 呼び出し元の情報を取得
    local caller_info=""
    if caller 1 >/dev/null 2>&1; then
        local line_info=$(caller 1)
        local line_num=$(echo "$line_info" | awk '{print $1}')
        local func_name=$(echo "$line_info" | awk '{print $2}')
        local file_path=$(echo "$line_info" | awk '{print $3}')
        local file_name=$(basename "$file_path" 2>/dev/null || echo "$file_path")
        caller_info="$file_name:$line_num:$func_name"
    fi
    
    # JSON形式のログエントリを作成
    local log_entry
    log_entry=$(jq -n \
        --arg timestamp "$timestamp" \
        --arg level "$level_str" \
        --arg message "$message" \
        --arg component "${component:-unknown}" \
        --arg context "$context" \
        --arg caller "$caller_info" \
        --arg session "${AISH_SESSION:-unknown}" \
        --argjson metadata "${metadata:-null}" \
        '{
            timestamp: $timestamp,
            level: $level,
            message: $message,
            component: $component,
            context: (if $context != "" then $context else null end),
            caller: $caller,
            session: $session,
            metadata: $metadata
        }' 2>/dev/null) || log_entry=""
    
    echo "$log_entry"
}

# ログの出力先を決定
_get_log_output() {
    # セッションログファイルが存在する場合は使用
    if [ -n "$AISH_SESSION" ] && [ -d "$AISH_SESSION" ]; then
        echo "${AISH_SESSION}/app.log"
    else
        # セッションがない場合は標準エラー出力
        echo "/dev/stderr"
    fi
}

# ログの書き込み
_write_log() {
    local log_entry="$1"
    local output=$(_get_log_output)
    
    # log_entryが空の場合はスキップ
    if [ -z "$log_entry" ]; then
        return 0
    fi
    
    # JSON配列として管理（app.logが存在する場合）
    if [ -f "$output" ] && [ "$output" != "/dev/stderr" ]; then
        # 既存のJSON配列に追加
        local temp_file=$(mktemp)
        jq --argjson entry "$log_entry" '. + [$entry]' "$output" > "$temp_file" 2>/dev/null
        if [ $? -eq 0 ]; then
            mv "$temp_file" "$output"
        else
            # JSON配列として初期化
            echo "[$log_entry]" > "$output"
        fi
    elif [ "$output" != "/dev/stderr" ]; then
        # 新規作成
        echo "[$log_entry]" > "$output"
    else
        # 標準エラー出力（人間が読める形式）
        local level_str=$(echo "$log_entry" | jq -r '.level' 2>/dev/null || echo "UNKNOWN")
        local message=$(echo "$log_entry" | jq -r '.message' 2>/dev/null || echo "$log_entry")
        local component=$(echo "$log_entry" | jq -r '.component' 2>/dev/null || echo "unknown")
        
        # 色付け
        local color="\033[0m"
        case "$level_str" in
            FATAL) color="\033[1;31m" ;;
            ERROR) color="\033[0;31m" ;;
            WARN)  color="\033[1;33m" ;;
            INFO)  color="\033[0;36m" ;;
            DEBUG) color="\033[0;90m" ;;
            TRACE) color="\033[0;90m" ;;
        esac
        local reset="\033[0m"
        
        echo -e "${color}[$level_str]${reset} [$component] $message" >&2
    fi
}

# 統一されたログ出力関数
# 引数:
#   $1: ログレベル (LOG_LEVEL_*)
#   $2: メッセージ
#   $3: コンポーネント名（オプション）
#   $4: コンテキスト情報（オプション、JSON形式推奨）
#   $5: メタデータ（オプション、JSON形式）
log_write() {
    local level="$1"
    local message="$2"
    local component="${3:-aish}"
    local context="${4:-}"
    local metadata="${5:-}"
    
    # ログレベルチェック
    if ! _should_log "$level"; then
        return 0
    fi
    
    # コンテキストをJSON形式に変換（文字列の場合はそのまま、JSONの場合は検証）
    local context_json=""
    if [ -n "$context" ]; then
        # JSONとして有効かチェック
        if echo "$context" | jq . >/dev/null 2>&1; then
            context_json="$context"
        else
            # 文字列の場合はJSONオブジェクトに変換
            context_json=$(jq -n --arg context "$context" '{message: $context}' 2>/dev/null || echo "null")
        fi
    fi
    
    # メタデータをJSON形式に変換
    local metadata_json="null"
    if [ -n "$metadata" ]; then
        if echo "$metadata" | jq . >/dev/null 2>&1; then
            metadata_json="$metadata"
        else
            metadata_json="null"
        fi
    fi
    
    # ログエントリの作成と出力
    local log_entry=$(_create_log_entry "$level" "$message" "$component" "$context_json" "$metadata_json")
    if [ -n "$log_entry" ]; then
        _write_log "$log_entry"
    fi
}

# 便利関数: TRACE
log_trace() {
    local message="$1"
    local component="${2:-aish}"
    local context="${3:-}"
    local metadata="${4:-}"
    
    log_write $LOG_LEVEL_TRACE "$message" "$component" "$context" "$metadata"
}

# 便利関数: DEBUG
log_debug() {
    local message="$1"
    local component="${2:-aish}"
    local context="${3:-}"
    local metadata="${4:-}"
    
    log_write $LOG_LEVEL_DEBUG "$message" "$component" "$context" "$metadata"
}

# 便利関数: INFO
log_info() {
    local message="$1"
    local component="${2:-aish}"
    local context="${3:-}"
    local metadata="${4:-}"
    
    log_write $LOG_LEVEL_INFO "$message" "$component" "$context" "$metadata"
}

# 便利関数: WARN
log_warn() {
    local message="$1"
    local component="${2:-aish}"
    local context="${3:-}"
    local metadata="${4:-}"
    
    log_write $LOG_LEVEL_WARN "$message" "$component" "$context" "$metadata"
}

# 便利関数: ERROR
log_error() {
    local message="$1"
    local component="${2:-aish}"
    local context="${3:-}"
    local metadata="${4:-}"
    
    log_write $LOG_LEVEL_ERROR "$message" "$component" "$context" "$metadata"
}

# 便利関数: FATAL
log_fatal() {
    local message="$1"
    local component="${2:-aish}"
    local context="${3:-}"
    local metadata="${4:-}"
    
    log_write $LOG_LEVEL_FATAL "$message" "$component" "$context" "$metadata"
}

# 後方互換性: 既存のログ関数のラッパー

# detail.aish_log_request の置き換え
log_request() {
    local payload="$1"
    local component="${2:-llm}"
    
    # 既存のlog.jsonにも記録（後方互換性）
    if [ -n "$AISH_SESSION" ] && [ -n "$LOG" ] && [ -f "$LOG" ]; then
        echo '{"type":"request","timestamp":"'$(date +%Y-%m-%dT%H:%M:%S.%NZ)'", "payload":' >> "$LOG"
        echo "$payload"'}' >> "$LOG"
    fi
    
    # 新しいログシステムにも記録（jqが失敗しても続行）
    local context_json=$(jq -n --arg payload "$payload" '{payload: $payload}' 2>/dev/null || echo '{}')
    log_info "LLM Request" "$component" "$context_json"
}

# detail.aish_log_response の置き換え
log_response() {
    local payload="$1"
    local component="${2:-llm}"
    
    # 既存のlog.jsonにも記録（後方互換性）
    if [ -n "$AISH_SESSION" ] && [ -n "$LOG" ] && [ -f "$LOG" ]; then
        echo '{"type":"response","timestamp":"'$(date +%Y-%m-%dT%H:%M:%S.%NZ)'", "payload":' >> "$LOG"
        echo "$payload"'}' >> "$LOG"
    fi
    
    # 新しいログシステムにも記録（jqが失敗しても続行）
    local context_json=$(jq -n --arg payload "$payload" '{payload: $payload}' 2>/dev/null || echo '{}')
    log_info "LLM Response" "$component" "$context_json"
}

# detail.aish_log_tool の置き換え
log_tool() {
    local message="$1"
    local component="${2:-tool}"
    
    log_info "$message" "$component"
    
    # 既存の出力形式も維持（後方互換性）
    echo -e "\033[0;32m[Tool] $message\033[0m" >&2
}

# ロガーの初期化
logger_init() {
    # ログファイルの初期化（セッションが存在する場合）
    if [ -n "$AISH_SESSION" ] && [ -d "$AISH_SESSION" ]; then
        local app_log="${AISH_SESSION}/app.log"
        # ファイルが存在しない場合は作成（空のJSON配列）
        if [ ! -f "$app_log" ]; then
            echo "[]" > "$app_log"
        fi
    fi
}

