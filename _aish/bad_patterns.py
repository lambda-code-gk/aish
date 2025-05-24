#!/usr/bin/env python3
import sys
import math
import re

# 英単語リスト（/usr/share/dict/words など）を使いたければファイル読み込みに変更可能
SAFE_WORDS = {
    "password", "secret", "token", "admin", "login", "access", "private", "github",
    "echo ", "grep ", "find ", "cat ", "ls ", "rm ", "cp ", "mv ", "chmod ", "chown ",
    "sed ", "touch ", "mkdir ", "rmdir ", "tar ", "gzip ", "gunzip ", "zip ", "unzip ",
}
SAFE_PATTERNS = [
    # ソースコードのようなもの
    # メソッド呼び出し .hoge()
    re.compile(r'\.?\w+\s*\(.*?\)'),
    # メソッド呼び出し .hoge<T>()
    re.compile(r'\.\w+<.*?>\s*\(.*?\)'),
    # 代入 hoge = 1
    re.compile(r'^\s*\w+\s*=\s*.*$'),
    # if hoge == 1
    re.compile(r'^\s*if\s+\w+\s*==\s*.*$'),
]

def load_patterns(pattern_file):
    patterns = []
    try:
        with open(pattern_file, 'r', encoding='utf-8') as f:
            for line in f:
                line = line.strip()
                if line == '' or line.startswith('#'):
                    continue
                try:
                    patterns.append(re.compile(line))
                except re.error as e:
                    print(f"パターンエラー: {line} → {e}", file=sys.stderr)
    except Exception as e:
        print(f"パターンファイル読み込み失敗: {e}", file=sys.stderr)
        sys.exit(1)
    return patterns

def shannon_entropy(s):
    freq = {}
    for c in s:
        freq[c] = freq.get(c, 0) + 1
    entropy = -sum((f / len(s)) * math.log2(f / len(s)) for f in freq.values())
    return entropy

def char_diversity(s):
    categories = [
        bool(re.search(r'[A-Z]', s)),
        bool(re.search(r'[a-z]', s)),
        bool(re.search(r'[0-9]', s)),
        bool(re.search(r'[^A-Za-z0-9]', s)),
    ]
    return sum(categories)

def contains_dictionary_word(s):
    s_lower = s.lower()
    for word in SAFE_WORDS:
        if word in s_lower:
            return True
    return False

def analyze_stdin(pattern_file, verbose=False, skip_heuristics=False):
    patterns = load_patterns(pattern_file)

    for line in sys.stdin:
        line = line.strip()

        # パターンマッチ → 一発アウト
        if any(p.search(line) for p in patterns):
            if verbose:
                print(f"{line}  (match=pattern)")
            else:
                print(f"{line}")
            continue

        if skip_heuristics:
            continue

        # ヒューリスティック検出（誤検知抑制モード）
        if len(line) < 30:
            continue
        if contains_dictionary_word(line):
            continue
        if any(p.search(line) for p in SAFE_PATTERNS):
            continue

        entropy = shannon_entropy(line)
        diversity = char_diversity(line)

        if entropy >= 4.5 and diversity >= 3:
            if verbose:
                print(f"{line}  (entropy={entropy:.2f}, diversity={diversity})")
            else:
                print(f"{line}")

if __name__ == "__main__":
    verbose = False
    skip_heuristics = False  # エントロピー計算を無視するオプション
    if len(sys.argv) > 2:
        if '-v' in sys.argv:
            verbose = True
            sys.argv.remove('-v')
        if '--skip-heuristics' in sys.argv:
            skip_heuristics = True
            sys.argv.remove('--skip-heuristics')
    if len(sys.argv) != 2:
        print("使い方: cat file.txt | python heuristic_dlp.py [オプション] <パターンファイル>")
        print("オプション:")
        print("  -v                詳細モード")
        print("  --skip-heuristics ヒューリスティック検出をスキップ")
        sys.exit(1)

    pattern_file = sys.argv[1]
    analyze_stdin(pattern_file, verbose, skip_heuristics)
