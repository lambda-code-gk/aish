#!/usr/bin/env bash

# 確認不要なコマンドのリストをファイルから読み込む
# 設定ファイル: $AISH_HOME/agent_approved_commands
function get_approved_commands_list
{
  local config_file="$AISH_HOME/agent_approved_commands"
  
  # ファイルが存在する場合のみ読み込む
  if [ -f "$config_file" ]; then
    cat "$config_file" | grep -v '^#' | grep -v '^$'
  fi
}

# コマンド文字列から各コマンドを抽出（パイプ、セミコロン、&&、||で分割）
# 引用符で囲まれた部分は保護する
function extract_commands
{
  local cmd="$1"
  local result=""
  
  # Pythonを使って引用符を考慮したパースを行う
  python3 -c "
import sys
import re
import shlex

cmd = sys.argv[1]

# 引用符で囲まれた部分を保護しながら、パイプ、セミコロン、&&、||で分割
# シンプルなアプローチ: 引用符外のパイプ、セミコロン、&&、||で分割
parts = []
in_quote = False
quote_char = None
current = ''
i = 0

while i < len(cmd):
    char = cmd[i]
    
    if char in ['\"', \"'\"] and (i == 0 or cmd[i-1] != '\\\\'):
        if not in_quote:
            in_quote = True
            quote_char = char
        elif char == quote_char:
            in_quote = False
            quote_char = None
        current += char
    elif not in_quote and char == '|' and (i == 0 or cmd[i-1] != '|') and (i == len(cmd)-1 or cmd[i+1] != '|'):
        # パイプ（||は除く）
        if current.strip():
            parts.append(current.strip())
        current = ''
    elif not in_quote and char == ';':
        # セミコロン
        if current.strip():
            parts.append(current.strip())
        current = ''
    elif not in_quote and i < len(cmd) - 1 and cmd[i:i+2] == '&&':
        # &&
        if current.strip():
            parts.append(current.strip())
        current = ''
        i += 1
    elif not in_quote and i < len(cmd) - 1 and cmd[i:i+2] == '||':
        # ||
        if current.strip():
            parts.append(current.strip())
        current = ''
        i += 1
    else:
        current += char
    i += 1

if current.strip():
    parts.append(current.strip())

# 各パートからコマンド名を抽出
for part in parts:
    # リダイレクト記号を除去
    part = re.sub(r'\\s*\\d*[<>]&?\\s*\\S*', '', part)
    # 最初の単語を抽出
    words = part.strip().split()
    if words:
        print(words[0])
" "$cmd" | sort -u
}

# コマンドが確認不要かチェック
function is_command_approved
{
  local command="$1"
  local approved_list=$(get_approved_commands_list)
  
  # コマンド文字列から各コマンドを抽出
  local commands=$(extract_commands "$command")
  
  # すべてのコマンドが承認リストに含まれているかチェック
  local all_approved=true
  while IFS= read -r cmd_name; do
    if [ -z "$cmd_name" ]; then
      continue
    fi
    # コマンド名が承認リストに含まれているかチェック
    if ! echo "$approved_list" | grep -Fxq "$cmd_name" 2>/dev/null; then
      all_approved=false
      break
    fi
  done <<< "$commands"
  
  if [ "$all_approved" = true ]; then
    return 0
  else
    return 1
  fi
}

