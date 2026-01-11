#!/usr/bin/env python3
import sys
import math
import re
import json

def shannon_entropy(s):
    if not s:
        return 0
    freq = {}
    for c in s:
        freq[c] = freq.get(c, 0) + 1
    entropy = -sum((f / len(s)) * math.log2(f / len(s)) for f in freq.values())
    return entropy

def load_rules(rules_file):
    try:
        with open(rules_file, 'r', encoding='utf-8') as f:
            rules = json.load(f)
            for rule in rules:
                rule['_re'] = re.compile(rule['regex'])
            return rules
    except Exception as e:
        print(f"Error loading rules: {e}", file=sys.stderr)
        sys.exit(1)

def analyze_stdin(rules_file, verbose=False):
    rules = load_rules(rules_file)

    for line in sys.stdin:
        line_clean = line.strip()
        if not line_clean:
            continue

        matched_rule = None
        for rule in rules:
            # Step 1: Keyword check (Fast)
            keywords = rule.get('keywords', [])
            if keywords and not any(kw in line_clean for kw in keywords):
                continue
            
            # Step 2: Regex check
            match = rule['_re'].search(line_clean)
            if match:
                # Step 3: Optional entropy check
                if 'entropy' in rule:
                    # Check entropy of the matched part or the whole line? 
                    # Usually the matched part is better.
                    matched_str = match.group(0)
                    if shannon_entropy(matched_str) < rule['entropy']:
                        continue
                
                matched_rule = rule
                break
        
        if matched_rule:
            if verbose:
                print(f"{line_clean}  (match={matched_rule['id']})")
            else:
                print(f"{line_clean}")

if __name__ == "__main__":
    verbose = False
    if '-v' in sys.argv:
        verbose = True
        sys.argv.remove('-v')
    
    if len(sys.argv) != 2:
        print("Usage: cat file.txt | python3 bad_patterns.py [-v] <rules.json>")
        sys.exit(1)

    rules_file = sys.argv[1]
    analyze_stdin(rules_file, verbose)
