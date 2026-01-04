use std::io::{self, BufRead, Write};
use std::process;
use std::time::{Duration, Instant};

mod dsl;

use dsl::parse_script;

struct Config {
    execute_script: Option<String>,
    script_file: Option<String>,
    debug: bool,
    verbose: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            execute_script: None,
            script_file: None,
            debug: false,
            verbose: false,
        }
    }
}

fn main() {
    let exit_code = match run() {
        Ok(code) => code,
        Err((msg, code)) => {
            eprintln!("aish-script: {}", msg);
            code
        }
    };
    process::exit(exit_code);
}

fn run() -> Result<i32, (String, i32)> {
    let args: Vec<String> = std::env::args().collect();
    let mut config = Config::default();
    
    // コマンドライン引数解析
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-e" | "--execute" => {
                i += 1;
                if i >= args.len() {
                    return Err(("Option requires an argument".to_string(), 64));
                }
                config.execute_script = Some(args[i].clone());
                i += 1;
            }
            "-s" | "--script" => {
                i += 1;
                if i >= args.len() {
                    return Err(("Option requires an argument".to_string(), 64));
                }
                config.script_file = Some(args[i].clone());
                i += 1;
            }
            "--debug" => {
                config.debug = true;
                i += 1;
            }
            "--verbose" => {
                config.verbose = true;
                i += 1;
            }
            _ if args[i].starts_with('-') => {
                return Err((format!("Unknown option: {}", args[i]), 64));
            }
            _ => {
                return Err((format!("Unexpected argument: {}", args[i]), 64));
            }
        }
    }
    
    // スクリプトの取得
    let script = if let Some(ref script_file) = config.script_file {
        std::fs::read_to_string(script_file)
            .map_err(|e| (format!("Failed to read script file: {}", e), 74))?
    } else if let Some(ref execute_script) = config.execute_script {
        execute_script.clone()
    } else {
        return Err(("Either --execute or --script option is required".to_string(), 64));
    };
    
    // DSLパーサーでスクリプトを取得
    let script = parse_script(&script)
        .map_err(|e| (format!("Failed to parse DSL: {}", e), 64))?;
    
    let rules = &script.rules;
    let mut current_state = script.initial_state.clone();
    
    if config.debug {
        eprintln!("Initial state: {}", current_state);
        eprintln!("Parsed {} rules", rules.len());
        for (i, rule) in rules.iter().enumerate() {
            let pattern_display = match &rule.pattern {
                dsl::Pattern::String(s) => format!("\"{}\"", s),
                dsl::Pattern::Regex(_) => "regex".to_string(),
            };
            let state_display = rule.state.as_ref().map(|s| s.as_str()).unwrap_or("any");
            let next_state_display = rule.next_state.as_ref().map(|s| s.as_str()).unwrap_or("none");
            eprintln!("  Rule {}: [state: {}] match {} then send {:?} goto {} (timeout: {:?})", 
                     i + 1, state_display, pattern_display, rule.response, next_state_display, rule.timeout);
        }
    }
    
    // 標準出力（バッファなし）
    let stdout = io::stdout();
    let mut stdout_lock = stdout.lock();
    
    // タイムアウト管理用の構造体
    struct RuleTimeout {
        rule_index: usize,
        timeout_sec: u64,
        start_time: Instant,
    }
    
    let mut rule_timeouts: Vec<RuleTimeout> = rules.iter()
        .enumerate()
        .filter_map(|(i, rule)| {
            rule.timeout.map(|timeout_sec| RuleTimeout {
                rule_index: i,
                timeout_sec,
                start_time: Instant::now(),
            })
        })
        .collect();
    
    // 標準入力から読み取り
    let stdin = io::stdin();
    let mut stdin_lock = stdin.lock();
    let mut accumulated_output = String::new();
    let mut buffer = String::new();
    
    loop {
        // タイムアウトチェック
        let now = Instant::now();
        for timeout_info in &rule_timeouts {
            let elapsed = now.duration_since(timeout_info.start_time);
            if elapsed >= Duration::from_secs(timeout_info.timeout_sec) {
                let rule = &rules[timeout_info.rule_index];
                let pattern_display = match &rule.pattern {
                    dsl::Pattern::String(s) => format!("\"{}\"", s),
                    dsl::Pattern::Regex(_) => "regex".to_string(),
                };
                return Err((format!("Timeout: Pattern {} not found within {} seconds", pattern_display, timeout_info.timeout_sec), 1));
            }
        }
        
        // 標準入力から1行読み取り
        buffer.clear();
        match stdin_lock.read_line(&mut buffer) {
            Ok(0) => {
                // EOF（標準入力が閉じられた）
                break;
            }
            Ok(_) => {
                // 行を読み取った
                accumulated_output.push_str(&buffer);
                
                if config.debug {
                    eprintln!("Accumulated output length: {} bytes", accumulated_output.len());
                }
                
                // 現在の状態に適用可能なルールをフィルタリング
                let applicable_rules: Vec<(usize, &dsl::Rule)> = rules.iter()
                    .enumerate()
                    .filter(|(_, rule)| {
                        // 状態が指定されていないルールは全状態で有効
                        // 状態が指定されているルールは現在の状態と一致する場合のみ有効
                        rule.state.as_ref().map(|s| s == &current_state).unwrap_or(true)
                    })
                    .collect();
                
                // 適用可能なルールに対してマッチングを試行
                for (rule_index, rule) in applicable_rules {
                    if rule.matches(&accumulated_output) {
                        let pattern_display = match &rule.pattern {
                            dsl::Pattern::String(s) => format!("\"{}\"", s),
                            dsl::Pattern::Regex(_) => "regex".to_string(),
                        };
                        
                        if config.debug || config.verbose {
                            eprintln!("Matched pattern: {} (current state: {})", pattern_display, current_state);
                        }
                        
                        // 標準出力に送信
                        match stdout_lock.write_all(rule.response.as_bytes()) {
                            Ok(_) => {
                                if let Err(e) = stdout_lock.flush() {
                                    return Err((format!("Failed to flush stdout: {}", e), 74));
                                }
                            }
                            Err(e) => {
                                return Err((format!("Failed to write to stdout: {}", e), 74));
                            }
                        }
                        
                        if config.debug || config.verbose {
                            eprintln!("Sent to stdout: {:?}", rule.response);
                        }
                        
                        // マッチした部分を削除
                        match &rule.pattern {
                            dsl::Pattern::String(pattern) => {
                                if let Some(pos) = accumulated_output.find(pattern) {
                                    accumulated_output = accumulated_output[pos + pattern.len()..].to_string();
                                }
                            }
                            dsl::Pattern::Regex(regex) => {
                                if let Some(m) = regex.find(&accumulated_output) {
                                    accumulated_output = accumulated_output[m.end()..].to_string();
                                }
                            }
                        }
                        
                        // 状態遷移
                        if let Some(ref next_state) = rule.next_state {
                            if config.debug || config.verbose {
                                eprintln!("State transition: {} -> {}", current_state, next_state);
                            }
                            current_state = next_state.clone();
                        }
                        
                        // マッチしたルールのタイムアウトをクリア
                        rule_timeouts.retain(|rt| rt.rule_index != rule_index);
                        
                        break; // 最初にマッチしたルールのみ処理
                    }
                }
            }
            Err(e) => {
                return Err((format!("Failed to read from stdin: {}", e), 74));
            }
        }
    }
    
    // 標準入力読み取り完了後、タイムアウトが設定されているルールがマッチしなかった場合
    for timeout_info in &rule_timeouts {
        let rule = &rules[timeout_info.rule_index];
        let pattern_display = match &rule.pattern {
            dsl::Pattern::String(s) => format!("\"{}\"", s),
            dsl::Pattern::Regex(_) => "regex".to_string(),
        };
        return Err((format!("Timeout: Pattern {} not found within {} seconds", pattern_display, timeout_info.timeout_sec), 1));
    }
    
    Ok(0)
}
