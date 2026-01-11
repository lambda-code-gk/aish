#!/usr/bin/env bash

# functionsファイルのjson_string関数を使用するため、読み込む
. "$AISH_HOME/functions"

# パッチを適用する関数
function apply_patch
{
  local patch="$1"
  
  if [ -z "$patch" ]; then
    echo '{"error": "patch is required"}' >&2
    return 1
  fi
  
  # 一時ファイルにパッチを保存
  if [ -z "$AISH_SESSION" ]; then
    local patch_file=$(mktemp)
  else
    local patch_file=$(mktemp "$AISH_SESSION/patch_XXXXXX" 2>/dev/null || mktemp)
  fi
  echo "$patch" > "$patch_file"
  
  # patchコマンドを実行（-p1はデフォルトのprefixレベル）
  # -u: unified diff形式
  # -d: 作業ディレクトリ（現在のディレクトリ）
  patch_output=$(patch -p1 -u -d "$(pwd)" < "$patch_file" 2>&1)
  patch_exit_code=$?
  
  rm -f "$patch_file"
  
  if [ $patch_exit_code -eq 0 ]; then
    result="{\"success\": true, \"output\": $(echo "$patch_output" | json_string)}"
    echo "$result"
    return 0
  else
    result="{\"success\": false, \"error\": $(echo "$patch_output" | json_string)}"
    echo "$result" >&2
    return 1
  fi
}

# パッチの適用を確認する関数（適用しない）
function dry_run_patch
{
  local patch="$1"
  
  if [ -z "$patch" ]; then
    echo '{"error": "patch is required"}' >&2
    return 1
  fi
  
  # 一時ファイルにパッチを保存
  if [ -z "$AISH_SESSION" ]; then
    local patch_file=$(mktemp)
  else
    local patch_file=$(mktemp "$AISH_SESSION/patch_XXXXXX" 2>/dev/null || mktemp)
  fi
  echo "$patch" > "$patch_file"
  
  # patchコマンドをdry-runモードで実行（--dry-runオプション）
  patch_output=$(patch -p1 -u --dry-run -d "$(pwd)" < "$patch_file" 2>&1)
  patch_exit_code=$?
  
  rm -f "$patch_file"
  
  if [ $patch_exit_code -eq 0 ]; then
    result="{\"success\": true, \"can_apply\": true, \"output\": $(echo "$patch_output" | json_string)}"
    echo "$result"
    return 0
  else
    result="{\"success\": true, \"can_apply\": false, \"error\": $(echo "$patch_output" | json_string)}"
    echo "$result"
    return 0
  fi
}

# OpenAI形式のtool定義を返す（apply_patch）
function _tool_apply_patch_definition_openai
{
  echo '{"type": "function", "function": {"name": "apply_patch", "description": "Apply a unified diff patch to files. The patch will be applied to files in the current working directory.", "parameters": {"type": "object", "properties": {"patch": {"type": "string", "description": "The unified diff patch to apply"}}, "required": ["patch"]}}}'
}

# Gemini形式のtool定義を返す（apply_patch）
function _tool_apply_patch_definition_gemini
{
  echo '{"name": "apply_patch", "description": "Apply a unified diff patch to files. The patch will be applied to files in the current working directory.", "parameters": {"type": "object", "properties": {"patch": {"type": "string", "description": "The unified diff patch to apply"}}, "required": ["patch"]}}'
}

# tool実行処理（apply_patch）
function _tool_apply_patch_execute
{
  local tool_call_id="$1"
  local func_args="$2"
  local provider="$3"
  
  patch=$(echo "$func_args" | jq -r '.patch')
  
  if [ -z "$patch" ] || [ "$patch" = "null" ]; then
    echo '{"error": "patch is required"}' >&2
    return 1
  fi
  
  # パッチを適用
  result=$(apply_patch "$patch")
  
  if [ $? -ne 0 ]; then
    return 1
  fi
  
  echo "$result"
}

# OpenAI形式のtool定義を返す（dry_run_patch）
function _tool_dry_run_patch_definition_openai
{
  echo '{"type": "function", "function": {"name": "dry_run_patch", "description": "Check if a unified diff patch can be applied without actually applying it. This is useful for validating patches before applying them.", "parameters": {"type": "object", "properties": {"patch": {"type": "string", "description": "The unified diff patch to check"}}, "required": ["patch"]}}}'
}

# Gemini形式のtool定義を返す（dry_run_patch）
function _tool_dry_run_patch_definition_gemini
{
  echo '{"name": "dry_run_patch", "description": "Check if a unified diff patch can be applied without actually applying it. This is useful for validating patches before applying them.", "parameters": {"type": "object", "properties": {"patch": {"type": "string", "description": "The unified diff patch to check"}}, "required": ["patch"]}}'
}

# tool実行処理（dry_run_patch）
function _tool_dry_run_patch_execute
{
  local tool_call_id="$1"
  local func_args="$2"
  local provider="$3"
  
  patch=$(echo "$func_args" | jq -r '.patch')
  
  if [ -z "$patch" ] || [ "$patch" = "null" ]; then
    echo '{"error": "patch is required"}' >&2
    return 1
  fi
  
  # パッチをdry-run
  result=$(dry_run_patch "$patch")
  
  if [ $? -ne 0 ]; then
    return 1
  fi
  
  echo "$result"
}

