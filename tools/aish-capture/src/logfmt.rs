use crate::util::base64;

fn json_escape(s: &str) -> String {
    let mut result = String::new();
    for ch in s.chars() {
        match ch {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            c if c.is_control() => {
                result.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => result.push(c),
        }
    }
    result
}

// ANSIエスケープシーケンスを含むかどうかを判定
// ANSIエスケープシーケンスはJSONエスケープで表現可能なため、JSON-safeとして扱う
fn contains_ansi_escape_sequences(data: &[u8]) -> bool {
    let mut i = 0;
    while i < data.len() {
        if data[i] == 0x1B {
            i += 1;
            if i >= data.len() {
                return true; // ESCで終わる場合はANSIエスケープシーケンスの可能性がある
            }
            
            // OSCシーケンス: \x1B]...\x07
            if data[i] == b']' {
                i += 1;
                while i < data.len() && data[i] != 0x07 {
                    i += 1;
                }
                if i < data.len() {
                    i += 1; // \x07をスキップ
                    continue; // OSCシーケンスを検出
                }
                return true; // \x07が見つからないが、OSCシーケンスの可能性がある
            }
            
            // CSIシーケンス: \x1B[...]
            if data[i] == b'[' {
                i += 1;
                // パラメータと終端文字をスキップ
                while i < data.len() {
                    let ch = data[i];
                    // 終端文字（アルファベット、@、?など）
                    if (ch >= b'@' && ch <= b'~') || ch == b'?' {
                        return true; // CSIシーケンスを検出
                    }
                    i += 1;
                }
                return true; // [で始まるが終端が見つからない場合もCSIシーケンスの可能性がある
            }
            
            // その他のエスケープシーケンス: \x1B...（2文字目がアルファベットなど）
            if (data[i] >= b'@' && data[i] <= b'~') || data[i] < 0x20 {
                return true; // エスケープシーケンスを検出
            }
        }
        i += 1;
    }
    false
}

// データがJSON-safeなテキストかどうかを判定し、文字列として変換可能かも返す
// JSON-safeとは、UTF-8として有効で、JSON文字列として安全に埋め込めること
// ANSIエスケープシーケンスを含むデータもJSON-safeとして扱う（JSONエスケープで表現可能）
// 返り値: (is_safe, Option<&str>)
fn check_json_safe_text(data: &[u8]) -> (bool, Option<&str>) {
    // UTF-8として有効な文字列かチェック
    match std::str::from_utf8(data) {
        Ok(s) => {
            // ANSIエスケープシーケンスを含む場合はJSON-safeとして扱う
            if contains_ansi_escape_sequences(data) {
                return (true, Some(s));
            }
            
            // 制御文字が含まれていないかチェック
            // JSONエスケープで処理可能な範囲（\n, \r, \t）以外の制御文字がある場合はbase64が必要
            // 注意: ANSIエスケープシーケンス（\x1Bで始まる）は上で既にチェック済み
            let is_safe = s.chars().all(|c| {
                !c.is_control() || c == '\n' || c == '\r' || c == '\t'
            });
            if is_safe {
                (true, Some(s))
            } else {
                (false, None)
            }
        }
        Err(_) => (false, None), // UTF-8として無効な場合はbase64が必要
    }
}

pub fn write_start(
    out: &mut dyn std::io::Write,
    cols: u16,
    rows: u16,
    argv: &[String],
    cwd: &str,
    pid: libc::pid_t,
) -> std::io::Result<()> {
    let argv_json = argv
        .iter()
        .map(|s| format!("\"{}\"", json_escape(s)))
        .collect::<Vec<_>>()
        .join(",");
    
    writeln!(
        out,
        r#"{{"v":1,"t_ms":{},"type":"start","cols":{},"rows":{},"argv":[{}],"cwd":"{}","pid":{}}}"#,
        now_ms(),
        cols,
        rows,
        argv_json,
        json_escape(cwd),
        pid
    )
}

pub fn write_stdin(out: &mut dyn std::io::Write, data: &[u8]) -> std::io::Result<()> {
    let (_is_safe, text_opt) = check_json_safe_text(data);
    if let Some(text) = text_opt {
        // JSON-safeなテキストの場合：直接文字列として保存（encフィールドなし）
        writeln!(
            out,
            r#"{{"v":1,"t_ms":{},"type":"stdin","n":{},"data":"{}"}}"#,
            now_ms(),
            data.len(),
            json_escape(text)
        )
    } else {
        // バイナリデータや制御文字を含む場合：base64エンコード
        let encoded = base64::encode(data);
        writeln!(
            out,
            r#"{{"v":1,"t_ms":{},"type":"stdin","enc":"b64","n":{},"data":"{}"}}"#,
            now_ms(),
            data.len(),
            json_escape(&encoded)
        )
    }
}

pub fn write_stdout(out: &mut dyn std::io::Write, data: &[u8]) -> std::io::Result<()> {
    let (_is_safe, text_opt) = check_json_safe_text(data);
    if let Some(text) = text_opt {
        // JSON-safeなテキストの場合：直接文字列として保存（encフィールドなし）
        writeln!(
            out,
            r#"{{"v":1,"t_ms":{},"type":"stdout","n":{},"data":"{}"}}"#,
            now_ms(),
            data.len(),
            json_escape(text)
        )
    } else {
        // バイナリデータや制御文字を含む場合：base64エンコード
        let encoded = base64::encode(data);
        writeln!(
            out,
            r#"{{"v":1,"t_ms":{},"type":"stdout","enc":"b64","n":{},"data":"{}"}}"#,
            now_ms(),
            data.len(),
            json_escape(&encoded)
        )
    }
}

pub fn write_resize(out: &mut dyn std::io::Write, cols: u16, rows: u16) -> std::io::Result<()> {
    writeln!(
        out,
        r#"{{"v":1,"t_ms":{},"type":"resize","cols":{},"rows":{}}}"#,
        now_ms(),
        cols,
        rows
    )
}

pub fn write_exit_code(out: &mut dyn std::io::Write, code: i32) -> std::io::Result<()> {
    writeln!(
        out,
        r#"{{"v":1,"t_ms":{},"type":"exit","how":"code","code":{}}}"#,
        now_ms(),
        code
    )
}

pub fn write_exit_signal(out: &mut dyn std::io::Write, signal: i32) -> std::io::Result<()> {
    writeln!(
        out,
        r#"{{"v":1,"t_ms":{},"type":"exit","how":"signal","signal":{}}}"#,
        now_ms(),
        signal
    )
}

fn now_ms() -> i64 {
    use libc::{clock_gettime, CLOCK_REALTIME, timespec};
    unsafe {
        let mut ts: timespec = std::mem::zeroed();
        if clock_gettime(CLOCK_REALTIME, &mut ts) == 0 {
            ts.tv_sec * 1000 + ts.tv_nsec / 1_000_000
        } else {
            0
        }
    }
}

// UTF-8テキスト用のバッファ（改行までまとめる）
pub struct TextBuffer {
    buf: Vec<u8>,
    is_text: bool,
}

impl TextBuffer {
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            is_text: true,
        }
    }

    // データを追加し、改行まで揃った場合はSome(データ)を返す
    // 返り値: 書き出すべきデータのベクタ（複数の行が含まれる可能性がある）
    pub fn append(&mut self, data: &[u8]) -> Vec<Vec<u8>> {
        let mut result = Vec::new();
        
        // 最初のデータでテキストかどうかを判定（バッファが空の場合のみ）
        if self.buf.is_empty() {
            let (_is_safe, text_opt) = check_json_safe_text(data);
            self.is_text = text_opt.is_some();
        }

        // バイナリデータの場合は即座に書き出す
        if !self.is_text {
            result.push(data.to_vec());
            return result;
        }

        // テキストデータの場合は改行までバッファリング
        self.buf.extend_from_slice(data);
        
        // 複数の改行がある場合も処理
        while let Some(newline_pos) = self.buf.iter().position(|&b| b == b'\n') {
            let line_end = if newline_pos > 0 && self.buf[newline_pos - 1] == b'\r' {
                newline_pos + 1
            } else {
                newline_pos + 1
            };
            let line = self.buf[..line_end].to_vec();
            self.buf.drain(..line_end);
            result.push(line);
        }
        
        result
    }

    // 残っているデータを書き出す（EOF時など）
    pub fn flush(&mut self) -> Option<Vec<u8>> {
        if self.buf.is_empty() {
            None
        } else {
            let data = std::mem::take(&mut self.buf);
            Some(data)
        }
    }
}

