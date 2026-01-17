#!/usr/bin/env bash

# セッション管理ライブラリ
# aiコマンドとaishコマンドのセッション管理を提供します

# 元のセッション情報を保存する変数（aiコマンド用）
_ORIGINAL_AISH_SESSION=""
_TEMP_SESSION_CREATED=false

# セッションディレクトリのパスを取得
function session_manager_get_sessions_dir
{
  echo "${AISH_HOME}/sessions"
}

# 新規セッションを作成する
# 戻り値: 作成されたセッションのパス（環境変数としても設定される）
function session_manager_create_new_session
{
  local sessions_dir=$(session_manager_get_sessions_dir)
  mkdir -p "$sessions_dir"
  
  local session_id=$(date +%Y%m%d_%H%M%S)
  local session="$sessions_dir/$session_id"
  mkdir -p "$session"
  ln -sfT "$session_id" "$sessions_dir/latest"
  
  echo "Starting new session: $session_id" >&2
  
  session_manager_setup_session "$session"
}

# 既存のセッションを再開する
# 引数: session_id - 再開するセッションID（省略時は最新）
function session_manager_resume_session
{
  local session_id="$1"
  local sessions_dir=$(session_manager_get_sessions_dir)
  
  if [ -z "$session_id" ]; then
    # 最新のセッションを取得
    session_id=$(ls -1t "$sessions_dir" 2>/dev/null | grep -v "latest" | head -n 1)
    if [ -z "$session_id" ]; then
      echo "No sessions found to resume." >&2
      return 1
    fi
    echo "Resuming latest session: $session_id" >&2
  fi
  
  local session="$sessions_dir/$session_id"
  if [ ! -d "$session" ]; then
    echo "Session not found: $session_id" >&2
    return 1
  fi
  
  session_manager_setup_session "$session"
}

# セッション一覧を表示する
function session_manager_list_sessions
{
  local sessions_dir=$(session_manager_get_sessions_dir)
  ls -1 "$sessions_dir" 2>/dev/null | grep -v "latest" | sort -r
  
  if [ -d "$AISH_SESSION" ]; then
    echo "current: $(basename $AISH_SESSION)" >&2
  fi
}

# セッション環境変数を設定し、必要なディレクトリとFIFOを作成する
# 引数: session - セッションディレクトリのパス
function session_manager_setup_session
{
  local session="$1"
  
  if [ -z "$session" ]; then
    echo "Error: session path is required" >&2
    return 1
  fi
  
  # セッションディレクトリが存在しない場合は作成
  if [ ! -d "$session" ]; then
    mkdir -p "$session"
  fi
  
  # 環境変数を設定
  export AISH_SESSION="$session"
  export AISH_PART="$AISH_SESSION/part"
  export AISH_LOGFILE="$session/script.jsonl"
  
  # partディレクトリを作成
  if [ ! -d "$AISH_PART" ]; then
    mkdir -p "$AISH_PART"
  fi
  
  # FIFOを作成
  [ -p "$AISH_SESSION/fifo" ] || { rm -f "$AISH_SESSION/fifo"; mkfifo "$AISH_SESSION/fifo"; }
}

# ロックファイルをチェックし、必要に応じて一時セッションを作成する（aiコマンド用）
# この関数は ai コマンドの開始時に呼び出される
function session_manager_init
{
  if [ -z "$AISH_SESSION" ]; then
    # AISH_SESSIONが設定されていない場合は何もしない
    return 0
  fi

  _ORIGINAL_AISH_SESSION="$AISH_SESSION"
  local lock_file="$AISH_SESSION/ai.lock"

  if [ -f "$lock_file" ]; then
    # 既存のセッションでai.lockが存在する場合、一時セッションを作成
    local sessions_dir=$(session_manager_get_sessions_dir)
    local original_session_id=$(basename "$AISH_SESSION")
    local child_session_id=$(date +%Y%m%d_%H%M%S)
    local temp_session_id="${original_session_id}_${child_session_id}"
    local temp_session="$sessions_dir/$temp_session_id"

    session_manager_setup_session "$temp_session"
    _TEMP_SESSION_CREATED=true
  else
    # ロックファイルが存在しない場合は通常通りロックファイルを作成
    touch "$lock_file"
    _TEMP_SESSION_CREATED=false
  fi
}

# セッションのクリーンアップ処理（aiコマンド用）
# 元のセッションの場合のみロックファイルを削除する
function session_manager_cleanup
{
  if [ "$_TEMP_SESSION_CREATED" = false ]; then
    # 元のセッションの場合のみログ記録とロックファイル処理を行う
    if [ -n "$_ORIGINAL_AISH_SESSION" ]; then
      # functionsファイルの関数を使用するため、事前に読み込まれている必要がある
      if type detail.aish_flush_script_log >/dev/null 2>&1; then
        detail.aish_flush_script_log
        detail.aish_truncate_script_log
      fi
      rm -f "$_ORIGINAL_AISH_SESSION/ai.lock"
      rm -f "$_ORIGINAL_AISH_SESSION"/masked_* 2>/dev/null
    fi
  else
    # 一時セッションの場合
    if [ "${AISH_DEBUG:-false}" != "true" ]; then
      # デバッグモードでない場合は一時セッションを削除
      rm -rf "$AISH_SESSION"
    fi
  fi
}

# 一時セッションかどうかを判定する
# 戻り値: true または false（文字列として返す）
function session_manager_is_temp_session
{
  echo "$_TEMP_SESSION_CREATED"
}

# 元のセッションのパスを取得する
function session_manager_get_original_session
{
  echo "$_ORIGINAL_AISH_SESSION"
}

