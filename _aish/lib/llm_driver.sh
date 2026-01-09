#!/usr/bin/env bash

# LLMプロバイダドライバー
# このライブラリは、異なるLLMプロバイダ（GPT、Geminiなど）で共通する処理を提供します。
# プロバイダ固有の処理は、各プロバイダファイルで以下の関数を実装する必要があります:
#   - _provider_make_http_request: HTTPリクエストの実行
#   - _provider_parse_response_text: レスポンスからテキストを抽出
#   - _provider_check_tool_calls: tool/function callの有無をチェック
#   - _provider_process_tool_calls: tool/function callを処理

# query関数の共通実装
# 引数: クエリ関数と同じ引数
function _llm_driver_query
{
    query_entry_prepare "$@"
    
    if [ "$_query_agent_mode" = true ]; then
        echo -e "$_query_files" | _provider_make_request_payload_agent "$_query_args" "$_query_system_instruction" | _llm_driver_send_request_with_tools
    else
        echo -e "$_query_files" | _provider_make_request_payload "$_query_args" "$_query_system_instruction" | _llm_driver_send_request
    fi
}

# send_to_llmの共通実装
# 標準入力からリクエストデータを読み取り、LLMに送信してレスポンスを処理します。
function _llm_driver_send_request
{
    if [ -z "$AISH_SESSION" ]; then
        echo "Error: AISH_SESSION is not set" >&2
        exit 1
    fi
    REQUEST_FILE="$AISH_SESSION/request.txt"
    cat > "$REQUEST_FILE"
    if [ -f "$REQUEST_FILE" ]; then
        request_data=$(cat "$REQUEST_FILE")
    else
        echo "Error: Failed to create request file" >&2
        exit 1
    fi

    detail.aish_log_request "$request_data"

    # プロバイダ固有のHTTPリクエストを実行
    # 注意: response=$(...) の前に一時ファイルに出力し、終了コードを取得してから読み込む
    if [ -z "$AISH_SESSION" ]; then
        echo "Error: AISH_SESSION is not set" >&2
        exit 1
    fi
    temp_response_file="$AISH_SESSION/temp_response_$$.json"
    _provider_make_http_request "$REQUEST_FILE" > "$temp_response_file"
    http_exit_code=$?
    if [ -f "$temp_response_file" ]; then
        response=$(cat "$temp_response_file")
        rm -f "$temp_response_file"
    else
        response=""
    fi
    
    if [ $http_exit_code -ne 0 ]; then
        echo "$response" >&2
        exit 1
    fi

    detail.aish_log_response "$response"

    # プロバイダ固有のレスポンス解析
    text=$(_provider_parse_response_text "$response")
    
    if [ "$text" == "null" ] || [ -z "$text" ]; then
        echo "$response"
        exit 1
    fi

    save_response_text "$text"
}

# send_to_llm_agentの共通実装（イテレーションループ）
# 標準入力からリクエストデータを読み取り、tool/function callを処理しながら
# 最大MAX_ITERATIONS回までイテレーションを繰り返します。
function _llm_driver_send_request_with_tools
{
    if [ -z "$AISH_SESSION" ]; then
        echo "Error: AISH_SESSION is not set" >&2
        exit 1
    fi
    REQUEST_FILE="$AISH_SESSION/request.txt"
    MAX_ITERATIONS=20
    iteration=0
    
    # 初期リクエストを保存
    cat > "$REQUEST_FILE"
    if [ ! -f "$REQUEST_FILE" ]; then
        echo "Error: Failed to create request file" >&2
        exit 1
    fi
    
    while [ $iteration -lt $MAX_ITERATIONS ]; do
        iteration=$((iteration + 1))
        if [ -f "$REQUEST_FILE" ]; then
            request_data=$(cat "$REQUEST_FILE")
        else
            echo "Error: Request file not found" >&2
            exit 1
        fi

        detail.aish_log_request "$request_data"

        # プロバイダ固有のHTTPリクエストを実行
        # 注意: response=$(...) の前に一時ファイルに出力し、終了コードを取得してから読み込む
        if [ -z "$AISH_SESSION" ]; then
            echo "Error: AISH_SESSION is not set" >&2
            exit 1
        fi
        temp_response_file="$AISH_SESSION/temp_response_$$.json"
        _provider_make_http_request "$REQUEST_FILE" > "$temp_response_file"
        http_exit_code=$?
        if [ -f "$temp_response_file" ]; then
            response=$(cat "$temp_response_file")
            rm -f "$temp_response_file"
        else
            response=""
        fi
        
        if [ $http_exit_code -ne 0 ]; then
            echo "$response" >&2
            exit 1
        fi

        detail.aish_log_response "$response"

        # エラーチェック
        error=$(echo "$response" | jq -r '.error.message // empty' 2>/dev/null)
        if [ ! -z "$error" ]; then
            echo "$response" >&2
            exit 1
        fi

        # プロバイダ固有のtool/function callチェック
        has_tool_calls=$(_provider_check_tool_calls "$response")
        
        if [ "$has_tool_calls" = "yes" ]; then
            # tool/function callがある場合、プロバイダ固有の処理を実行
            updated_request=$(_provider_process_tool_calls "$request_data" "$response")
            
            if [ $? -ne 0 ]; then
                exit 1
            fi
            
            echo "$updated_request" > "$REQUEST_FILE"
            
            # 次のイテレーションに進む
            continue
        else
            # tool/function callがない場合、テキスト応答を返して終了
            text=$(_provider_parse_response_text "$response")
            
            if [ "$text" == "null" ] || [ -z "$text" ]; then
                echo "$response" >&2
                exit 1
            fi

            save_response_text "$text"
            return 0
        fi
    done
    
    echo "Error: Maximum iterations ($MAX_ITERATIONS) reached" >&2
    exit 1
}

