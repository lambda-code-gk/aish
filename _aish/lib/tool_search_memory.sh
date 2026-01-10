#!/usr/bin/env bash

# memory_manager.shの関数を使用するため、読み込む
. "$AISH_HOME/lib/memory_manager.sh"

# OpenAI形式のtool定義を返す
function _tool_search_memory_definition_openai
{
  echo '{"type": "function", "function": {"name": "search_memory", "description": "Search memories related to the query. Searches both project-specific and global memories.", "parameters": {"type": "object", "properties": {"query": {"type": "string", "description": "Search query"}, "category": {"type": "string", "description": "Filter by category (optional)"}, "limit": {"type": "integer", "description": "Maximum number of results", "default": 5}}, "required": ["query"]}}}'
}

# Gemini形式のtool定義を返す
function _tool_search_memory_definition_gemini
{
  echo '{"name": "search_memory", "description": "Search memories related to the query. Searches both project-specific and global memories.", "parameters": {"type": "object", "properties": {"query": {"type": "string", "description": "Search query"}, "category": {"type": "string", "description": "Filter by category (optional)"}, "limit": {"type": "integer", "description": "Maximum number of results", "default": 5}}, "required": ["query"]}}'
}

# tool実行処理
# 引数: tool_call_id - tool call ID（OpenAI形式のみ使用）
#      func_args - 関数引数（JSON文字列）
#      provider - "openai" または "gemini"
# 戻り値: tool実行結果（JSON形式）
function _tool_search_memory_execute
{
  local tool_call_id="$1"
  local func_args="$2"
  local provider="$3"
  
  query_str=$(echo "$func_args" | jq -r '.query')
  category=$(echo "$func_args" | jq -r '.category // ""')
  limit=$(echo "$func_args" | jq -r '.limit // 5')
  
  if [ -z "$query_str" ]; then
    echo '{"error": "query is required"}' >&2
    return 1
  fi
  
  result=$(search_memory_efficient "$query_str" "$category" "$limit")
  
  if [ $? -ne 0 ]; then
    return 1
  fi
  
  echo "$result"
}

