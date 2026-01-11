#!/usr/bin/env bash

# functionsファイルのjson_string関数を使用するため、読み込む
. "$AISH_HOME/functions"

# 検証を実行する関数
function run_validation
{
  local mode="${1:-auto}"
  local project_root="${2:-$(pwd)}"
  
  detail.aish_log_tool "run_validation $mode $project_root"

  case "$mode" in
    configured)
      run_validation_configured "$project_root"
      ;;
    auto)
      run_validation_auto "$project_root"
      ;;
    none)
      run_validation_none "$project_root"
      ;;
    *)
      echo '{"error": "invalid mode. must be one of: configured, auto, none"}' >&2
      return 1
      ;;
  esac
}

# configured mode: .aish/validate.jsonを読む
function run_validation_configured
{
  local project_root="$1"
  local validate_file="$project_root/.aish/validate.json"
  
  if [ ! -f "$validate_file" ]; then
    result="{\"success\": false, \"error\": \"validate.json not found at .aish/validate.json\", \"mode\": \"configured\"}"
    echo "$result" >&2
    return 1
  fi
  
  # jqでvalidate.jsonを読み込む
  if ! command -v jq >/dev/null 2>&1; then
    result="{\"success\": false, \"error\": \"jq is required but not installed\", \"mode\": \"configured\"}"
    echo "$result" >&2
    return 1
  fi
  
  # validate.jsonからcommandsを読み込む
  commands=$(jq -c '.commands // []' "$validate_file" 2>/dev/null)
  if [ $? -ne 0 ] || [ -z "$commands" ]; then
    result="{\"success\": false, \"error\": \"failed to parse validate.json\", \"mode\": \"configured\"}"
    echo "$result" >&2
    return 1
  fi
  
  # 各コマンドを実行
  cd "$project_root"
  local all_success=true
  local output=""
  local command_count=$(echo "$commands" | jq 'length')
  
  for i in $(seq 0 $((command_count - 1))); do
    command=$(echo "$commands" | jq -r ".[$i].command // empty")
    description=$(echo "$commands" | jq -r ".[$i].description // \"\"")
    
    if [ -z "$command" ] || [ "$command" = "null" ]; then
      continue
    fi
    
    # コマンドを実行
    command_output=$(bash -c "$command" 2>&1)
    command_exit_code=$?
    
    if [ $command_exit_code -ne 0 ]; then
      all_success=false
    fi
    
    output="${output}Command: ${command}"
    if [ ! -z "$description" ] && [ "$description" != "null" ]; then
      output="${output} (${description})"
    fi
    output="${output}\nExit code: ${command_exit_code}\n${command_output}\n---\n"
  done
  
  cd - > /dev/null
  
  if [ "$all_success" = "true" ]; then
    result="{\"success\": true, \"mode\": \"configured\", \"output\": $(echo -e "$output" | json_string)}"
    echo "$result"
    return 0
  else
    result="{\"success\": false, \"mode\": \"configured\", \"output\": $(echo -e "$output" | json_string)}"
    echo "$result" >&2
    return 1
  fi
}

# auto mode: 一般的な推測で検証を試す
function run_validation_auto
{
  local project_root="$1"
  cd "$project_root"
  
  # Makefileがある場合
  if [ -f "Makefile" ]; then
    # make testを試す
    if grep -q "^test:" Makefile 2>/dev/null || grep -q "^test:" Makefile 2>/dev/null; then
      command_output=$(make test 2>&1)
      command_exit_code=$?
      
      if [ $command_exit_code -eq 0 ]; then
        result="{\"success\": true, \"mode\": \"auto\", \"detected\": \"Makefile\", \"command\": \"make test\", \"output\": $(echo "$command_output" | json_string)}"
        echo "$result"
        cd - > /dev/null
        return 0
      else
        result="{\"success\": false, \"mode\": \"auto\", \"detected\": \"Makefile\", \"command\": \"make test\", \"output\": $(echo "$command_output" | json_string)}"
        echo "$result" >&2
        cd - > /dev/null
        return 1
      fi
    fi
  fi
  
  # package.jsonがある場合（Node.jsプロジェクト）
  if [ -f "package.json" ]; then
    if command -v npm >/dev/null 2>&1; then
      # npm testを試す
      if grep -q "\"test\"" package.json 2>/dev/null; then
        command_output=$(npm test 2>&1)
        command_exit_code=$?
        
        if [ $command_exit_code -eq 0 ]; then
          result="{\"success\": true, \"mode\": \"auto\", \"detected\": \"package.json\", \"command\": \"npm test\", \"output\": $(echo "$command_output" | json_string)}"
          echo "$result"
          cd - > /dev/null
          return 0
        else
          result="{\"success\": false, \"mode\": \"auto\", \"detected\": \"package.json\", \"command\": \"npm test\", \"output\": $(echo "$command_output" | json_string)}"
          echo "$result" >&2
          cd - > /dev/null
          return 1
        fi
      fi
    fi
  fi
  
  # pyproject.tomlがある場合（Pythonプロジェクト）
  if [ -f "pyproject.toml" ]; then
    if command -v pytest >/dev/null 2>&1; then
      command_output=$(pytest 2>&1)
      command_exit_code=$?
      
      if [ $command_exit_code -eq 0 ]; then
        result="{\"success\": true, \"mode\": \"auto\", \"detected\": \"pyproject.toml\", \"command\": \"pytest\", \"output\": $(echo "$command_output" | json_string)}"
        echo "$result"
        cd - > /dev/null
        return 0
      else
        result="{\"success\": false, \"mode\": \"auto\", \"detected\": \"pyproject.toml\", \"command\": \"pytest\", \"output\": $(echo "$command_output" | json_string)}"
        echo "$result" >&2
        cd - > /dev/null
        return 1
      fi
    fi
  fi
  
  # Cargo.tomlがある場合（Rustプロジェクト）
  if [ -f "Cargo.toml" ]; then
    if command -v cargo >/dev/null 2>&1; then
      command_output=$(cargo test 2>&1)
      command_exit_code=$?
      
      if [ $command_exit_code -eq 0 ]; then
        result="{\"success\": true, \"mode\": \"auto\", \"detected\": \"Cargo.toml\", \"command\": \"cargo test\", \"output\": $(echo "$command_output" | json_string)}"
        echo "$result"
        cd - > /dev/null
        return 0
      else
        result="{\"success\": false, \"mode\": \"auto\", \"detected\": \"Cargo.toml\", \"command\": \"cargo test\", \"output\": $(echo "$command_output" | json_string)}"
        echo "$result" >&2
        cd - > /dev/null
        return 1
      fi
    fi
  fi
  
  # 検証方法が見つからない場合
  result="{\"success\": false, \"mode\": \"auto\", \"message\": \"no validation method detected. supported: Makefile, package.json, pyproject.toml, Cargo.toml\"}"
  echo "$result" >&2
  cd - > /dev/null
  return 1
}

# none mode: 検証不能として扱う
function run_validation_none
{
  local project_root="$1"
  
  result="{\"success\": true, \"mode\": \"none\", \"message\": \"validation skipped. changes should be reviewed manually.\"}"
  echo "$result"
  return 0
}

# OpenAI形式のtool定義を返す
function _tool_run_validation_definition_openai
{
  echo '{"type": "function", "function": {"name": "run_validation", "description": "Run validation commands based on the specified mode. configured: read from .aish/validate.json (jq-readable). auto: try to detect and run common validation commands (Makefile, package.json, pyproject.toml, Cargo.toml). none: skip validation and indicate that changes should be reviewed manually.", "parameters": {"type": "object", "properties": {"mode": {"type": "string", "enum": ["auto", "configured", "none"], "description": "Validation mode: configured (read from .aish/validate.json), auto (detect common build systems), none (skip validation)", "default": "auto"}}, "required": []}}}'
}

# Gemini形式のtool定義を返す
function _tool_run_validation_definition_gemini
{
  echo '{"name": "run_validation", "description": "Run validation commands based on the specified mode. configured: read from .aish/validate.json (jq-readable). auto: try to detect and run common validation commands (Makefile, package.json, pyproject.toml, Cargo.toml). none: skip validation and indicate that changes should be reviewed manually.", "parameters": {"type": "object", "properties": {"mode": {"type": "string", "enum": ["auto", "configured", "none"], "description": "Validation mode: configured (read from .aish/validate.json), auto (detect common build systems), none (skip validation)", "default": "auto"}}, "required": []}}'
}

# tool実行処理
# 引数: tool_call_id - tool call ID（OpenAI形式のみ使用）
#      func_args - 関数引数（JSON文字列）
#      provider - "openai" または "gemini"
# 戻り値: tool実行結果（JSON形式）
function _tool_run_validation_execute
{
  local tool_call_id="$1"
  local func_args="$2"
  local provider="$3"
  
  mode=$(echo "$func_args" | jq -r '.mode // "auto"')
  project_root=$(echo "$func_args" | jq -r '.project_root // ""')
  
  if [ -z "$mode" ] || [ "$mode" = "null" ]; then
    mode="auto"
  fi
  
  if [ -z "$project_root" ] || [ "$project_root" = "null" ]; then
    project_root="$(pwd)"
  fi
  
  # 検証を実行
  result=$(run_validation "$mode" "$project_root")
  
  if [ $? -ne 0 ]; then
    return 1
  fi
  
  echo "$result"
}

