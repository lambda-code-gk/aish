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
  
  # 記憶管理ライブラリを読み込む
  [ -f "$AISH_HOME/lib/memory_manager.sh" ] && . "$AISH_HOME/lib/memory_manager.sh"
  
  # metadata.jsonから全記憶のsubject、keywords、idを取得してコンテキストに追加
  # プロジェクト固有とグローバルの両方を確認
  local project_memory_dir
  project_memory_dir=$(find_memory_directory 2>/dev/null)
  local global_memory_dir="$AISH_HOME/memory"
  
  # 両方のmetadata.jsonから記憶情報を集約
  local memories_info="[]"
  
  # プロジェクト固有のmetadata.jsonから記憶情報を取得
  local project_memories_raw=$(memory_system_load_all "$project_memory_dir")
  if [ "$project_memories_raw" != "[]" ]; then
    local project_memories=$(echo "$project_memories_raw" | jq -c '[.[] | {id: .id, subject: (.subject // ""), keywords: .keywords, category: .category}]' 2>/dev/null || echo "[]")
    if [ ! -z "$project_memories" ] && [ "$project_memories" != "null" ] && [ "$project_memories" != "[]" ]; then
      memories_info="$project_memories"
    fi
  fi
  
  # グローバルのmetadata.jsonから記憶情報を取得（プロジェクト固有と異なる場合のみ）
  if [ "$project_memory_dir" != "$global_memory_dir" ]; then
    local global_memories_raw=$(memory_system_load_all "$global_memory_dir")
    if [ "$global_memories_raw" != "[]" ]; then
      local global_memories=$(echo "$global_memories_raw" | jq -c '[.[] | {id: .id, subject: (.subject // ""), keywords: .keywords, category: .category}]' 2>/dev/null || echo "[]")
      if [ ! -z "$global_memories" ] && [ "$global_memories" != "null" ] && [ "$global_memories" != "[]" ]; then
        # プロジェクト固有の記憶情報とマージ（重複を除外）
        if [ "$memories_info" != "[]" ]; then
          memories_info=$(echo "$memories_info $global_memories" | jq -s 'flatten | group_by(.id) | map(.[0])')
        else
          memories_info="$global_memories"
        fi
      fi
    fi
  fi
  
  # 記憶情報が存在する場合はコンテキストに追加
  if [ "$memories_info" != "[]" ] && [ "$memories_info" != "null" ] && [ ! -z "$memories_info" ]; then
    local memories_text=$(echo "$memories_info" | jq -r '
      "### Available Knowledge in Memory System:\n" +
      "Each entry shows: Subject, Keywords, and ID. Use get_memory_content with the ID to retrieve full details, or use search_memory to search for specific topics.\n\n" +
      ([.[] | 
        (if .subject and .subject != "" then "Subject: " + .subject + " | " else "" end) +
        "Keywords: " + (.keywords | join(", ")) + " | " +
        "ID: " + .id +
        (if .category then " | Category: " + .category else "" end)
      ] | join("\n"))
    ')
    
    if [ ! -z "$memories_text" ] && [ "$memories_text" != "null" ]; then
      # システムインストラクションの前に追加
      if [ -z "$_query_system_instruction" ]; then
        _query_system_instruction="$memories_text"
      else
        _query_system_instruction="$memories_text\n\n$_query_system_instruction"
      fi
    fi
  fi
  
  # 関連する記憶を検索して注入
  if [ ! -z "$_query_args" ]; then
    # 検索を実行
    local memories=$(search_memory_efficient "$_query_args" "" 3 true)
    
    if [ ! -z "$memories" ] && [ "$memories" != "[]" ]; then
        # subject、keywords、id、contentをセットで表示
        local memory_text=$(echo "$memories" | jq -r '
            "### Relevant Knowledge from Past Interactions:\n" +
            "Each entry below shows: Subject, Keywords, ID, and Content.\n\n" +
            ([.[] | 
              (if .subject and .subject != "" then "Subject: " + .subject + "\n" else "" end) +
              "Keywords: " + (.keywords | join(", ")) + "\n" +
              "ID: " + .id + "\n" +
              (if .category then "Category: " + .category + "\n" else "" end) +
              (if .content then "Content: " + .content + "\n" else "" end) +
              "---"
            ] | join("\n"))
        ')
        
        if [ ! -z "$memory_text" ]; then
            # システムインストラクションに追加
            _query_system_instruction="$_query_system_instruction\n\n$memory_text"
        fi
    fi
  fi
  
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
