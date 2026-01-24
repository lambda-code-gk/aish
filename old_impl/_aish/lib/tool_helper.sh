#!/usr/bin/env bash

# tool定義と実行処理を集約するヘルパーライブラリ
# 各toolファイル（tool_*.sh）からtool定義を動的に読み込み、実行処理を呼び出す

# 全てのtoolファイルを読み込む
function _load_all_tool_files
{
  local tool_dir="$AISH_HOME/lib"
  
  # tool_*.shファイルを検索して読み込む
  for tool_file in "$tool_dir"/tool_*.sh; do
    if [ -f "$tool_file" ]; then
      # ファイルを読み込む（エラーがあっても続行）
      . "$tool_file" 2>/dev/null || true
    fi
  done
}

# OpenAI形式の全てのtool定義を集約して返す
function _load_all_tool_definitions_openai
{
  local tool_dir="$AISH_HOME/lib"
  local definitions="[]"
  
  # tool_*.shファイルを検索して読み込む
  for tool_file in "$tool_dir"/tool_*.sh; do
    if [ ! -f "$tool_file" ]; then
      continue
    fi
    
    # toolファイルを読み込む（エラーがあっても続行）
    . "$tool_file" 2>/dev/null || true
    
    # ファイル名からtool名を抽出（例: tool_execute_shell_command.sh -> execute_shell_command）
    tool_basename=$(basename "$tool_file" .sh)
    tool_name=$(echo "$tool_basename" | sed 's/^tool_//')
    
    # tool定義関数を呼び出す（例: _tool_execute_shell_command_definition_openai）
    definition_func="_tool_${tool_name}_definition_openai"
    
    if type "$definition_func" >/dev/null 2>&1; then
      definition=$($definition_func)
      if [ $? -eq 0 ] && [ ! -z "$definition" ]; then
        definitions=$(echo "$definitions" | jq --argjson def "$definition" '. += [$def]')
      fi
    fi
  done
  
  echo "$definitions"
}

# Gemini形式の全てのtool定義を集約して返す
function _load_all_tool_definitions_gemini
{
  local tool_dir="$AISH_HOME/lib"
  local definitions="[]"
  
  # tool_*.shファイルを検索して読み込む
  for tool_file in "$tool_dir"/tool_*.sh; do
    if [ ! -f "$tool_file" ]; then
      continue
    fi
    
    # toolファイルを読み込む（エラーがあっても続行）
    . "$tool_file" 2>/dev/null || true
    
    # ファイル名からtool名を抽出（例: tool_execute_shell_command.sh -> execute_shell_command）
    tool_basename=$(basename "$tool_file" .sh)
    tool_name=$(echo "$tool_basename" | sed 's/^tool_//')
    
    # tool定義関数を呼び出す（例: _tool_execute_shell_command_definition_gemini）
    definition_func="_tool_${tool_name}_definition_gemini"
    
    if type "$definition_func" >/dev/null 2>&1; then
      definition=$($definition_func)
      if [ $? -eq 0 ] && [ ! -z "$definition" ]; then
        definitions=$(echo "$definitions" | jq --argjson def "$definition" '. += [$def]')
      fi
    fi
  done
  
  echo "$definitions"
}

# tool実行処理を呼び出す
# 引数: tool_name - tool名（例: "execute_shell_command"）
#      tool_call_id - tool call ID（OpenAI形式のみ使用、Gemini形式では無視）
#      func_args - 関数引数（JSON文字列）
#      provider - "openai" または "gemini"
# 戻り値: tool実行結果（JSON形式）
function _execute_tool_call
{
  local tool_name="$1"
  local tool_call_id="$2"
  local func_args="$3"
  local provider="$4"
  
  if [ -z "$tool_name" ] || [ -z "$func_args" ] || [ -z "$provider" ]; then
    echo '{"error": "tool_name, func_args, and provider are required"}' >&2
    return 1
  fi
  
  # tool実行関数を呼び出す（例: _tool_execute_shell_command_execute）
  execute_func="_tool_${tool_name}_execute"
  
  if ! type "$execute_func" >/dev/null 2>&1; then
    echo "{\"error\": \"Tool execution function not found: $execute_func\"}" >&2
    return 1
  fi
  
  # tool実行処理を呼び出す
  result=$($execute_func "$tool_call_id" "$func_args" "$provider")
  
  if [ $? -ne 0 ]; then
    return 1
  fi
  
  echo "$result"
}

