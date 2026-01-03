use regex::Regex;

pub struct Rule {
    pub pattern: Pattern,
    pub response: String,
    pub timeout: Option<u64>,
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

pub fn parse_script(script: &str) -> Result<Vec<Rule>, String> {
    let mut rules = Vec::new();
    
    // セミコロンで区切られたルールを解析
    let parts: Vec<&str> = script.split(';').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    
    for part in parts {
        let rule = parse_rule(part)?;
        rules.push(rule);
    }
    
    Ok(rules)
}

fn parse_rule(rule_str: &str) -> Result<Rule, String> {
    // 基本構文: match <pattern> [timeout <sec>] then send <text> [wait <sec>]
    // 正規表現: match /pattern/flags then send <text>
    
    let rule_str = rule_str.trim();
    
    // "match" を探す
    if !rule_str.starts_with("match ") {
        return Err("Rule must start with 'match'".to_string());
    }
    
    let rest = &rule_str[6..]; // "match " をスキップ
    
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
    
    // "then send" を探す
    if !rest.starts_with("then send ") {
        return Err("Rule must contain 'then send'".to_string());
    }
    
    let rest = &rest[10..]; // "then send " をスキップ
    
    // レスポンスを抽出（最後まで）
    let response = if rest.starts_with('"') {
        // クォートされた文字列（エスケープシーケンスを処理）
        let mut result = String::new();
        let mut i = 1; // 最初の"をスキップ
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
                break;
            } else {
                result.push(rest.as_bytes()[i] as char);
                i += 1;
            }
        }
        result
    } else {
        // クォートなし（最後まで、エスケープシーケンスを処理）
        let mut result = String::new();
        let mut i = 0;
        while i < rest.len() {
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
        result
    };
    
    Ok(Rule {
        pattern,
        response,
        timeout,
    })
}

