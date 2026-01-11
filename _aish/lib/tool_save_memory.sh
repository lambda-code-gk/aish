#!/usr/bin/env bash

# memory_manager.shの関数を使用するため、読み込む
. "$AISH_HOME/lib/memory_manager.sh"

# OpenAI形式のtool定義を返す
function _tool_save_memory_definition_openai
{
  echo '{"type": "function", "function": {"name": "save_memory", "description": "Save useful information to the memory system. The memory will be stored in the project-specific directory if .aish/memory exists, otherwise in the global directory.", "parameters": {"type": "object", "properties": {"content": {"type": "string", "description": "The content to remember"}, "category": {"type": "string", "description": "Category: code_pattern, error_solution, workflow, best_practice, configuration, etc.", "default": "general"}, "keywords": {"type": "array", "items": {"type": "string"}, "description": "Keywords for searching this memory later"}, "subject": {"type": "string", "description": "A brief subject or title describing what this memory is about"}}, "required": ["content"]}}}'
}

# Gemini形式のtool定義を返す
function _tool_save_memory_definition_gemini
{
  echo '{"name": "save_memory", "description": "Save useful information to the memory system. The memory will be stored in the project-specific directory if .aish/memory exists, otherwise in the global directory.", "parameters": {"type": "object", "properties": {"content": {"type": "string", "description": "The content to remember"}, "category": {"type": "string", "description": "Category: code_pattern, error_solution, workflow, best_practice, configuration, etc.", "default": "general"}, "keywords": {"type": "array", "items": {"type": "string"}, "description": "Keywords for searching this memory later"}, "subject": {"type": "string", "description": "A brief subject or title describing what this memory is about"}}, "required": ["content"]}}'
}

# tool実行処理
# 引数: tool_call_id - tool call ID（OpenAI形式のみ使用）
#      func_args - 関数引数（JSON文字列）
#      provider - "openai" または "gemini"
# 戻り値: tool実行結果（JSON形式）
function _tool_save_memory_execute
{
  local tool_call_id="$1"
  local func_args="$2"
  local provider="$3"
  
  content=$(echo "$func_args" | jq -r '.content')
  category=$(echo "$func_args" | jq -r '.category // "general"')
  keywords=$(echo "$func_args" | jq -r '.keywords // [] | join(",")')
  subject=$(echo "$func_args" | jq -r '.subject // ""')
  
  if [ -z "$content" ]; then
    echo '{"error": "content is required"}' >&2
    return 1
  fi
  
  result=$(save_memory "$content" "$category" "$keywords" "$subject")
  
  if [ $? -ne 0 ]; then
    return 1
  fi
  
  echo "$result"
}

