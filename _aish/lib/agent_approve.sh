#!/usr/bin/env bash

# 確認不要なコマンドのリストをファイルから読み込む
# 設定ファイル: $AISH_HOME/command_rules
function get_approved_commands_list
{
  local config_file="$AISH_HOME/command_rules"
  
  # ファイルが存在する場合のみ読み込む
  # regex:で始まる行は正規表現パターン（コメントではない）
  if [ -f "$config_file" ]; then
    cat "$config_file" | awk '
      /^regex:/ { print; next }   # 正規表現パターンはそのまま出力
      /^#/ { next }                # コメント行は除外
      /^$/ { next }                # 空行は除外
      { print }                    # その他の行は出力
    '
  fi
}

# パターンを分類し、タイプを判定する
# 戻り値: "exact", "wildcard", "regex", "deny_exact", "deny_wildcard", "deny_regex"
# 出力: パターンタイプ（stdout）
function _classify_pattern_type
{
  local pattern="$1"
  
  # 空のパターンは除外
  if [ -z "$pattern" ]; then
    return 1
  fi
  
  # 拒否パターンの判定（行頭に!または-がある）
  local is_deny=false
  if [[ "$pattern" =~ ^[!-] ]]; then
    is_deny=true
    pattern="${pattern:1}"  # プレフィックスを除去
  fi
  
  # 正規表現パターンの判定
  if [[ "$pattern" =~ ^regex: ]]; then
    pattern="${pattern#regex:}"  # regex:プレフィックスを除去
    if [ "$is_deny" = true ]; then
      echo "deny_regex"
    else
      echo "regex"
    fi
    return 0
  fi
  
  # ワイルドカードパターンの判定（*または?を含む）
  if [[ "$pattern" == *"*"* ]] || [[ "$pattern" == *"?"* ]]; then
    if [ "$is_deny" = true ]; then
      echo "deny_wildcard"
    else
      echo "wildcard"
    fi
    return 0
  fi
  
  # 完全一致パターン
  if [ "$is_deny" = true ]; then
    echo "deny_exact"
  else
    echo "exact"
  fi
  return 0
}

# パターンとコマンドのマッチングを判定する
# $1: コマンド文字列
# $2: パターン
# $3: パターンタイプ（"exact", "wildcard", "regex", "deny_exact", "deny_wildcard", "deny_regex"）
# 戻り値: 0=マッチ, 1=マッチしない
function _is_pattern_match
{
  local command="$1"
  local pattern="$2"
  local pattern_type="$3"
  
  case "$pattern_type" in
    exact)
      if [ "$command" = "$pattern" ]; then
        return 0
      fi
      ;;
    deny_exact)
      # 拒否パターンの完全一致: 完全一致またはコマンドがパターンで始まる場合も拒否
      if [ "$command" = "$pattern" ] || [[ "$command" == "$pattern"* ]]; then
        return 0
      fi
      ;;
    wildcard|deny_wildcard)
      # bashのパターンマッチングを使用
      if [[ "$command" == $pattern ]]; then
        return 0
      fi
      # ワイルドカードパターンが "cmd *" の形式の場合、コマンド名のみでもマッチ
      # 例: "git *" は "git" にもマッチ
      if [[ "$pattern" == *" *" ]]; then
        # スペースと*の前の部分を抽出
        local prefix="${pattern% *}"
        if [ "$command" = "$prefix" ]; then
          return 0
        fi
      fi
      ;;
    regex|deny_regex)
      # Python3のreモジュールを使用して正規表現マッチング
      if python3 -c "
import sys
import re
command = sys.argv[1]
pattern = sys.argv[2]
try:
    if re.match(pattern, command):
        sys.exit(0)
    else:
        sys.exit(1)
except Exception:
    sys.exit(1)
" "$command" "$pattern" 2>/dev/null; then
        return 0
      fi
      ;;
  esac
  
  return 1
}

# 拒否パターンにマッチするかチェック（コマンド全体をチェック）
# $1: コマンド文字列（引数含む）
# 戻り値: 0=拒否される（マッチ）, 1=拒否されない（マッチしない）
function is_command_denied
{
  local command="$1"
  local config_file="$AISH_HOME/command_rules"
  
  if [ ! -f "$config_file" ]; then
    return 1  # ファイルが存在しない場合は拒否されない
  fi
  
  # 拒否パターン（行頭に!または-がある行）を取得
  # regex:で始まる行は正規表現パターン（コメントではない）
  local deny_patterns=$(cat "$config_file" | awk '
    /^regex:/ { print; next }   # 正規表現パターンはそのまま出力
    /^#/ { next }                # コメント行は除外
    /^$/ { next }                # 空行は除外
    { print }                    # その他の行は出力
  ' | grep -E '^[!-]')
  
  while IFS= read -r pattern_line; do
    if [ -z "$pattern_line" ]; then
      continue
    fi
    
    # パターンタイプを判定
    local pattern_type=$(_classify_pattern_type "$pattern_line")
    
    # 拒否パターンのみを処理
    case "$pattern_type" in
      deny_exact|deny_wildcard|deny_regex)
        # プレフィックスを除去してパターンを取得
        local pattern="$pattern_line"
        if [[ "$pattern" =~ ^[!-] ]]; then
          pattern="${pattern:1}"
        fi
        if [[ "$pattern" =~ ^regex: ]]; then
          pattern="${pattern#regex:}"
        fi
        
        # マッチングをチェック
        if _is_pattern_match "$command" "$pattern" "$pattern_type"; then
          return 0  # 拒否される
        fi
        ;;
    esac
  done <<< "$deny_patterns"
  
  return 1  # 拒否されない
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

# 許可パターンにマッチするかチェック（コマンド名をチェック）
# $1: コマンド名（例: "git status"）
# $2: 設定ファイルの内容（パターンリスト）
# $3: コマンド全体（オプション、正規表現チェック用）
# 戻り値: 0=承認される, 1=承認されない
function _is_command_name_approved
{
  local cmd_name="$1"
  local patterns="$2"
  local full_command="${3:-$cmd_name}"
  
  # パターンを分類: 完全一致、ワイルドカード、正規表現に分ける
  local exact_patterns=""
  local wildcard_patterns=""
  local regex_patterns=""
  
  while IFS= read -r pattern_line; do
    if [ -z "$pattern_line" ]; then
      continue
    fi
    
    # 拒否パターンはスキップ（許可パターンのみを処理）
    if [[ "$pattern_line" =~ ^[!-] ]]; then
      continue
    fi
    
    local pattern_type=$(_classify_pattern_type "$pattern_line")
    
    case "$pattern_type" in
      exact)
        if [ -z "$exact_patterns" ]; then
          exact_patterns="$pattern_line"
        else
          exact_patterns="$exact_patterns"$'\n'"$pattern_line"
        fi
        ;;
      wildcard)
        if [ -z "$wildcard_patterns" ]; then
          wildcard_patterns="$pattern_line"
        else
          wildcard_patterns="$wildcard_patterns"$'\n'"$pattern_line"
        fi
        ;;
      regex)
        # regex:プレフィックスを除去
        local pattern="$pattern_line"
        if [[ "$pattern" =~ ^regex: ]]; then
          pattern="${pattern#regex:}"
        fi
        if [ -z "$regex_patterns" ]; then
          regex_patterns="$pattern"
        else
          regex_patterns="$regex_patterns"$'\n'"$pattern"
        fi
        ;;
    esac
  done <<< "$patterns"
  
  # 優先順位に従ってチェック: 完全一致 → ワイルドカード → 正規表現
  
  # 1. 完全一致チェック（最優先）
  while IFS= read -r pattern; do
    if [ -z "$pattern" ]; then
      continue
    fi
    if _is_pattern_match "$cmd_name" "$pattern" "exact"; then
      return 0  # 承認される
    fi
  done <<< "$exact_patterns"
  
  # 2. ワイルドカードチェック
  while IFS= read -r pattern; do
    if [ -z "$pattern" ]; then
      continue
    fi
    if _is_pattern_match "$cmd_name" "$pattern" "wildcard"; then
      return 0  # 承認される
    fi
  done <<< "$wildcard_patterns"
  
  # 3. 正規表現チェック
  while IFS= read -r pattern; do
    if [ -z "$pattern" ]; then
      continue
    fi
    # 正規表現パターンの場合は、コマンド名とコマンド全体の両方をチェック
    if _is_pattern_match "$cmd_name" "$pattern" "regex" || _is_pattern_match "$full_command" "$pattern" "regex"; then
      return 0  # 承認される
    fi
  done <<< "$regex_patterns"
  
  return 1  # 承認されない
}

# コマンドが確認不要かチェック
function is_command_approved
{
  local command="$1"
  local config_file="$AISH_HOME/command_rules"
  
  # 1. 拒否パターンチェック（最優先）- コマンド全体をチェック
  if is_command_denied "$command"; then
    return 1  # 拒否される
  fi
  
  # 2. 許可パターンチェック - コマンド名をチェック
  local approved_list=""
  if [ -f "$config_file" ]; then
    approved_list=$(cat "$config_file" | awk '
      /^regex:/ { print; next }   # 正規表現パターンはそのまま出力
      /^#/ { next }                # コメント行は除外
      /^$/ { next }                # 空行は除外
      { print }                    # その他の行は出力
    ')
  fi
  
  # コマンド文字列から各コマンドを抽出
  local commands=$(extract_commands "$command")
  
  # すべてのコマンドが承認リストに含まれているかチェック
  local all_approved=true
  while IFS= read -r cmd_name; do
    if [ -z "$cmd_name" ]; then
      continue
    fi
    
    # コマンド名が承認リストに含まれているかチェック（後方互換性のため完全一致も試す）
    local approved=false
    
    # 新しいパターンマッチングでチェック（コマンド全体も渡す）
    if _is_command_name_approved "$cmd_name" "$approved_list" "$command"; then
      approved=true
    # 後方互換性: 完全一致チェック（既存の動作を維持）
    elif echo "$approved_list" | grep -v '^[!-]' | grep -v '^regex:' | grep -Fxq "$cmd_name" 2>/dev/null; then
      approved=true
    fi
    
    if [ "$approved" = false ]; then
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

# 危険性レベルの文字列化
# $1: 危険性レベル（0=安全, 1=critical, 2=high, 3=medium）
# 出力: 危険性レベルの文字列（stdout）
function _get_danger_level_string
{
  case "$1" in
    1) echo "critical" ;;
    2) echo "high" ;;
    3) echo "medium" ;;
    *) echo "safe" ;;
  esac
}

# 危険なコマンド・引数を検出する
# $1: コマンド文字列（引数含む）
# 戻り値: 0=安全, 1=critical, 2=high, 3=medium
# 出力: 検出されたパターン名（stdout、複数の場合は改行区切り）
function check_command_danger
{
  local command="$1"
  local detected_patterns=""
  local max_level=0
  
  # パターン1: ルートディレクトリの削除（critical）
  if [[ "$command" =~ rm\ -rf\ (/[[:space:]]|/\*|/\ |/\.\.|/etc|/usr|/var|/bin|/sbin|/boot|/lib|/lib64) ]] || [[ "$command" =~ rm\ -rf\ /$ ]]; then
    if [ -z "$detected_patterns" ]; then
      detected_patterns="rm_rf_root"
    else
      detected_patterns="$detected_patterns"$'\n'"rm_rf_root"
    fi
    max_level=1
  fi
  
  # パターン2: カレントディレクトリの全削除（critical）
  if [[ "$command" =~ rm\ -rf\ \* ]]; then
    if [ -z "$detected_patterns" ]; then
      detected_patterns="rm_rf_all"
    else
      detected_patterns="$detected_patterns"$'\n'"rm_rf_all"
    fi
    if [ $max_level -lt 1 ]; then
      max_level=1
    fi
  fi
  
  # パターン3: 危険なddコマンド（critical）
  if [[ "$command" =~ dd\ .*of=(/dev/|/etc|/usr|/var|/) ]]; then
    if [ -z "$detected_patterns" ]; then
      detected_patterns="dd_disk_write"
    else
      detected_patterns="$detected_patterns"$'\n'"dd_disk_write"
    fi
    if [ $max_level -lt 1 ]; then
      max_level=1
    fi
  fi
  
  # パターン4: ファイルシステム操作（critical）
  if [[ "$command" =~ mkfs\.[[:alnum:]]+\ ( /dev/|/etc|/usr|/var|/) ]]; then
    if [ -z "$detected_patterns" ]; then
      detected_patterns="mkfs_dangerous"
    else
      detected_patterns="$detected_patterns"$'\n'"mkfs_dangerous"
    fi
    if [ $max_level -lt 1 ]; then
      max_level=1
    fi
  fi
  
  # パターン5: 権限変更（全権限、ルートディレクトリ）（high）
  if [[ "$command" =~ chmod\ 777\ (/\ |/ |/) ]] || [[ "$command" =~ chmod\ 777\ /$ ]]; then
    if [ -z "$detected_patterns" ]; then
      detected_patterns="chmod_777_root"
    else
      detected_patterns="$detected_patterns"$'\n'"chmod_777_root"
    fi
    if [ $max_level -lt 2 ]; then
      max_level=2
    fi
  fi
  
  # パターン6: sudo使用（high）- 特に危険なコマンドと組み合わせた場合
  if [[ "$command" =~ ^sudo\ .*(rm\ -rf|chmod\ 777|chown.*root|dd|mkfs) ]]; then
    if [ -z "$detected_patterns" ]; then
      detected_patterns="sudo_dangerous"
    else
      detected_patterns="$detected_patterns"$'\n'"sudo_dangerous"
    fi
    if [ $max_level -lt 2 ]; then
      max_level=2
    fi
  fi
  
  # パターン7: PATH環境変数の上書き（high）
  if [[ "$command" =~ export\ PATH= ]]; then
    if [ -z "$detected_patterns" ]; then
      detected_patterns="export_path_overwrite"
    else
      detected_patterns="$detected_patterns"$'\n'"export_path_overwrite"
    fi
    if [ $max_level -lt 2 ]; then
      max_level=2
    fi
  fi
  
  # パターン8: LD_LIBRARY_PATH環境変数の上書き（medium）
  if [[ "$command" =~ export\ LD_LIBRARY_PATH= ]]; then
    if [ -z "$detected_patterns" ]; then
      detected_patterns="export_ld_library_path"
    else
      detected_patterns="$detected_patterns"$'\n'"export_ld_library_path"
    fi
    if [ $max_level -lt 3 ]; then
      max_level=3
    fi
  fi
  
  # 検出されたパターンを出力（後続の処理で使用）
  if [ -n "$detected_patterns" ]; then
    echo "$detected_patterns"
  fi
  
  return $max_level
}

# 危険性警告メッセージを生成
# $1: 危険性レベル（1=critical, 2=high, 3=medium）
# $2: 検出されたパターン名（改行区切り）
# $3: コマンド文字列
# 出力: 警告メッセージ（stdout）
function _get_danger_warning_message
{
  local level="$1"
  local patterns="$2"
  local command="$3"
  local level_str=$(_get_danger_level_string "$level")
  
  # レベルに応じたアイコンと色を決定
  case "$level" in
    1)
      echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
      echo "🚨 CRITICAL SECURITY WARNING"
      echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
      ;;
    2)
      echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
      echo "⚠️  HIGH SECURITY WARNING"
      echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
      ;;
    3)
      echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
      echo "⚠️  SECURITY WARNING"
      echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
      ;;
  esac
  
  echo "Command: $command"
  echo "Risk Level: $(echo "$level_str" | tr '[:lower:]' '[:upper:]')"
  
  # 検出されたパターンの説明を追加
  local first_pattern=true
  while IFS= read -r pattern; do
    if [ -z "$pattern" ]; then
      continue
    fi
    if [ "$first_pattern" = true ]; then
      echo -n "Reason: "
      first_pattern=false
    else
      echo -n "         "
    fi
    case "$pattern" in
      rm_rf_root)
        echo "Attempting to delete root directory or critical system directories"
        ;;
      rm_rf_all)
        echo "Attempting to delete all files in current directory"
        ;;
      dd_disk_write)
        echo "Attempting to write to disk device directly"
        ;;
      mkfs_dangerous)
        echo "Attempting to format filesystem"
        ;;
      chmod_777_root)
        echo "Attempting to set world-writable permissions on root directory"
        ;;
      sudo_dangerous)
        echo "Using sudo with dangerous command combination"
        ;;
      export_path_overwrite)
        echo "Attempting to overwrite PATH environment variable"
        ;;
      export_ld_library_path)
        echo "Attempting to modify LD_LIBRARY_PATH environment variable"
        ;;
      *)
        echo "Potentially dangerous operation detected"
        ;;
    esac
  done <<< "$patterns"
  
  echo ""
  if [ "$level" -eq 1 ]; then
    echo "This command is extremely dangerous and can cause"
    echo "irreversible data loss. Are you absolutely sure?"
  elif [ "$level" -eq 2 ]; then
    echo "This command may cause system instability or security issues."
    echo "Please verify that this is the intended operation."
  else
    echo "This command may have unexpected side effects."
    echo "Please review before proceeding."
  fi
  echo ""
}

