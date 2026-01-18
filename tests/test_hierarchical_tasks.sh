#!/bin/bash
# 階層化タスクの動作確認テストスクリプト

set -euo pipefail

# プロジェクトルートに移動
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

# 色付き出力のための変数
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# テスト用のディレクトリ
TEST_DIR=$(mktemp -d)
trap "rm -rf $TEST_DIR" EXIT SIGINT SIGTERM SIGHUP

# aiコマンドのパス
AI_CMD="${PROJECT_ROOT}/ai"

# AISH_HOMEの設定（テスト用）
export AISH_HOME="${TEST_DIR}/.aish"
mkdir -p "$AISH_HOME"

# AISH_SESSIONを解除して再帰呼び出しエラーを回避
unset AISH_SESSION

# テスト用のfunctionsファイルを作成
mkdir -p "$AISH_HOME/lib"
cp "${PROJECT_ROOT}/_aish/lib/error_handler.sh" "$AISH_HOME/lib/error_handler.sh"
cp "${PROJECT_ROOT}/_aish/lib/logger.sh" "$AISH_HOME/lib/logger.sh"
cp "${PROJECT_ROOT}/_aish/lib/session_manager.sh" "$AISH_HOME/lib/session_manager.sh"

cat > "$AISH_HOME/functions" << 'EOF'
#!/bin/bash
function detail.aish_flush_script_log { :; }
function detail.aish_truncate_script_log { :; }
function detail.aish_calc_message_size { echo 0; }
EOF

# テスト用のタスクディレクトリとファイルを作成
setup_test_tasks() {
    # defaultタスク
    mkdir -p "$AISH_HOME/task.d/default"
    cat > "$AISH_HOME/task.d/default/conf" << 'EOF'
description="Default task"
EOF
    cat > "$AISH_HOME/task.d/default/execute" << 'EOF'
echo "default task executed"
EOF

    # 階層化タスク
    mkdir -p "$AISH_HOME/task.d/group1"
    cat > "$AISH_HOME/task.d/group1/task1.sh" << 'EOF'
# Description: Task 1 in Group 1
echo "group1/task1 executed"
EOF

    mkdir -p "$AISH_HOME/task.d/group2/subgroup"
    cat > "$AISH_HOME/task.d/group2/subgroup/task2.sh" << 'EOF'
# Description: Task 2 in Group 2 Subgroup
echo "group2/subgroup/task2 executed"
EOF

    # 脱出試行用のファイル
    echo "secret data" > "$TEST_DIR/secret.txt"
}

# ヘルパー関数
log_info() { echo -e "${GREEN}[INFO]${NC} $*"; }
log_error() { echo -e "${RED}[ERROR]${NC} $*"; }

assert_contains() {
    local output="$1"
    local pattern="$2"
    if echo "$output" | grep -q "$pattern"; then
        return 0
    else
        log_error "Output does not contain: $pattern"
        log_error "Actual output: $output"
        return 1
    fi
}

test_hierarchical_execution() {
    log_info "Testing hierarchical task execution..."
    setup_test_tasks
    
    local output
    output=$($AI_CMD group1/task1 2>&1)
    assert_contains "$output" "group1/task1 executed"
    
    output=$($AI_CMD group2/subgroup/task2 2>&1)
    assert_contains "$output" "group2/subgroup/task2 executed"
}

test_directory_traversal_prevention() {
    log_info "Testing directory traversal prevention..."
    setup_test_tasks
    
    local output
    output=$($AI_CMD "../../../secret.txt" 2>&1 || true)
    # Try to access a file outside task.d
    # If it works, it might try to '.' the file.
    # We want it to fail safely or reject it.
    if echo "$output" | grep -qi "error"; then
        log_info "✓ Traversal rejected"
    else
        log_error "✗ Traversal NOT rejected"
        log_error "Actual output: $output"
        return 1
    fi
}

test_completion() {
    log_info "Testing bash completion logic..."

    setup_test_tasks

    # Create a wrapper to test completion
    cat > "$TEST_DIR/test_comp.sh" << EOF
export AISH_HOME="$AISH_HOME"
. "$PROJECT_ROOT/_aish/aishrc"
COMP_WORDS=(ai gro)
COMP_CWORD=1
_ai_subcommand_completions
echo "\${COMPREPLY[@]}"
EOF

    local output
    output=$(bash "$TEST_DIR/test_comp.sh")

    # completion should return task names, not directories
    assert_contains "$output" "group1/task1"
    assert_contains "$output" "group2/subgroup/task2"
}

test_help_display() {
    log_info "Testing help display for hierarchical tasks..."
    setup_test_tasks
    
    local output
    output=$($AI_CMD -h 2>&1)
    assert_contains "$output" "group1/task1"
    assert_contains "$output" "group2/subgroup/task2"
}

# Main
setup_test_tasks
test_hierarchical_execution || exit 1
test_directory_traversal_prevention || exit 1
test_completion || exit 1
test_help_display || exit 1

log_info "All tests passed!"