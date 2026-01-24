#!/bin/bash
# コマンド承認システムのテストスクリプト

set -euo pipefail

# 色付き出力のための変数
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# プロジェクトルートの取得
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# AISH_HOMEの設定
export AISH_HOME="$PROJECT_ROOT/_aish"

# テスト結果のカウント
TESTS_PASSED=0
TESTS_FAILED=0

# ヘルパー関数
log_info() {
    echo -e "${GREEN}[INFO]${NC} $*"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $*"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $*"
}

# agent_approve.shを読み込む
if [ ! -f "$AISH_HOME/lib/agent_approve.sh" ]; then
    log_error "agent_approve.sh not found: $AISH_HOME/lib/agent_approve.sh"
    exit 1
fi

. "$AISH_HOME/lib/agent_approve.sh"

# テスト用の設定ファイル
TEST_CONFIG_DIR=$(mktemp -d)
TEST_CONFIG="$TEST_CONFIG_DIR/command_rules"
ORIG_CONFIG="$AISH_HOME/command_rules"
ORIG_CONFIG_BACKUP=""

# テスト開始時に一度だけバックアップを作成
backup_original_config() {
    if [ -f "$ORIG_CONFIG" ]; then
        ORIG_CONFIG_BACKUP=$(mktemp)
        cp "$ORIG_CONFIG" "$ORIG_CONFIG_BACKUP"
        log_info "Backed up original config to: $ORIG_CONFIG_BACKUP"
    else
        log_warn "Original config file not found: $ORIG_CONFIG"
    fi
}

# 元の設定ファイルを復元
restore_original_config() {
    if [ -n "$ORIG_CONFIG_BACKUP" ] && [ -f "$ORIG_CONFIG_BACKUP" ]; then
        cp "$ORIG_CONFIG_BACKUP" "$ORIG_CONFIG" 2>/dev/null || true
        log_info "Restored original config"
    elif [ ! -f "$ORIG_CONFIG_BACKUP" ] && [ ! -f "$ORIG_CONFIG" ]; then
        # 元々ファイルが存在しなかった場合は削除
        rm -f "$ORIG_CONFIG" 2>/dev/null || true
        log_info "Removed config file (did not exist originally)"
    fi
}

# クリーンアップ関数
cleanup() {
    restore_original_config
    rm -rf "$TEST_CONFIG_DIR" 2>/dev/null || true
    rm -f "$ORIG_CONFIG_BACKUP" 2>/dev/null || true
}

trap cleanup EXIT

# テストケース実行関数
run_test_case() {
    local test_name="$1"
    local test_func="$2"
    
    echo ""
    echo "========================================="
    echo "Test: $test_name"
    echo "========================================="
    
    # テスト実行
    local test_result=0
    if "$test_func"; then
        log_info "✓ $test_name PASSED"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        log_error "✗ $test_name FAILED"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        test_result=1
    fi
    
    # テスト後に設定ファイルを復元
    teardown_test_config
    
    return $test_result
}

# 設定ファイルを一時的に置き換える
setup_test_config() {
    local config_content="$1"
    echo "$config_content" > "$TEST_CONFIG"
    
    # テスト用の設定ファイルを実際の場所にコピー
    cp "$TEST_CONFIG" "$ORIG_CONFIG"
}

# テスト後に設定ファイルを復元
teardown_test_config() {
    restore_original_config
}

# ============================================
# テストケース
# ============================================

# テスト1: パターン分類
test_pattern_classification() {
    local result
    
    result=$(_classify_pattern_type "ls")
    [ "$result" = "exact" ] || return 1
    
    result=$(_classify_pattern_type "git *")
    [ "$result" = "wildcard" ] || return 1
    
    result=$(_classify_pattern_type "regex:^git")
    [ "$result" = "regex" ] || return 1
    
    result=$(_classify_pattern_type "!sed -i")
    [ "$result" = "deny_exact" ] || return 1
    
    result=$(_classify_pattern_type "-sudo *")
    [ "$result" = "deny_wildcard" ] || return 1
    
    result=$(_classify_pattern_type "!regex:^rm")
    [ "$result" = "deny_regex" ] || return 1
    
    return 0
}

# テスト2: 完全一致マッチング
test_exact_match() {
    setup_test_config "ls
cat
grep"
    
    is_command_approved "ls" || return 1
    is_command_approved "cat" || return 1
    is_command_approved "grep" || return 1
    is_command_approved "find" && return 1  # 許可されていないので承認されないはず
    return 0
}

# テスト3: ワイルドカードマッチング
test_wildcard_match() {
    setup_test_config "git *
docker *"
    
    is_command_approved "git status" || return 1
    is_command_approved "git log" || return 1
    is_command_approved "git diff" || return 1
    is_command_approved "docker ps" || return 1
    is_command_approved "docker images" || return 1
    is_command_approved "cat" && return 1  # catは許可されていない
    return 0
}

# テスト4: 正規表現マッチング
test_regex_match() {
    setup_test_config "regex:^docker (ps|images)( .*)?$"
    
    is_command_approved "docker ps" || return 1
    is_command_approved "docker images" || return 1
    is_command_approved "docker ps -a" || return 1
    is_command_approved "docker run" && return 1  # マッチしないはず
    return 0
}

# テスト5: 拒否パターン（完全一致）
test_deny_exact() {
    setup_test_config "sed
!sed -i"
    
    is_command_approved "sed s/a/b/ file.txt" || return 1
    is_command_approved "sed -i s/a/b/ file.txt" && return 1  # 拒否されるはず
    return 0
}

# テスト6: 拒否パターン（ワイルドカード）
test_deny_wildcard() {
    setup_test_config "git *
-sudo *"
    
    is_command_approved "git status" || return 1
    is_command_approved "sudo ls" && return 1  # 拒否されるはず
    is_command_approved "sudo apt update" && return 1  # 拒否されるはず
    return 0
}

# テスト7: 拒否パターンの優先順位
test_deny_priority() {
    setup_test_config "git *
!git push
!git commit"
    
    is_command_approved "git status" || return 1
    is_command_approved "git log" || return 1
    is_command_approved "git push" && return 1  # 拒否されるはず
    is_command_approved "git commit -m test" && return 1  # 拒否されるはず
    return 0
}

# テスト8: 危険性検出（critical）
test_danger_detection_critical() {
    local detected level
    
    detected=$(check_command_danger "rm -rf /")
    level=$?
    [ $level -eq 1 ] || return 1  # critical = 1
    
    detected=$(check_command_danger "rm -rf /*")
    level=$?
    [ $level -eq 1 ] || return 1
    
    detected=$(check_command_danger "dd if=/dev/zero of=/dev/sda")
    level=$?
    [ $level -eq 1 ] || return 1
    
    return 0
}

# テスト9: 危険性検出（high）
test_danger_detection_high() {
    local detected level
    
    detected=$(check_command_danger "sudo rm -rf *")
    level=$?
    [ $level -eq 2 ] || return 1  # high = 2
    
    detected=$(check_command_danger "chmod 777 /")
    level=$?
    [ $level -eq 2 ] || return 1
    
    detected=$(check_command_danger "export PATH=")
    level=$?
    [ $level -eq 2 ] || return 1
    
    return 0
}

# テスト10: 危険性検出（medium）
test_danger_detection_medium() {
    local detected level
    
    detected=$(check_command_danger "export LD_LIBRARY_PATH=")
    level=$?
    [ $level -eq 3 ] || return 1  # medium = 3
    
    return 0
}

# テスト11: 安全なコマンド（危険性検出されない）
test_safe_commands() {
    local detected level
    
    detected=$(check_command_danger "ls")
    level=$?
    [ $level -eq 0 ] || return 1  # safe = 0
    
    detected=$(check_command_danger "git status")
    level=$?
    [ $level -eq 0 ] || return 1
    
    detected=$(check_command_danger "cat file.txt")
    level=$?
    [ $level -eq 0 ] || return 1
    
    detected=$(check_command_danger "docker ps")
    level=$?
    [ $level -eq 0 ] || return 1
    
    return 0
}

# テスト12: 統合テスト（承認 + 危険性検出）
test_integration() {
    setup_test_config "git *
!git push
!git commit
-sudo *"
    
    # 安全で承認されたコマンド
    is_command_approved "git status" || return 1
    is_command_approved "git log" || return 1
    
    # 危険で承認されていないコマンド
    local detected level
    detected=$(check_command_danger "rm -rf /")
    level=$?
    [ $level -gt 0 ] || return 1  # 危険性が検出されるはず
    
    is_command_approved "rm -rf /" && return 1  # 承認されないはず
    
    # 拒否パターンで承認されないコマンド
    is_command_approved "git push" && return 1
    is_command_approved "git commit -m test" && return 1
    is_command_approved "sudo ls" && return 1
    
    return 0
}

# テスト13: extract_commands関数
test_extract_commands() {
    local commands result
    
    commands=$(extract_commands "ls")
    echo "$commands" | grep -q "^ls$" || return 1
    
    commands=$(extract_commands "git status")
    echo "$commands" | grep -q "^git$" || return 1
    
    commands=$(extract_commands "ls | grep test")
    echo "$commands" | grep -q "^ls$" || return 1
    echo "$commands" | grep -q "^grep$" || return 1
    
    # 複数コマンドのテスト
    commands=$(extract_commands "cat file.txt && grep pattern file.txt")
    echo "$commands" | grep -q "^cat$" || return 1
    echo "$commands" | grep -q "^grep$" || return 1
    
    return 0
}

# テスト14: 後方互換性（既存の完全一致形式）
test_backward_compatibility() {
    setup_test_config "ls
cat
grep"
    
    is_command_approved "ls" || return 1
    is_command_approved "cat" || return 1
    is_command_approved "grep" || return 1
    is_command_approved "find" && return 1  # 許可されていない
    
    return 0
}

# テスト15: 危険性レベルの文字列化
test_danger_level_string() {
    local result
    
    result=$(_get_danger_level_string 0)
    [ "$result" = "safe" ] || return 1
    
    result=$(_get_danger_level_string 1)
    [ "$result" = "critical" ] || return 1
    
    result=$(_get_danger_level_string 2)
    [ "$result" = "high" ] || return 1
    
    result=$(_get_danger_level_string 3)
    [ "$result" = "medium" ] || return 1
    
    return 0
}

# ============================================
# メイン実行
# ============================================

main() {
    echo "========================================="
    echo "Command Approval System Tests"
    echo "========================================="
    echo "AISH_HOME: $AISH_HOME"
    echo ""
    
    # テスト開始時に元の設定ファイルをバックアップ
    backup_original_config
    
    # テスト実行
    run_test_case "Pattern Classification" test_pattern_classification
    run_test_case "Exact Match" test_exact_match
    run_test_case "Wildcard Match" test_wildcard_match
    run_test_case "Regex Match" test_regex_match
    run_test_case "Deny Pattern (Exact)" test_deny_exact
    run_test_case "Deny Pattern (Wildcard)" test_deny_wildcard
    run_test_case "Deny Priority" test_deny_priority
    run_test_case "Danger Detection (Critical)" test_danger_detection_critical
    run_test_case "Danger Detection (High)" test_danger_detection_high
    run_test_case "Danger Detection (Medium)" test_danger_detection_medium
    run_test_case "Safe Commands" test_safe_commands
    run_test_case "Integration Test" test_integration
    run_test_case "Extract Commands" test_extract_commands
    run_test_case "Backward Compatibility" test_backward_compatibility
    run_test_case "Danger Level String" test_danger_level_string
    
    # 結果サマリー
    echo ""
    echo "========================================="
    echo "Test Summary"
    echo "========================================="
    echo "Passed: $TESTS_PASSED"
    echo "Failed: $TESTS_FAILED"
    echo "Total:  $((TESTS_PASSED + TESTS_FAILED))"
    
    if [ $TESTS_FAILED -eq 0 ]; then
        echo ""
        log_info "All tests passed! ✓"
        return 0
    else
        echo ""
        log_error "Some tests failed. ✗"
        return 1
    fi
}

main "$@"

