use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::process;
use std::thread;
use std::time::{Duration, Instant};
use base64::{Engine as _, engine::general_purpose::STANDARD};

mod platform;
mod dsl;
mod jsonl;

use platform::FileWatcher;
use dsl::{Rule, parse_script};

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
    let rules = parse_script(&script)
        .map_err(|e| (format!("Failed to parse DSL: {}", e), 64))?;
    
    if config.debug {
        eprintln!("Parsed {} rules", rules.len());
        for (i, rule) in rules.iter().enumerate() {
            let pattern_display = match &rule.pattern {
                dsl::Pattern::String(s) => format!("\"{}\"", s),
                dsl::Pattern::Regex(_) => "regex".to_string(),
            };
            eprintln!("  Rule {}: match {} then send {:?} (timeout: {:?})", 
                     i + 1, pattern_display, rule.response, rule.timeout);
        }
    }
    
    // FIFOを開く
    let mut fifo = File::create(input_fifo)
        .map_err(|e| (format!("Failed to open FIFO: {}", e), 74))?;
    
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
    
    // ファイルの行を処理する関数
    let process_line = |line: &str, accumulated_output: &mut String, fifo: &mut File, rules: &[Rule], debug: bool, verbose: bool| -> Result<Option<usize>, (String, i32)> {
        // 不正なJSONL行を無視（エラーログは出力するが処理は継続）
        let (event_type, data_opt, enc_opt) = match jsonl::parse_line(line) {
            Some(result) => result,
            None => {
                if debug {
                    eprintln!("Warning: Failed to parse JSONL line (ignoring): {}", line.chars().take(100).collect::<String>());
                }
                return Ok(None);
            }
        };
        
        if event_type == "stdout" {
            if let Some(data_str) = data_opt {
                let data = if enc_opt.as_ref().map(|e| e == "b64").unwrap_or(false) {
                    match STANDARD.decode(&data_str) {
                        Ok(decoded) => decoded,
                        Err(e) => {
                            if debug {
                                eprintln!("Warning: Failed to decode base64 (ignoring): {}", e);
                            }
                            return Ok(None);
                        }
                    }
                } else {
                    data_str.as_bytes().to_vec()
                };
                
                let text = String::from_utf8_lossy(&data);
                accumulated_output.push_str(&text);
                
                if debug {
                    eprintln!("Accumulated output length: {} bytes", accumulated_output.len());
                }
                
                // 各ルールに対してマッチングを試行
                for (rule_index, rule) in rules.iter().enumerate() {
                    if rule.matches(accumulated_output) {
                        let pattern_display = match &rule.pattern {
                            dsl::Pattern::String(s) => format!("\"{}\"", s),
                            dsl::Pattern::Regex(_) => "regex".to_string(),
                        };
                        
                        if debug || verbose {
                            eprintln!("Matched pattern: {}", pattern_display);
                        }
                        
                        // FIFOに送信
                        match fifo.write_all(rule.response.as_bytes()) {
                            Ok(_) => {
                                if let Err(e) = fifo.flush() {
                                    return Err((format!("Failed to flush FIFO: {}", e), 74));
                                }
                            }
                            Err(e) => {
                                return Err((format!("Failed to write to FIFO: {}", e), 74));
                            }
                        }
                        
                        if debug || verbose {
                            eprintln!("Sent to FIFO: {:?}", rule.response);
                        }
                        
                        // マッチした部分を削除（簡易版：最初のマッチのみ処理）
                        match &rule.pattern {
                            dsl::Pattern::String(pattern) => {
                                if let Some(pos) = accumulated_output.find(pattern) {
                                    *accumulated_output = accumulated_output[pos + pattern.len()..].to_string();
                                }
                            }
                            dsl::Pattern::Regex(regex) => {
                                if let Some(m) = regex.find(accumulated_output) {
                                    *accumulated_output = accumulated_output[m.end()..].to_string();
                                }
                            }
                        }
                        
                        return Ok(Some(rule_index)); // マッチしたルールのインデックスを返す
                    }
                }
            }
        }
        Ok(None) // マッチしなかった
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
                let line = match line {
                    Ok(l) => l,
                    Err(e) => {
                        if config.debug {
                            eprintln!("Warning: Failed to read line (ignoring): {}", e);
                        }
                        continue;
                    }
                };
                match process_line(&line, &mut accumulated_output, &mut fifo, &rules, config.debug, config.verbose) {
                    Ok(Some(matched_rule_index)) => {
                        // マッチしたルールのタイムアウトをクリア
                        rule_timeouts.retain(|rt| rt.rule_index != matched_rule_index);
                    }
                    Ok(None) => {}
                    Err(e) => return Err(e),
                }
            }
        }
        
        // ポーリングループ
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
            
            match watcher.read_new_lines() {
                Ok(lines) => {
                    for line in lines {
                        let line = line.trim_end_matches('\n').trim_end_matches('\r');
                        if !line.is_empty() {
                            match process_line(&line, &mut accumulated_output, &mut fifo, &rules, config.debug, config.verbose) {
                                Ok(Some(matched_rule_index)) => {
                                    // マッチしたルールのタイムアウトをクリア
                                    rule_timeouts.retain(|rt| rt.rule_index != matched_rule_index);
                                }
                                Ok(None) => {}
                                Err(e) => return Err(e),
                            }
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
            
            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    if config.debug {
                        eprintln!("Warning: Failed to read line (ignoring): {}", e);
                    }
                    continue;
                }
            };
            
            match process_line(&line, &mut accumulated_output, &mut fifo, &rules, config.debug, config.verbose) {
                Ok(Some(matched_rule_index)) => {
                    // マッチしたルールのタイムアウトをクリア
                    rule_timeouts.retain(|rt| rt.rule_index != matched_rule_index);
                }
                Ok(None) => {}
                Err(e) => return Err(e),
            }
        }
        
        // ファイル読み取り完了後、タイムアウトが設定されているルールがマッチしなかった場合
        for timeout_info in &rule_timeouts {
            let rule = &rules[timeout_info.rule_index];
            let pattern_display = match &rule.pattern {
                dsl::Pattern::String(s) => format!("\"{}\"", s),
                dsl::Pattern::Regex(_) => "regex".to_string(),
            };
            return Err((format!("Timeout: Pattern {} not found within {} seconds", pattern_display, timeout_info.timeout_sec), 1));
        }
    }
    
    Ok(0)
}
