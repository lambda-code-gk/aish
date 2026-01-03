use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::process;

// base64デコード関数（aish-renderから引用）
mod base64_util {
    pub fn decode(data: &str) -> Result<Vec<u8>, String> {
        let mut result = Vec::new();
        let mut i = 0;
        let bytes = data.as_bytes();
        
        while i < bytes.len() {
            if bytes[i] == b'=' {
                break;
            }
            
            let mut chunk = 0u32;
            let mut count = 0;
            
            while count < 4 && i < bytes.len() {
                let c = bytes[i];
                if c == b'=' {
                    break;
                }
                
                let value = match c {
                    b'A'..=b'Z' => (c - b'A') as u32,
                    b'a'..=b'z' => (c - b'a' + 26) as u32,
                    b'0'..=b'9' => (c - b'0' + 52) as u32,
                    b'+' => 62,
                    b'/' => 63,
                    _ => return Err(format!("Invalid base64 character: {}", c as char)),
                };
                
                chunk = (chunk << 6) | value;
                count += 1;
                i += 1;
            }
            
            match count {
                2 => {
                    result.push((chunk >> 4) as u8);
                }
                3 => {
                    result.push((chunk >> 10) as u8);
                    result.push((chunk >> 2) as u8);
                }
                4 => {
                    result.push((chunk >> 16) as u8);
                    result.push((chunk >> 8) as u8);
                    result.push(chunk as u8);
                }
                _ => {}
            }
            
            // Skip padding
            while i < bytes.len() && bytes[i] == b'=' {
                i += 1;
            }
        }
        
        Ok(result)
    }
}

// 最小限のJSONパーサー（type, data, encフィールドのみ抽出）
fn parse_jsonl_line(line: &str) -> Option<(String, Option<String>, Option<String>)> {
    let mut event_type: Option<String> = None;
    let mut data: Option<String> = None;
    let mut enc: Option<String> = None;
    
    let mut i = 0;
    while i < line.len() {
        if i + 6 < line.len() && &line[i..i+6] == "\"type\"" {
            i += 6;
            while i < line.len() && (line.as_bytes()[i] == b' ' || line.as_bytes()[i] == b':') {
                i += 1;
            }
            if i < line.len() && line.as_bytes()[i] == b'"' {
                i += 1;
                let start = i;
                while i < line.len() && line.as_bytes()[i] != b'"' {
                    if line.as_bytes()[i] == b'\\' && i + 1 < line.len() {
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                if i < line.len() {
                    event_type = Some(line[start..i].to_string());
                    i += 1;
                }
            }
        } else if i + 6 < line.len() && &line[i..i+6] == "\"data\"" {
            i += 6;
            while i < line.len() && (line.as_bytes()[i] == b' ' || line.as_bytes()[i] == b':') {
                i += 1;
            }
            if i < line.len() && line.as_bytes()[i] == b'"' {
                i += 1;
                let mut data_str = String::new();
                while i < line.len() {
                    if line.as_bytes()[i] == b'\\' && i + 1 < line.len() {
                        i += 1;
                        match line.as_bytes()[i] as char {
                            '"' => { data_str.push('"'); i += 1; }
                            '\\' => { data_str.push('\\'); i += 1; }
                            '/' => { data_str.push('/'); i += 1; }
                            'b' => { data_str.push('\x08'); i += 1; }
                            'f' => { data_str.push('\x0c'); i += 1; }
                            'n' => { data_str.push('\n'); i += 1; }
                            'r' => { data_str.push('\r'); i += 1; }
                            't' => { data_str.push('\t'); i += 1; }
                            'u' => {
                                i += 1;
                                let mut hex = String::new();
                                for _ in 0..4 {
                                    if i < line.len() {
                                        hex.push(line.as_bytes()[i] as char);
                                        i += 1;
                                    }
                                }
                                if let Ok(code) = u32::from_str_radix(&hex, 16) {
                                    if let Some(ch) = std::char::from_u32(code) {
                                        data_str.push(ch);
                                    }
                                }
                            }
                            _ => {
                                data_str.push('\\');
                                data_str.push(line.as_bytes()[i] as char);
                                i += 1;
                            }
                        }
                    } else if line.as_bytes()[i] == b'"' {
                        break;
                    } else {
                        data_str.push(line.as_bytes()[i] as char);
                        i += 1;
                    }
                }
                if i < line.len() {
                    data = Some(data_str);
                    i += 1;
                }
            }
        } else if i + 5 < line.len() && &line[i..i+5] == "\"enc\"" {
            i += 5;
            while i < line.len() && (line.as_bytes()[i] == b' ' || line.as_bytes()[i] == b':') {
                i += 1;
            }
            if i < line.len() && line.as_bytes()[i] == b'"' {
                i += 1;
                let start = i;
                while i < line.len() && line.as_bytes()[i] != b'"' {
                    if line.as_bytes()[i] == b'\\' && i + 1 < line.len() {
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                if i < line.len() {
                    enc = Some(line[start..i].to_string());
                    i += 1;
                }
            }
        } else {
            i += 1;
        }
    }
    
    if let Some(t) = event_type {
        Some((t, data, enc))
    } else {
        None
    }
}

// DSLパーサー（簡易版）
#[derive(Debug, Clone)]
struct Rule {
    pattern: String,
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
        
        // match "pattern" then send "text" の形式をパース
        if !part.starts_with("match ") {
            return Err(format!("Expected 'match' at start of rule: {}", part));
        }
        
        let part = &part[6..]; // "match " を削除
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
            pattern,
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
    
    // ログファイルを開く
    let file = File::open(log_file)
        .map_err(|e| (format!("Failed to open log file: {}", e), 74))?;
    let reader = BufReader::new(file);
    
    // JSONLを読み取ってパターンマッチング
    let mut accumulated_output = String::new();
    
    for line in reader.lines() {
        let line = line.map_err(|e| (format!("Failed to read line: {}", e), 74))?;
        
        if let Some((event_type, data_opt, enc_opt)) = parse_jsonl_line(&line) {
            if event_type == "stdout" {
                if let Some(data_str) = data_opt {
                    let data = if enc_opt.as_ref().map(|e| e == "b64").unwrap_or(false) {
                        base64_util::decode(&data_str)
                            .map_err(|e| (format!("Failed to decode base64: {}", e), 74))?
                    } else {
                        data_str.as_bytes().to_vec()
                    };
                    
                    let text = String::from_utf8_lossy(&data);
                    accumulated_output.push_str(&text);
                    
                    // 各ルールに対してマッチングを試行
                    for rule in &rules {
                        if accumulated_output.contains(&rule.pattern) {
                            if config.debug || config.verbose {
                                eprintln!("Matched pattern: {}", rule.pattern);
                            }
                            
                            // FIFOに送信
                            fifo.write_all(rule.send_text.as_bytes())
                                .map_err(|e| (format!("Failed to write to FIFO: {}", e), 74))?;
                            fifo.flush()
                                .map_err(|e| (format!("Failed to flush FIFO: {}", e), 74))?;
                            
                            if config.debug || config.verbose {
                                eprintln!("Sent to FIFO: {:?}", rule.send_text);
                            }
                            
                            // マッチした部分を削除（簡易版：最初のマッチのみ処理）
                            if let Some(pos) = accumulated_output.find(&rule.pattern) {
                                accumulated_output = accumulated_output[pos + rule.pattern.len()..].to_string();
                            }
                            break; // 最初にマッチしたルールのみ処理
                        }
                    }
                }
            }
        }
    }
    
    Ok(0)
}
