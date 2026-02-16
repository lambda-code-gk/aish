#!/usr/bin/env bash
# .sandbox/ と dist/ を削除して作り直す（確認プロンプト or --force）
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SANDBOX="$PROJECT_ROOT/.sandbox"
DIST="$PROJECT_ROOT/dist"

do_reset() {
    rm -rf "$SANDBOX" "$DIST"
    echo "Removed .sandbox and dist. Run ./build.sh to rebuild."
}

if [ "$1" = "--force" ] || [ "$1" = "-f" ]; then
    do_reset
    exit 0
fi

if [ -d "$SANDBOX" ] || [ -d "$DIST" ]; then
    echo "This will remove: $SANDBOX $DIST"
    echo -n "Continue? [y/N] "
    read -r r
    if [ "$r" = "y" ] || [ "$r" = "Y" ]; then
        do_reset
    else
        echo "Aborted."
    fi
else
    echo "Nothing to reset (.sandbox and dist not present)."
fi
