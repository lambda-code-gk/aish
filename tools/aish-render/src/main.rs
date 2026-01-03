use std::io::{self, BufRead, Write};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::Value;

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

// ターミナルバッファエミュレーション
struct Cursor {
    row: usize,
    col: usize,
    saved_row: usize,
    saved_col: usize,
}

impl Cursor {
    fn new() -> Self {
        Self {
            row: 0,
            col: 0,
            saved_row: 0,
            saved_col: 0,
        }
    }
    
    fn save_position(&mut self) {
        self.saved_row = self.row;
        self.saved_col = self.col;
    }
    
    fn restore_position(&mut self) {
        self.row = self.saved_row;
        self.col = self.saved_col;
    }
    
    fn move_left(&mut self, steps: usize) {
        if steps > self.col {
            self.col = 0;
        } else {
            self.col -= steps;
        }
    }
    
    fn move_right(&mut self, steps: usize) {
        self.col += steps;
    }
    
    fn move_up(&mut self, steps: usize) {
        if steps > self.row {
            self.row = 0;
        } else {
            self.row -= steps;
        }
    }
    
    fn move_down(&mut self, steps: usize) {
        self.row += steps;
    }
    
    fn set_position(&mut self, row: usize, col: usize) {
        self.row = row;
        self.col = col;
    }
}

struct TerminalBuffer {
    lines: Vec<String>,
    cursor: Cursor,
}

impl TerminalBuffer {
    fn new() -> Self {
        Self {
            lines: vec![String::new()],
            cursor: Cursor::new(),
        }
    }
    
    fn ensure_line(&mut self, row: usize) {
        while self.lines.len() <= row {
            self.lines.push(String::new());
        }
    }
    
    fn get_line(&mut self, row: usize) -> &mut String {
        self.ensure_line(row);
        &mut self.lines[row]
    }
    
    fn insert_char(&mut self, ch: char) {
        let row = self.cursor.row;
        let col = self.cursor.col;
        let line = self.get_line(row);
        
        // カーソル位置が行の長さを超えている場合は空白で埋める
        while line.len() < col {
            line.push(' ');
        }
        
        // カーソル位置に文字を挿入（既存の文字を上書き）
        if col < line.len() {
            line.replace_range(col..col + 1, &ch.to_string());
        } else {
            line.push(ch);
        }
        
        self.cursor.move_right(1);
    }
    
    fn process_ansi_escape(&mut self, data: &[u8], mut i: usize) -> usize {
        if i >= data.len() || data[i] != 0x1B {
            return i;
        }
        
        i += 1; // ESCをスキップ
        
        // OSCシーケンス: \x1B]...\x07
        if i < data.len() && data[i] == b']' {
            i += 1;
            while i < data.len() && data[i] != 0x07 {
                i += 1;
            }
            if i < data.len() {
                i += 1; // \x07をスキップ
            }
            return i;
        }
        
        // CSIシーケンス: \x1B[...]
        if i >= data.len() || data[i] != b'[' {
            return i;
        }
        
        i += 1; // [をスキップ
        
        // パラメータを読み取る
        let mut params = Vec::new();
        let mut current_param = String::new();
        
        while i < data.len() {
            let ch = data[i] as char;
            if ch.is_ascii_digit() {
                current_param.push(ch);
            } else if ch == ';' {
                if current_param.is_empty() {
                    params.push(0);
                } else {
                    params.push(current_param.parse().unwrap_or(0));
                    current_param.clear();
                }
            } else {
                break;
            }
            i += 1;
        }
        
        if !current_param.is_empty() {
            params.push(current_param.parse().unwrap_or(0));
        }
        
        // デフォルト値が1の場合
        if params.is_empty() {
            params.push(1);
        }
        
        // 終端文字を取得
        if i >= data.len() {
            return i;
        }
        
        let terminator = data[i] as char;
        i += 1;
        
        match terminator {
            'D' => {
                // カーソル左移動
                let steps = params[0];
                self.cursor.move_left(steps);
            }
            'C' => {
                // カーソル右移動
                let steps = params[0];
                self.cursor.move_right(steps);
            }
            'A' => {
                // カーソル上移動
                let steps = params[0];
                self.cursor.move_up(steps);
                // 行の長さに合わせて列位置を調整
                let line_len = {
                    let line = self.get_line(self.cursor.row);
                    line.len()
                };
                if self.cursor.col > line_len {
                    self.cursor.col = line_len;
                }
            }
            'B' => {
                // カーソル下移動
                let steps = params[0];
                self.cursor.move_down(steps);
                // 行の長さに合わせて列位置を調整
                let line_len = {
                    let line = self.get_line(self.cursor.row);
                    line.len()
                };
                if self.cursor.col > line_len {
                    self.cursor.col = line_len;
                }
            }
            's' => {
                // カーソル位置の保存
                self.cursor.save_position();
            }
            'u' => {
                // カーソル位置の復元
                self.cursor.restore_position();
            }
            'K' => {
                // 行の消去
                let param = if params.is_empty() { 0 } else { params[0] };
                let cursor_col = self.cursor.col;
                match param {
                    0 => {
                        // カーソル位置から行末まで消去
                        let line = self.get_line(self.cursor.row);
                        if cursor_col < line.len() {
                            line.truncate(cursor_col);
                        }
                    }
                    1 => {
                        // 行の先頭からカーソル位置まで消去
                        let keep = {
                            let line = self.get_line(self.cursor.row);
                            if cursor_col < line.len() {
                                line[cursor_col..].to_string()
                            } else {
                                String::new()
                            }
                        };
                        let line = self.get_line(self.cursor.row);
                        *line = keep;
                        self.cursor.col = 0;
                    }
                    2 => {
                        // 行全体を消去
                        let line = self.get_line(self.cursor.row);
                        line.clear();
                        self.cursor.col = 0;
                    }
                    _ => {}
                }
            }
            'J' => {
                // 画面の消去
                let param = if params.is_empty() { 0 } else { params[0] };
                let cursor_row = self.cursor.row;
                let cursor_col = self.cursor.col;
                match param {
                    0 => {
                        // カーソル位置から画面末尾まで消去
                        let line = self.get_line(cursor_row);
                        if cursor_col < line.len() {
                            line.truncate(cursor_col);
                        }
                        self.lines.truncate(cursor_row + 1);
                    }
                    1 => {
                        // 画面の先頭からカーソル位置まで消去
                        let keep = {
                            let line = self.get_line(cursor_row);
                            if cursor_col < line.len() {
                                line[cursor_col..].to_string()
                            } else {
                                String::new()
                            }
                        };
                        let line = self.get_line(cursor_row);
                        *line = keep;
                        self.lines.drain(0..cursor_row);
                        self.cursor.row = 0;
                    }
                    2 => {
                        // 画面全体を消去
                        self.lines.clear();
                        self.lines.push(String::new());
                        self.cursor.set_position(0, 0);
                    }
                    _ => {}
                }
            }
            'H' => {
                // カーソル位置の設定
                // \x1B[H (パラメータなし) は \x1B[1;1H と同等
                let row = if params.is_empty() || params[0] == 0 {
                    0 // デフォルトは1（0ベースでは0）
                } else {
                    params[0] - 1 // 1ベースから0ベースに変換
                };
                let col = if params.len() >= 2 {
                    if params[1] == 0 {
                        0 // デフォルトは1（0ベースでは0）
                    } else {
                        params[1] - 1 // 1ベースから0ベースに変換
                    }
                } else if params.len() == 1 {
                    self.cursor.col // 列が指定されていない場合は現在の列を維持
                } else {
                    0 // パラメータなしの場合は(0,0)
                };
                self.cursor.set_position(row, col);
            }
            _ => {
                // その他のエスケープシーケンスは無視
            }
        }
        
        i
    }
    
    fn process_data(&mut self, data: &[u8]) {
        let mut i = 0;
        
        while i < data.len() {
            let ch = data[i] as char;
            
            if ch == '\x1B' {
                // ANSIエスケープシーケンス
                let new_i = self.process_ansi_escape(data, i);
                if new_i == i {
                    // 処理されなかった場合は1文字進む
                    i += 1;
                } else {
                    i = new_i;
                }
            } else if ch == '\x08' {
                // バックスペース
                if self.cursor.col > 0 {
                    let col_to_remove = self.cursor.col - 1;
                    {
                        let line = self.get_line(self.cursor.row);
                        if col_to_remove < line.len() {
                            line.remove(col_to_remove);
                        }
                    }
                    self.cursor.move_left(1);
                }
                i += 1;
            } else if ch == '\r' {
                // キャリッジリターン
                self.cursor.col = 0;
                i += 1;
            } else if ch == '\n' {
                // ラインフィード
                self.cursor.row += 1;
                self.cursor.col = 0;
                self.ensure_line(self.cursor.row);
                i += 1;
            } else if ch == '\x07' || ch == '\x00' {
                // ベル文字、ヌル文字は無視
                i += 1;
            } else {
                // 通常の文字
                if ch.is_control() {
                    // その他の制御文字は無視
                    i += 1;
                } else {
                    self.insert_char(ch);
                    i += 1;
                }
            }
        }
    }
    
    fn output(&self) -> String {
        self.lines.join("\n")
    }
}

fn main() {
    let stdin = io::stdin();
    let mut buffer = TerminalBuffer::new();
    
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        
        if let Some((event_type, data_opt, enc_opt)) = parse_jsonl_line(&line) {
            if event_type == "stdout" {
                if let Some(data_str) = data_opt {
                    let data = if enc_opt.as_ref().map(|e| e == "b64").unwrap_or(false) {
                        // base64デコード
                        match STANDARD.decode(&data_str) {
                            Ok(bytes) => bytes,
                            Err(_) => continue,
                        }
                    } else {
                        // JSONパーサーで既にエスケープを処理しているので、そのままバイト列に変換
                        data_str.as_bytes().to_vec()
                    };
                    
                    buffer.process_data(&data);
                }
            }
        }
    }
    
    let output = buffer.output();
    io::stdout().write_all(output.as_bytes()).unwrap();
    io::stdout().write_all(b"\n").unwrap();
}

