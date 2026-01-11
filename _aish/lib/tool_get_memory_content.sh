#!/usr/bin/env bash

# memory_manager.shの関数を使用するため、読み込む
. "$AISH_HOME/lib/memory_manager.sh"

# OpenAI形式のtool定義を返す
function _tool_get_memory_content_definition_openai
{
  echo '{"type": "function", "function": {"name": "get_memory_content", "description": "Get the full content of a specific memory by its ID. Use this after search_memory returns memory IDs to retrieve detailed information when needed.", "parameters": {"type": "object", "properties": {"memory_id": {"type": "string", "description": "The memory ID returned by search_memory"}}, "required": ["memory_id"]}}}'
}

# Gemini形式のtool定義を返す
function _tool_get_memory_content_definition_gemini
{
  echo '{"name": "get_memory_content", "description": "Get the full content of a specific memory by its ID. Use this after search_memory returns memory IDs to retrieve detailed information when needed.", "parameters": {"type": "object", "properties": {"memory_id": {"type": "string", "description": "The memory ID returned by search_memory"}}, "required": ["memory_id"]}}'
}

# tool実行処理
# 引数: tool_call_id - tool call ID（OpenAI形式のみ使用）
#      func_args - 関数引数（JSON文字列）
#      provider - "openai" または "gemini"
# 戻り値: tool実行結果（JSON形式）
function _tool_get_memory_content_execute
{
  local tool_call_id="$1"
  local func_args="$2"
  local provider="$3"
  
  memory_id=$(echo "$func_args" | jq -r '.memory_id')
  
  if [ -z "$memory_id" ] || [ "$memory_id" = "null" ]; then
    echo '{"error": "memory_id is required"}' >&2
    return 1
  fi
  
  # プロジェクト固有の記憶ディレクトリを取得
  local memory_dir
  memory_dir=$(find_memory_directory)
  
  # プロジェクト固有から検索
  result=$(memory_system_get_by_id "$memory_dir" "$memory_id")
  
  if [ ! -z "$result" ] && [ "$result" != "null" ]; then
    # 標準エラー出力に検索結果を表示: id,subject
    local subject=$(echo "$result" | jq -r '.subject // ""' 2>/dev/null)
    if [ ! -z "$subject" ] && [ "$subject" != "null" ]; then
      echo "get_memory_content: $memory_id,$subject" >&2
    else
      echo "get_memory_content: $memory_id," >&2
    fi
    echo "$result"
    return 0
  fi
  
  # 見つからない場合はグローバルから検索
  result=$(memory_system_get_by_id "$AISH_HOME/memory" "$memory_id")
  
  if [ ! -z "$result" ] && [ "$result" != "null" ]; then
    # 標準エラー出力に検索結果を表示: id,subject
    local subject=$(echo "$result" | jq -r '.subject // ""' 2>/dev/null)
    if [ ! -z "$subject" ] && [ "$subject" != "null" ]; then
      echo "$memory_id,$subject" >&2
    else
      echo "$memory_id," >&2
    fi
    echo "$result"
    return 0
  fi
  
  # 見つからない場合
  echo "get_memory_content: $memory_id not found" >&2
  echo "{\"error\": \"Memory with ID $memory_id not found\"}"
  return 1
}

