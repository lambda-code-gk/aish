#!/usr/bin/env bash
# Architecture tests (AGENTS.md の逆流禁止・依存方向の検証)
# 失敗時は "Architecture violation: ..." を表示して exit 1

set -e
USECASE_DIRS="core/ai/src/usecase core/aish/src/usecase"
MAIN_FILES="core/ai/src/main.rs core/aish/src/main.rs"
PORTS_DIRS="core/ai/src/ports core/aish/src/ports"
DOMAIN_DIRS="core/ai/src/domain core/aish/src/domain"

fail() { echo "Architecture violation: $1"; exit 1; }

# usecase は adapter / cli / wiring に依存しない
rg "crate::adapter" $USECASE_DIRS 2>/dev/null && fail "usecase must not depend on adapter" || true
rg "crate::cli" $USECASE_DIRS 2>/dev/null && fail "usecase must not depend on cli" || true
rg "crate::wiring" $USECASE_DIRS 2>/dev/null && fail "usecase must not depend on wiring" || true

# usecase 内で std::env を直接読まない
rg "std::env" $USECASE_DIRS 2>/dev/null && fail "usecase must not use std::env directly" || true

# usecase 内で stdout / stderr に直接出力しない
rg "println!|eprintln!|std::io::stdout|std::io::stderr" $USECASE_DIRS 2>/dev/null && fail "usecase must not use println!/eprintln!/stdout/stderr directly" || true

# main は adapter を直接 use しない（wiring 経由のみ）
rg "crate::adapter|use .*adapter::" $MAIN_FILES 2>/dev/null && fail "main must not depend on adapter (use wiring only)" || true

# common に usecase を置かない（ai 専用・aish 専用は各 crate に）
[ -d core/common/src/usecase ] && fail "common must not have usecase directory" || true

# ports は adapter に依存しない
rg "crate::adapter" $PORTS_DIRS 2>/dev/null && fail "ports must not depend on adapter" || true

# domain は adapter / cli / wiring に依存しない
rg "crate::adapter|crate::cli|crate::wiring" $DOMAIN_DIRS 2>/dev/null && fail "domain must not depend on adapter, cli, or wiring" || true

echo "Architecture checks passed."
