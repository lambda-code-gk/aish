use regex::Regex;

pub struct Script {
    pub initial_state: String,
    pub rules: Vec<Rule>,
}

pub struct Rule {
    pub state: Option<String>,  // このルールが有効な状態（Noneの場合は全状態で有効）
    pub pattern: Pattern,
    pub response: String,
    pub timeout: Option<u64>,
    pub next_state: Option<String>,  // マッチ後の遷移先状態
}

#[derive(Debug)]
pub enum Pattern {
    String(String),
    Regex(Regex),
}

impl Rule {
    pub fn matches(&self, text: &str) -> bool {
        match &self.pattern {
            Pattern::String(s) => text.contains(s),
            Pattern::Regex(re) => re.is_match(text),
        }
    }
}

pub fn parse_script(script: &str) -> Result<Script, String> {
    let mut rules = Vec::new();
    let mut initial_state = "default".to_string();
    
    // ステートブロック構文を処理するため、より高度なパーシングが必要
    let mut i = 0;
    let script_chars: Vec<char> = script.chars().collect();
    
    while i < script_chars.len() {
        // 空白をスキップ
        while i < script_chars.len() && script_chars[i].is_whitespace() {
            i += 1;
        }
        if i >= script_chars.len() {
            break;
        }
        
        // "state" で始まる場合
        if script[i..].starts_with("state ") {
            i += 6; // "state " をスキップ
            
            // 空白をスキップ
            while i < script_chars.len() && script_chars[i].is_whitespace() {
                i += 1;
            }
            
            // 状態名を抽出
            if i >= script_chars.len() || script_chars[i] != '"' {
                return Err("State name must be quoted".to_string());
            }
            i += 1; // '"' をスキップ
            let name_start = i;
            while i < script_chars.len() && script_chars[i] != '"' {
                if script_chars[i] == '\\' && i + 1 < script_chars.len() {
                    i += 2; // エスケープシーケンスをスキップ
                } else {
                    i += 1;
                }
            }
            if i >= script_chars.len() {
                return Err("Unclosed state name".to_string());
            }
            let state_name: String = script[name_start..i].chars().collect();
            i += 1; // '"' をスキップ
            
            // 空白をスキップ
            while i < script_chars.len() && script_chars[i].is_whitespace() {
                i += 1;
            }
            
            // 中括弧があるかチェック
            if i < script_chars.len() && script_chars[i] == '{' {
                // ステートブロック構文: state "name" { ... }
                i += 1; // '{' をスキップ
                let block_start = i;
                
                // 対応する '}' を見つける（ネストを考慮）
                let mut brace_depth = 1;
                while i < script_chars.len() && brace_depth > 0 {
                    if script_chars[i] == '{' {
                        brace_depth += 1;
                    } else if script_chars[i] == '}' {
                        brace_depth -= 1;
                    }
                    if brace_depth > 0 {
                        i += 1;
                    }
                }
                if brace_depth > 0 {
                    return Err("Unclosed state block".to_string());
                }
                let block_content = &script[block_start..i].trim();
                i += 1; // '}' をスキップ
                
                // ブロック内のルールを解析
                let block_rules = parse_rules_in_block(block_content, Some(state_name.clone()))?;
                rules.extend(block_rules);
                
                // 最初の状態ブロックが初期状態
                if initial_state == "default" {
                    initial_state = state_name;
                }
            } else {
                // 単純な状態宣言: state "name"
                if initial_state == "default" {
                    initial_state = state_name;
                }
            }
            
            // セミコロンをスキップ（あれば）
            while i < script_chars.len() && script_chars[i].is_whitespace() {
                i += 1;
            }
            if i < script_chars.len() && script_chars[i] == ';' {
                i += 1;
            }
        } else {
            // 通常のルールまたはセミコロンで区切られた部分を解析
            let mut part_end = i;
            while part_end < script_chars.len() && script_chars[part_end] != ';' {
                part_end += 1;
            }
            let part = script[i..part_end].trim();
            if !part.is_empty() {
                let rule = parse_rule(part)?;
                rules.push(rule);
            }
            i = part_end;
            if i < script_chars.len() && script_chars[i] == ';' {
                i += 1;
            }
        }
    }
    
    Ok(Script {
        initial_state,
        rules,
    })
}

fn parse_rules_in_block(block_content: &str, state_name: Option<String>) -> Result<Vec<Rule>, String> {
    let mut rules = Vec::new();
    
    // セミコロンで区切られたルールを解析
    let parts: Vec<&str> = block_content.split(';').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    
    for part in parts {
        let mut rule = parse_rule(part)?;
        // ブロック内のルールには状態を設定
        if let Some(ref state) = state_name {
            rule.state = Some(state.clone());
        }
        rules.push(rule);
    }
    
    Ok(rules)
}

fn parse_rule(rule_str: &str) -> Result<Rule, String> {
    // 基本構文: [in state "state_name"] match <pattern> [timeout <sec>] then send <text> [goto "next_state"]
    // 正規表現: [in state "state_name"] match /pattern/flags then send <text> [goto "next_state"]
    
    let rule_str = rule_str.trim();
    let mut rest = rule_str;
    
    // "in state" をチェック
    let mut state: Option<String> = None;
    if rest.starts_with("in state ") {
        let state_rest = &rest[9..].trim();
        if state_rest.starts_with('"') {
            let end = state_rest[1..].find('"')
                .ok_or("Unclosed state name")?;
            state = Some(state_rest[1..end+1].to_string());
            rest = state_rest[end+2..].trim();
        } else {
            return Err("State name must be quoted".to_string());
        }
    }
    
    // "match" を探す
    if !rest.starts_with("match ") {
        return Err("Rule must start with 'match'".to_string());
    }
    
    rest = &rest[6..]; // "match " をスキップ
    
    // パターンを抽出
    let (pattern, rest_after_pattern) = if rest.starts_with('"') {
        // 文字列パターン
        let end = rest[1..].find('"')
            .ok_or("Unclosed string pattern")?;
        let pattern_str = rest[1..end+1].to_string();
        let rest_after = rest[end+2..].trim();
        (Pattern::String(pattern_str), rest_after)
    } else if rest.starts_with('/') {
        // 正規表現パターン
        let end = rest[1..].find('/')
            .ok_or("Unclosed regex pattern")?;
        let pattern_str = &rest[1..end+1];
        let flags = if end + 2 < rest.len() && rest.as_bytes()[end+2] != b' ' {
            // フラグがある
            let flag_end = rest[end+2..].find(|c: char| c.is_whitespace() || c == 't')
                .unwrap_or(rest.len() - end - 2);
            &rest[end+2..end+2+flag_end]
        } else {
            ""
        };
        
        let mut regex_str = String::from("(?");
        if flags.contains('i') {
            regex_str.push('i');
        }
        if flags.contains('m') {
            regex_str.push('m');
        }
        regex_str.push(')');
        regex_str.push_str(pattern_str);
        
        let re = Regex::new(&regex_str)
            .map_err(|e| format!("Invalid regex: {}", e))?;
        let rest_after = rest[end+2+flags.len()..].trim();
        (Pattern::Regex(re), rest_after)
    } else {
        return Err("Pattern must be a string or regex".to_string());
    };
    
    let mut rest = rest_after_pattern;
    
    // タイムアウトをチェック
    let mut timeout = None;
    if rest.starts_with("timeout ") {
        let timeout_end = rest[8..].find(|c: char| c.is_whitespace())
            .unwrap_or(rest.len() - 8);
        if let Ok(secs) = rest[8..8+timeout_end].parse::<u64>() {
            timeout = Some(secs);
            rest = rest[8+timeout_end..].trim();
        }
    }
    
    // "then send" または "then goto" を探す
    let mut response = String::new();
    let mut next_state = None;
    if rest.starts_with("then send ") {
        rest = &rest[10..]; // "then send " をスキップ
        
        // レスポンスを抽出
        let (response_str, rest_after_response) = if rest.starts_with('"') {
            // クォートされた文字列（エスケープシーケンスを処理）
            let mut result = String::new();
            let mut i = 1; // 最初の"をスキップ
            let mut found_end = false;
            while i < rest.len() {
                if rest.as_bytes()[i] == b'\\' && i + 1 < rest.len() {
                    // エスケープシーケンス
                    i += 1;
                    match rest.as_bytes()[i] as char {
                        'n' => { result.push('\n'); i += 1; }
                        'r' => { result.push('\r'); i += 1; }
                        't' => { result.push('\t'); i += 1; }
                        '\\' => { result.push('\\'); i += 1; }
                        '"' => { result.push('"'); i += 1; }
                        c => { result.push('\\'); result.push(c); i += 1; }
                    }
                } else if rest.as_bytes()[i] == b'"' {
                    // 終了
                    found_end = true;
                    i += 1;
                    break;
                } else {
                    result.push(rest.as_bytes()[i] as char);
                    i += 1;
                }
            }
            let rest_after = if found_end { rest[i..].trim() } else { "" };
            (result, rest_after)
        } else {
            // クォートなし（空白または"goto"まで、エスケープシーケンスを処理）
            let mut result = String::new();
            let mut i = 0;
            while i < rest.len() {
                // "goto" が来たら終了
                if i + 4 < rest.len() && &rest[i..i+5] == " goto" {
                    break;
                }
                if rest.as_bytes()[i] == b'\\' && i + 1 < rest.len() {
                    // エスケープシーケンス
                    i += 1;
                    match rest.as_bytes()[i] as char {
                        'n' => { result.push('\n'); i += 1; }
                        'r' => { result.push('\r'); i += 1; }
                        't' => { result.push('\t'); i += 1; }
                        '\\' => { result.push('\\'); i += 1; }
                        c => { result.push('\\'); result.push(c); i += 1; }
                    }
                } else {
                    result.push(rest.as_bytes()[i] as char);
                    i += 1;
                }
            }
            (result, rest[i..].trim())
        };
        
        response = response_str;
        rest = rest_after_response;
    } else if rest.starts_with("then goto ") {
        // "then goto" だけの場合（sendなし）
        let goto_rest = &rest[10..].trim(); // "then goto " をスキップ
        if goto_rest.starts_with('"') {
            let end = goto_rest[1..].find('"')
                .ok_or("Unclosed next state name")?;
            next_state = Some(goto_rest[1..end+1].to_string());
            rest = goto_rest[end+2..].trim();
        } else {
            return Err("Next state name must be quoted".to_string());
        }
        // レスポンスは空文字列のまま
    } else {
        return Err("Rule must contain 'then send' or 'then goto'".to_string());
    }
    
    // "goto" をチェック（"then send" の後に "goto" がある場合）
    if rest.starts_with("goto ") {
        let goto_rest = &rest[5..].trim();
        if goto_rest.starts_with('"') {
            let end = goto_rest[1..].find('"')
                .ok_or("Unclosed next state name")?;
            next_state = Some(goto_rest[1..end+1].to_string());
        } else {
            return Err("Next state name must be quoted".to_string());
        }
    }
    
    Ok(Rule {
        state,
        pattern,
        response,
        timeout,
        next_state,
    })
}

