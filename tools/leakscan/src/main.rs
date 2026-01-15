use regex::Regex;
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, BufRead};

#[derive(Deserialize, Debug)]
struct Rule {
    id: String,
    regex: String,
    keywords: Option<Vec<String>>,
    entropy: Option<f64>,
}

struct CompiledRule {
    id: String,
    re: Regex,
    keywords: Option<Vec<String>>,
    entropy: Option<f64>,
}

fn shannon_entropy(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let mut freq = HashMap::new();
    let mut total_chars = 0;
    for c in s.chars() {
        *freq.entry(c).or_insert(0) += 1;
        total_chars += 1;
    }
    let len = total_chars as f64;
    let mut entropy = 0.0;
    for &count in freq.values() {
        let f = count as f64 / len;
        entropy -= f * f.log2();
    }
    entropy
}

fn main() {
    let mut args: Vec<String> = env::args().collect();
    let mut verbose = false;
    let mut color = false;

    if let Some(pos) = args.iter().position(|x| x == "-v") {
        verbose = true;
        args.remove(pos);
    }
    if let Some(pos) = args.iter().position(|x| x == "--color") {
        color = true;
        args.remove(pos);
    }

    if args.len() != 2 {
        eprintln!("Usage: cat file.txt | {} [-v] [--color] <rules.json>", args[0]);
        std::process::exit(1);
    }

    let rules_file = &args[1];
    let rules_content = match fs::read_to_string(rules_file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error reading rules file: {}", e);
            std::process::exit(1);
        }
    };

    let raw_rules: Vec<Rule> = match serde_json::from_str(&rules_content) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error parsing rules JSON: {}", e);
            std::process::exit(1);
        }
    };

    let rules: Vec<CompiledRule> = raw_rules
        .into_iter()
        .map(|r| CompiledRule {
            id: r.id,
            re: Regex::new(&r.regex).unwrap_or_else(|_| panic!("Invalid regex: {}", r.regex)),
            keywords: r.keywords,
            entropy: r.entropy,
        })
        .collect();

    let stdin = io::stdin();
    let mut stdout = io::stdout();
    use std::io::Write;

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let line_clean = line.trim();
        if line_clean.is_empty() {
            continue;
        }

        let mut matched_rule_id: Option<String> = None;
        let mut match_range: Option<(usize, usize)> = None;

        for rule in &rules {
            // Step 1: Keyword check
            if let Some(ref keywords) = rule.keywords {
                if !keywords.iter().any(|kw| line_clean.contains(kw)) {
                    continue;
                }
            }

            // Step 2: Regex check
            if let Some(m) = rule.re.find(line_clean) {
                // Step 3: Entropy check
                if let Some(threshold) = rule.entropy {
                    let matched_str = m.as_str();
                    if shannon_entropy(matched_str) < threshold {
                        continue;
                    }
                }
                matched_rule_id = Some(rule.id.clone());
                match_range = Some((m.start(), m.end()));
                break;
            }
        }

        if let Some(id) = matched_rule_id {
            let output_line = if color {
                if let Some((start, end)) = match_range {
                    format!(
                        "{}\x1b[1;31m{}\x1b[0m{}",
                        &line_clean[..start],
                        &line_clean[start..end],
                        &line_clean[end..]
                    )
                } else {
                    line_clean.to_string()
                }
            } else {
                line_clean.to_string()
            };

            if verbose {
                let _ = writeln!(stdout, "{}  (match={})", output_line, id);
            } else {
                let _ = writeln!(stdout, "{}", output_line);
            }
        }
    }
}