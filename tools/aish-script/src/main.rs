use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::process;
use std::thread;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::Value;
use regex::Regex;

mod platform;
use platform::FileWatcher;

// JSONL行をパースしてtype, data, encフィールドを抽出
fn parse_jsonl_line(line: &str) -> Option<(String, Option<String>, Option<String>)> {
    let json: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return None,
    };
    
    let event_type = json.get("type")?.as_str()?.to_string();
    let data = json.get("data").and_then(|v| v.as_str()).map(|s| s.to_string());
    let enc = json.get("enc").and_then(|v| v.as_str()).map(|s| s.to_string());
    
    Some((event_type, data, enc))
}

// DSLパーサー（簡易版）
#[derive(Debug, Clone)]
enum PatternType {
    String(String),
    Regex(Regex, String), // (Regex, original pattern string for debug)
}

#[derive(Debug, Clone)]
struct Rule {
    pattern: PatternType,
    send_text: String,
}

fn parse_dsl(script: &str) -> Result<Vec<Rule>, String> {
    let mut rules = Vec::new();
    let parts: Vec<&str> = script.split(';').collect();
    
    for part in parts {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        
        // match "pattern" then send "text" または match /pattern/flags then send "text" の形式をパース
        if !part.starts_with("match ") {
            return Err(format!("Expected 'match' at start of rule: {}", part));
        }
        
        let part = &part[6..]; // "match " を削除
        let part = part.trim();
        
        // 正規表現パターンか文字列パターンかを判定
        let (pattern_type, remaining): (PatternType, &str) = if part.starts_with('/') {
            // 正規表現パターン: /pattern/flags
            let pattern_end = part[1..].find('/');
            if pattern_end.is_none() {
                return Err(format!("Expected closing '/' for regex pattern: {}", part));
            }
            let pattern_end = pattern_end.unwrap() + 1;
            let pattern_str = part[1..pattern_end - 1].to_string();
            
            // フラグを取得（/ の後）
            let flags_part = &part[pattern_end..];
            let flags_end = flags_part.find(' ').unwrap_or(flags_part.len());
            let flags = &flags_part[..flags_end];
            
            // フラグをパターン文字列に埋め込む形式に変換
            let mut final_pattern = String::new();
            if flags.contains('i') {
                final_pattern.push_str("(?i)");
            }
            if flags.contains('m') {
                final_pattern.push_str("(?m)");
            }
            final_pattern.push_str(&pattern_str);
            
            // Regexを構築
            let regex = Regex::new(&final_pattern)
                .map_err(|e| format!("Invalid regex pattern '{}': {}", pattern_str, e))?;
            
            let remaining = flags_part[flags_end..].trim();
            (PatternType::Regex(regex, pattern_str), remaining)
        } else {
            // 文字列パターン: "pattern"
            let pattern_start = part.find('"');
            if pattern_start.is_none() {
                return Err(format!("Expected quoted pattern: {}", part));
            }
            let pattern_start = pattern_start.unwrap() + 1;
            let pattern_end = part[pattern_start..].find('"');
            if pattern_end.is_none() {
                return Err(format!("Expected closing quote for pattern: {}", part));
            }
            let pattern_end = pattern_start + pattern_end.unwrap();
            let pattern = part[pattern_start..pattern_end].to_string();
            
            let remaining = part[pattern_end + 1..].trim();
            (PatternType::String(pattern), remaining)
        };
        
        if !remaining.starts_with("then send ") {
            return Err(format!("Expected 'then send' after pattern: {}", remaining));
        }
        
        let send_part = &remaining[10..]; // "then send " を削除
        let send_start = send_part.find('"');
        if send_start.is_none() {
            return Err(format!("Expected quoted send text: {}", send_part));
        }
        let send_start = send_start.unwrap() + 1;
        let send_end = send_part[send_start..].find('"');
        if send_end.is_none() {
            return Err(format!("Expected closing quote for send text: {}", send_part));
        }
        let send_end = send_start + send_end.unwrap();
        let send_text = send_part[send_start..send_end].to_string();
        
        // \nなどのエスケープシーケンスを処理
        let send_text = send_text.replace("\\n", "\n").replace("\\r", "\r").replace("\\t", "\t");
        
        rules.push(Rule {
            pattern: pattern_type,
            send_text,
        });
    }
    
    Ok(rules)
}

struct Config {
    log_file: Option<String>,
    execute_script: Option<String>,
    script_file: Option<String>,
    input_fifo: Option<String>,
    from_beginning: bool,
    follow: bool,
    poll_interval: u64,
    debug: bool,
    verbose: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            log_file: None,
            execute_script: None,
            script_file: None,
            input_fifo: None,
            from_beginning: false,
            follow: false,
            poll_interval: 100,
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
            "-f" | "--file" => {
                i += 1;
                if i >= args.len() {
                    return Err(("Option requires an argument".to_string(), 64));
                }
                config.log_file = Some(args[i].clone());
                i += 1;
            }
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
            "--input-fifo" => {
                i += 1;
                if i >= args.len() {
                    return Err(("Option requires an argument".to_string(), 64));
                }
                config.input_fifo = Some(args[i].clone());
                i += 1;
            }
            "--from-beginning" => {
                config.from_beginning = true;
                i += 1;
            }
            "--follow" => {
                config.follow = true;
                i += 1;
            }
            "--poll-interval" => {
                i += 1;
                if i >= args.len() {
                    return Err(("Option requires an argument".to_string(), 64));
                }
                config.poll_interval = args[i].parse()
                    .map_err(|_| ("Invalid poll-interval value".to_string(), 64))?;
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
    
    // 必須オプションのチェック
    let log_file = config.log_file.as_ref().ok_or_else(|| {
        ("--file option is required".to_string(), 64)
    })?;
    
    let input_fifo = config.input_fifo.as_ref().ok_or_else(|| {
        ("--input-fifo option is required".to_string(), 64)
    })?;
    
    // スクリプトの取得
    let script = if let Some(ref script_file) = config.script_file {
        std::fs::read_to_string(script_file)
            .map_err(|e| (format!("Failed to read script file: {}", e), 74))?
    } else if let Some(ref execute_script) = config.execute_script {
        execute_script.clone()
    } else {
        return Err(("Either --execute or --script option is required".to_string(), 64));
    };
    
    // DSLパーサーでルールを取得
    let rules = parse_dsl(&script)
        .map_err(|e| (format!("Failed to parse DSL: {}", e), 64))?;
    
    if config.debug {
        eprintln!("Parsed rules: {:?}", rules);
    }
    
    // FIFOを開く
    let mut fifo = File::create(input_fifo)
        .map_err(|e| (format!("Failed to open FIFO: {}", e), 74))?;
    
    // ファイルの行を処理する関数
    let process_line = |line: &str, accumulated_output: &mut String, fifo: &mut File, rules: &[Rule], debug: bool, verbose: bool| -> Result<bool, (String, i32)> {
        if let Some((event_type, data_opt, enc_opt)) = parse_jsonl_line(line) {
            if event_type == "stdout" {
                if let Some(data_str) = data_opt {
                    let data = if enc_opt.as_ref().map(|e| e == "b64").unwrap_or(false) {
                        STANDARD.decode(&data_str)
                            .map_err(|e| (format!("Failed to decode base64: {}", e), 74))?
                    } else {
                        data_str.as_bytes().to_vec()
                    };
                    
                    let text = String::from_utf8_lossy(&data);
                    accumulated_output.push_str(&text);
                    
                    // 各ルールに対してマッチングを試行
                    for rule in rules {
                        let matched = match &rule.pattern {
                            PatternType::String(pattern) => {
                                accumulated_output.contains(pattern)
                            }
                            PatternType::Regex(regex, _pattern_str) => {
                                regex.is_match(accumulated_output)
                            }
                        };
                        
                        if matched {
                            let pattern_display = match &rule.pattern {
                                PatternType::String(p) => p.clone(),
                                PatternType::Regex(_, p) => format!("/{}/", p),
                            };
                            
                            if debug || verbose {
                                eprintln!("Matched pattern: {}", pattern_display);
                            }
                            
                            // FIFOに送信
                            fifo.write_all(rule.send_text.as_bytes())
                                .map_err(|e| (format!("Failed to write to FIFO: {}", e), 74))?;
                            fifo.flush()
                                .map_err(|e| (format!("Failed to flush FIFO: {}", e), 74))?;
                            
                            if debug || verbose {
                                eprintln!("Sent to FIFO: {:?}", rule.send_text);
                            }
                            
                            // マッチした部分を削除（簡易版：最初のマッチのみ処理）
                            match &rule.pattern {
                                PatternType::String(pattern) => {
                                    if let Some(pos) = accumulated_output.find(pattern) {
                                        *accumulated_output = accumulated_output[pos + pattern.len()..].to_string();
                                    }
                                }
                                PatternType::Regex(regex, _) => {
                                    if let Some(m) = regex.find(accumulated_output) {
                                        *accumulated_output = accumulated_output[m.end()..].to_string();
                                    }
                                }
                            }
                            return Ok(true); // マッチした
                        }
                    }
                }
            }
        }
        Ok(false) // マッチしなかった
    };
    
    let mut accumulated_output = String::new();
    
    if config.follow {
        // リアルタイム監視モード
        let mut watcher = FileWatcher::new(log_file, config.poll_interval)
            .map_err(|e| (format!("Failed to create file watcher: {}", e), 74))?;
        
        if !config.from_beginning {
            // ファイルの末尾から読み取りを開始
            watcher.seek_to_end()
                .map_err(|e| (format!("Failed to seek to end: {}", e), 74))?;
        }
        
        // 最初に既存の内容を読み取る（from_beginningが指定されている場合）
        if config.from_beginning {
            let file = File::open(log_file)
                .map_err(|e| (format!("Failed to open log file: {}", e), 74))?;
            let reader = BufReader::new(file);
            
            for line in reader.lines() {
                let line = line.map_err(|e| (format!("Failed to read line: {}", e), 74))?;
                process_line(&line, &mut accumulated_output, &mut fifo, &rules, config.debug, config.verbose)?;
            }
        }
        
        // ポーリングループ
        loop {
            match watcher.read_new_lines() {
                Ok(lines) => {
                    for line in lines {
                        let line = line.trim_end_matches('\n').trim_end_matches('\r');
                        if !line.is_empty() {
                            process_line(&line, &mut accumulated_output, &mut fifo, &rules, config.debug, config.verbose)?;
                        }
                    }
                }
                Err(e) => {
                    if config.debug {
                        eprintln!("Error reading new lines: {}", e);
                    }
                }
            }
            
            thread::sleep(watcher.poll_interval());
        }
    } else {
        // 通常モード（ファイルを最初から最後まで読み取る）
        let file = File::open(log_file)
            .map_err(|e| (format!("Failed to open log file: {}", e), 74))?;
        let reader = BufReader::new(file);
        
        for line in reader.lines() {
            let line = line.map_err(|e| (format!("Failed to read line: {}", e), 74))?;
            process_line(&line, &mut accumulated_output, &mut fifo, &rules, config.debug, config.verbose)?;
        }
    }
    
    Ok(0)
}
