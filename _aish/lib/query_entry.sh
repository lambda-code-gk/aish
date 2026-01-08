#!/usr/bin/env bash

# query関数の共通エントリーポイント
# オプション解析、ファイル準備を行う
# 結果は以下のグローバル変数に設定される:
#   _query_system_instruction: システムインストラクション
#   _query_agent_mode: エージェントモード（true/false）
#   _query_files: 準備されたファイルリスト
#   _query_args: 残りの引数
function query_entry_prepare
{
  _query_system_instruction=""
  _query_agent_mode=false
  
  # OPTINDをリセット（再呼び出しに対応）
  OPTIND=1
  
  while getopts "s:a" opt; do
    case $opt in
      s) _query_system_instruction=$OPTARG ;;
      a) _query_agent_mode=true ;;
      *)  ;;
    esac
  done
  shift $((OPTIND - 1))
  
  # 残りの引数を保存
  _query_args="$*"
  
  aish_rollout
  
  _query_files=$(detail.aish_list_parts | detail.aish_security_check)
  if [ $? -ne 0 ]; then
    exit 1
  fi
}

# レスポンステキストをファイルに保存して標準出力にも出力
# 引数: text - 保存・出力するテキスト
function save_response_text
{
  local text="$1"
  echo "$text" | tee "$AISH_PART/part_$(date +%Y%m%d_%H%M%S)_assistant.txt"
}

