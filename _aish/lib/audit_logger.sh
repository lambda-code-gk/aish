#!/usr/bin/env bash

# 監査ログライブラリ
# セキュリティ関連イベントを構造化された形式で記録・検索する機能を提供

# 監査ログファイルのパスを取得
_audit_get_log_file() {
    if [ -n "$AISH_SESSION" ] && [ -d "$AISH_SESSION" ]; then
        echo "${AISH_SESSION}/audit.log"
    else
        echo ""
    fi
}

# 現在のユーザー名を取得
_audit_get_user() {
    whoami 2>/dev/null || echo "${USER:-${LOGNAME:-unknown}}"
}

# 現在のセッションIDを取得
_audit_get_session_id() {
    if [ -n "$AISH_SESSION" ]; then
        basename "$AISH_SESSION" 2>/dev/null || echo "unknown"
    else
        echo "unknown"
    fi
}

# タイムスタンプを取得（ISO 8601形式、ナノ秒精度、タイムゾーン付き）
_audit_get_timestamp() {
    date +"%Y-%m-%dT%H:%M:%S.%N%z" 2>/dev/null || date +"%Y-%m-%dT%H:%M:%S%z"
}

# イベントタイプからセキュリティレベルを判定
_audit_get_security_level() {
    local event_type="$1"
    
    case "$event_type" in
        command_execution|sensitive_info_detected)
            echo "critical"
            ;;
        command_rejection|sensitive_info_approved)
            echo "high"
            ;;
        command_approval|sensitive_info_masked)
            # command_approvalは後で承認方法により調整される可能性がある
            echo "medium"
            ;;
        session_created|session_deleted|session_resumed)
            echo "low"
            ;;
        *)
            echo "medium"
            ;;
    esac
}

# 監査ログエントリを作成
_audit_create_entry() {
    local event_type="$1"
    local component="$2"
    local context_json="${3:-{}}"
    local metadata_json="${4:-{}}"
    
    # 必須パラメータのチェック
    if [ -z "$event_type" ] || [ -z "$component" ]; then
        echo "Error: event_type and component are required" >&2
        return 1
    fi
    
    # コンテキストとメタデータがJSONとして有効かチェック
    local context_valid=false
    local metadata_valid=false
    
    if [ -z "$context_json" ] || [ "$context_json" = "{}" ]; then
        context_json="{}"
        context_valid=true
    else
        if echo "$context_json" | jq . >/dev/null 2>&1; then
            context_valid=true
        fi
    fi
    
    if [ -z "$metadata_json" ] || [ "$metadata_json" = "{}" ]; then
        metadata_json="{}"
        metadata_valid=true
    else
        if echo "$metadata_json" | jq . >/dev/null 2>&1; then
            metadata_valid=true
        fi
    fi
    
    # JSONとして無効な場合は空オブジェクトにフォールバック
    if [ "$context_valid" = false ]; then
        context_json="{}"
    fi
    if [ "$metadata_valid" = false ]; then
        metadata_json="{}"
    fi
    
    local timestamp=$(_audit_get_timestamp)
    local user=$(_audit_get_user)
    local session_id=$(_audit_get_session_id)
    local security_level=$(_audit_get_security_level "$event_type")
    
    # JSONエントリを作成
    local entry
    entry=$(jq -n \
        --arg timestamp "$timestamp" \
        --arg event_type "$event_type" \
        --arg session_id "$session_id" \
        --arg user "$user" \
        --arg component "$component" \
        --argjson context "$context_json" \
        --arg security_level "$security_level" \
        --argjson metadata "$metadata_json" \
        '{
            timestamp: $timestamp,
            event_type: $event_type,
            session_id: $session_id,
            user: $user,
            component: $component,
            context: $context,
            security_level: $security_level,
            metadata: $metadata
        }' 2>/dev/null)
    
    if [ -z "$entry" ]; then
        # jqが失敗した場合のフォールバック（簡易JSON生成）
        echo "Warning: jq is not available, using fallback JSON generation" >&2
        entry="{"
        entry+="\"timestamp\":\"$timestamp\","
        entry+="\"event_type\":\"$event_type\","
        entry+="\"session_id\":\"$session_id\","
        entry+="\"user\":\"$user\","
        entry+="\"component\":\"$component\","
        entry+="\"context\":$context_json,"
        entry+="\"security_level\":\"$security_level\","
        entry+="\"metadata\":$metadata_json"
        entry+="}"
    fi
    
    echo "$entry"
}

# キー・値のペアからJSONオブジェクトを構築
# 引数: "key1" "value1" "key2" "value2" ... (偶数個の引数)
_audit_build_json_from_pairs() {
    if [ $# -eq 0 ]; then
        echo "{}"
        return 0
    fi
    
    # 引数が偶数個でない場合はエラー
    if [ $(($# % 2)) -ne 0 ]; then
        echo "{}"
        return 1
    fi
    
    # jqが利用可能な場合はjqを使用
    if command -v jq >/dev/null 2>&1; then
        local jq_args=()
        local i=1
        while [ $i -le $# ]; do
            local key="${!i}"
            i=$((i + 1))
            local value="${!i}"
            i=$((i + 1))
            jq_args+=("--arg" "$key" "$value")
        done
        jq -n "${jq_args[@]}" 'reduce ($ARGS.named | to_entries | .[]) as $item ({}; .[$item.key] = $item.value)' 2>/dev/null || echo "{}"
    else
        # jqがない場合のフォールバック（簡易実装）
        echo "{}"
    fi
}

# 監査ログを記録（高レベルAPI: キー・値のペアを受け取る）
# 引数:
#   $1: event_type - イベントタイプ（必須）
#   $2: component - コンポーネント名（必須）
#   $3以降: キー・値のペア（context用）
#   "--metadata" 以降: メタデータ用のキー・値のペア（オプション）
# 例: audit_log_with_fields "command_approval" "tool" "command" "ls" "approval_method" "global_list"
#     audit_log_with_fields "command_approval" "tool" "command" "ls" "--metadata" "tool_call_id" "abc123"
audit_log_with_fields() {
    local event_type="$1"
    local component="$2"
    shift 2
    
    if [ -z "$event_type" ] || [ -z "$component" ]; then
        echo "Error: audit_log_with_fields requires event_type and component" >&2
        return 1
    fi
    
    # 引数を分割: "--metadata" が見つかるまでをcontext、以降をmetadata
    local context_args=()
    local metadata_args=()
    local found_metadata=false
    
    for arg in "$@"; do
        if [ "$arg" = "--metadata" ]; then
            found_metadata=true
            continue
        fi
        if [ "$found_metadata" = true ]; then
            metadata_args+=("$arg")
        else
            context_args+=("$arg")
        fi
    done
    
    # JSONを構築
    local context_json
    if [ ${#context_args[@]} -gt 0 ]; then
        context_json=$(_audit_build_json_from_pairs "${context_args[@]}")
    else
        context_json="{}"
    fi
    
    local metadata_json
    if [ ${#metadata_args[@]} -gt 0 ]; then
        metadata_json=$(_audit_build_json_from_pairs "${metadata_args[@]}")
    else
        metadata_json="{}"
    fi
    
    # audit_logを呼び出し
    audit_log "$event_type" "$component" "$context_json" "$metadata_json"
}

# 監査ログを安全に記録（関数が存在しない場合は何もしない）
# 引数は audit_log() と同じ（下位互換性のため残す）
audit_log_safe() {
    if type audit_log >/dev/null 2>&1; then
        audit_log "$@" 2>/dev/null || true
    fi
}

# 監査ログを安全に記録（高レベルAPI版）
audit_log_with_fields_safe() {
    if type audit_log_with_fields >/dev/null 2>&1; then
        audit_log_with_fields "$@" 2>/dev/null || true
    fi
}

# 監査ログを記録
# 引数:
#   $1: event_type - イベントタイプ（必須）
#   $2: component - コンポーネント名（必須）
#   $3: context - コンテキスト情報（JSON文字列、オプション）
#   $4: metadata - メタデータ（JSON文字列、オプション）
audit_log() {
    local event_type="$1"
    local component="$2"
    local context_json="${3:-{}}"
    local metadata_json="${4:-{}}"
    
    # 必須パラメータのチェック
    if [ -z "$event_type" ] || [ -z "$component" ]; then
        echo "Error: audit_log requires event_type and component arguments" >&2
        return 1
    fi
    
    # 監査ログファイルのパスを取得
    local log_file=$(_audit_get_log_file)
    
    # セッションディレクトリが存在しない場合は警告してスキップ
    if [ -z "$log_file" ]; then
        echo "Warning: AISH_SESSION is not set, skipping audit log" >&2
        return 0
    fi
    
    # ログエントリを作成
    local entry=$(_audit_create_entry "$event_type" "$component" "$context_json" "$metadata_json")
    
    if [ -z "$entry" ]; then
        echo "Warning: Failed to create audit log entry" >&2
        return 1
    fi
    
    # ログファイルに追記（エラー時も処理を継続）
    if echo "$entry" >> "$log_file" 2>/dev/null; then
        return 0
    else
        echo "Warning: Failed to write audit log to $log_file" >&2
        return 1
    fi
}

# 監査ログを検索・フィルタリング
# 引数:
#   --session <session_id> - セッションIDでフィルタ
#   --event-type <type> - イベントタイプでフィルタ
#   --from <timestamp> - 開始時刻（ISO 8601形式）
#   --to <timestamp> - 終了時刻（ISO 8601形式）
#   --component <component> - コンポーネントでフィルタ
#   --security-level <level> - セキュリティレベルでフィルタ
audit_query() {
    local session_id=""
    local event_type=""
    local from_time=""
    local to_time=""
    local component=""
    local security_level=""
    
    # オプション解析
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --session)
                session_id="$2"
                shift 2
                ;;
            --event-type)
                event_type="$2"
                shift 2
                ;;
            --from)
                from_time="$2"
                shift 2
                ;;
            --to)
                to_time="$2"
                shift 2
                ;;
            --component)
                component="$2"
                shift 2
                ;;
            --security-level)
                security_level="$2"
                shift 2
                ;;
            *)
                echo "Unknown option: $1" >&2
                return 1
                ;;
        esac
    done
    
    # ログファイルのパスを取得
    local log_file=$(_audit_get_log_file)
    
    # セッションディレクトリが設定されていない場合は、sessionsディレクトリから検索
    if [ -z "$log_file" ] || [ ! -f "$log_file" ]; then
        local sessions_dir="${AISH_HOME:-$HOME/.aish}/sessions"
        if [ -d "$sessions_dir" ] && [ -n "$session_id" ]; then
            log_file="$sessions_dir/$session_id/audit.log"
        fi
    fi
    
    # ログファイルが存在しない場合
    if [ -z "$log_file" ] || [ ! -f "$log_file" ]; then
        echo "Audit log file not found" >&2
        return 1
    fi
    
    # jqが利用可能かチェック
    if ! command -v jq >/dev/null 2>&1; then
        echo "Error: jq is required for audit_query" >&2
        return 1
    fi
    
    # フィルタリング用のjqフィルタを構築
    local filter="."
    
    if [ -n "$session_id" ]; then
        filter+=" | select(.session_id == \"$session_id\")"
    fi
    
    if [ -n "$event_type" ]; then
        filter+=" | select(.event_type == \"$event_type\")"
    fi
    
    if [ -n "$component" ]; then
        filter+=" | select(.component == \"$component\")"
    fi
    
    if [ -n "$security_level" ]; then
        filter+=" | select(.security_level == \"$security_level\")"
    fi
    
    # 時刻フィルタ（簡易実装）
    if [ -n "$from_time" ] || [ -n "$to_time" ]; then
        # タイムスタンプ比較用のフィルタ
        if [ -n "$from_time" ]; then
            filter+=" | select(.timestamp >= \"$from_time\")"
        fi
        if [ -n "$to_time" ]; then
            filter+=" | select(.timestamp <= \"$to_time\")"
        fi
    fi
    
    # JSONL形式のファイルを読み込んでフィルタリング
    while IFS= read -r line || [ -n "$line" ]; do
        if [ -z "$line" ]; then
            continue
        fi
        
        # 各行をjqで処理
        echo "$line" | jq "$filter" 2>/dev/null
    done < "$log_file" | jq -s '.' 2>/dev/null
}

