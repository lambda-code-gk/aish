// JSONLパーサー（aish-renderの実装を参考）

pub fn parse_line(line: &str) -> Option<(String, Option<String>, Option<String>)> {
    // type, data, enc フィールドを抽出
    let mut event_type: Option<String> = None;
    let mut data: Option<String> = None;
    let mut enc: Option<String> = None;
    
    // 簡易パース: "type":"value" のパターンを探す
    let mut i = 0;
    while i < line.len() {
        // "type"を探す
        if i + 6 < line.len() && &line[i..i+6] == "\"type\"" {
            i += 6;
            // : を探す
            while i < line.len() && (line.as_bytes()[i] == b' ' || line.as_bytes()[i] == b':') {
                i += 1;
            }
            // "value"を抽出
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
        }
        // "data"を探す
        else if i + 6 < line.len() && &line[i..i+6] == "\"data\"" {
            i += 6;
            while i < line.len() && (line.as_bytes()[i] == b' ' || line.as_bytes()[i] == b':') {
                i += 1;
            }
            if i < line.len() && line.as_bytes()[i] == b'"' {
                i += 1;
                let mut data_str = String::new();
                while i < line.len() {
                    if line.as_bytes()[i] == b'\\' && i + 1 < line.len() {
                        // エスケープシーケンスを処理
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
                                // \uXXXX の処理
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
        }
        // "enc"を探す
        else if i + 5 < line.len() && &line[i..i+5] == "\"enc\"" {
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

