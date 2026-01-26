// ターミナルバッファエミュレーション

// カーソル位置管理
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

// ターミナルバッファ
pub struct TerminalBuffer {
    lines: Vec<String>,
    cursor: Cursor,
}

impl TerminalBuffer {
    pub fn new() -> Self {
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
        
        // カーソル位置が行の文字数を超えている場合は空白で埋める
        let mut char_count = line.chars().count();
        while char_count < col {
            line.push(' ');
            char_count += 1;
        }
        
        // カーソル位置に文字を挿入（既存の文字を上書き）
        if col < char_count {
            // 文字位置をバイト位置に変換
            let byte_pos = line.char_indices()
                .nth(col)
                .map(|(pos, _)| pos)
                .unwrap_or(line.len());
            // 次の文字のバイト位置を取得
            let next_byte_pos = line.char_indices()
                .nth(col + 1)
                .map(|(pos, _)| pos)
                .unwrap_or(line.len());
            line.replace_range(byte_pos..next_byte_pos, &ch.to_string());
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
        
        // OSCシーケンス: \x1B]...\x07 または \x1B]...\x1B\\
        if i < data.len() && data[i] == b']' {
            i += 1;
            while i < data.len() {
                if data[i] == 0x07 {
                    // BEL終端
                    i += 1;
                    return i;
                } else if data[i] == 0x1B && i + 1 < data.len() && data[i + 1] == b'\\' {
                    // ST終端 (\x1B\\)
                    i += 2;
                    return i;
                }
                i += 1;
            }
            // 終端が見つからない場合は終端までスキップ
            return i;
        }
        
        // CSIシーケンス: \x1B[...]
        if i >= data.len() || data[i] != b'[' {
            return i;
        }
        
        i += 1; // [をスキップ
        
        // プライベートパラメータ（?）をチェック
        let has_private_param = i < data.len() && data[i] == b'?';
        if has_private_param {
            i += 1; // ?をスキップ
        }
        
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
        
        // 終端文字を取得
        if i >= data.len() {
            return i;
        }
        
        let terminator = data[i] as char;
        i += 1;
        
        match terminator {
            'D' => {
                // カーソル左移動（デフォルト値: 1）
                let steps = if params.is_empty() { 1 } else { params[0] };
                self.cursor.move_left(steps);
            }
            'C' => {
                // カーソル右移動（デフォルト値: 1）
                let steps = if params.is_empty() { 1 } else { params[0] };
                self.cursor.move_right(steps);
            }
            'A' => {
                // カーソル上移動（デフォルト値: 1）
                let steps = if params.is_empty() { 1 } else { params[0] };
                self.cursor.move_up(steps);
                // 行の長さに合わせて列位置を調整
                let line_len = {
                    let line = self.get_line(self.cursor.row);
                    line.chars().count()
                };
                if self.cursor.col > line_len {
                    self.cursor.col = line_len;
                }
            }
            'B' => {
                // カーソル下移動（デフォルト値: 1）
                let steps = if params.is_empty() { 1 } else { params[0] };
                self.cursor.move_down(steps);
                // 行の長さに合わせて列位置を調整
                let line_len = {
                    let line = self.get_line(self.cursor.row);
                    line.chars().count()
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
                        let char_count = line.chars().count();
                        if cursor_col < char_count {
                            // 文字位置をバイト位置に変換
                            let byte_pos = line.char_indices()
                                .nth(cursor_col)
                                .map(|(pos, _)| pos)
                                .unwrap_or(line.len());
                            line.truncate(byte_pos);
                        }
                    }
                    1 => {
                        // 行の先頭からカーソル位置まで消去
                        let keep = {
                            let line = self.get_line(self.cursor.row);
                            let char_count = line.chars().count();
                            if cursor_col < char_count {
                                // 文字位置をバイト位置に変換
                                let byte_pos = line.char_indices()
                                    .nth(cursor_col)
                                    .map(|(pos, _)| pos)
                                    .unwrap_or(line.len());
                                line[byte_pos..].to_string()
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
                        let char_count = line.chars().count();
                        if cursor_col < char_count {
                            // 文字位置をバイト位置に変換
                            let byte_pos = line.char_indices()
                                .nth(cursor_col)
                                .map(|(pos, _)| pos)
                                .unwrap_or(line.len());
                            line.truncate(byte_pos);
                        }
                        self.lines.truncate(cursor_row + 1);
                    }
                    1 => {
                        // 画面の先頭からカーソル位置まで消去
                        let keep = {
                            let line = self.get_line(cursor_row);
                            let char_count = line.chars().count();
                            if cursor_col < char_count {
                                // 文字位置をバイト位置に変換
                                let byte_pos = line.char_indices()
                                    .nth(cursor_col)
                                    .map(|(pos, _)| pos)
                                    .unwrap_or(line.len());
                                line[byte_pos..].to_string()
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
    
    pub fn process_data(&mut self, data: &[u8]) {
        // UTF-8文字列として解析
        let s = match std::str::from_utf8(data) {
            Ok(s) => s,
            Err(_) => {
                // UTF-8として無効な場合はバイト単位で処理（フォールバック）
                let mut i = 0;
                while i < data.len() {
                    if data[i] == 0x1B {
                        let new_i = self.process_ansi_escape(data, i);
                        i = if new_i > i { new_i } else { i + 1 };
                    } else if data[i] == 0x08 {
                        if self.cursor.col > 0 {
                            let col_to_remove = self.cursor.col - 1;
                            {
                                let line = self.get_line(self.cursor.row);
                                let char_count = line.chars().count();
                                if col_to_remove < char_count {
                                    let byte_pos = line.char_indices()
                                        .nth(col_to_remove)
                                        .map(|(pos, _)| pos)
                                        .unwrap_or(line.len());
                                    if let Some((_, ch)) = line.char_indices().nth(col_to_remove) {
                                        let ch_len = ch.len_utf8();
                                        line.drain(byte_pos..byte_pos + ch_len);
                                    }
                                }
                            }
                            self.cursor.move_left(1);
                        }
                        i += 1;
                    } else if data[i] == b'\r' {
                        self.cursor.col = 0;
                        i += 1;
                    } else if data[i] == b'\n' {
                        self.cursor.row += 1;
                        self.cursor.col = 0;
                        self.ensure_line(self.cursor.row);
                        i += 1;
                    } else if data[i] == 0x07 || data[i] == 0x00 {
                        i += 1;
                    } else {
                        i += 1;
                    }
                }
                return;
            }
        };
        
        // 文字列として処理（UTF-8マルチバイト文字を正しく処理）
        let s_bytes = s.as_bytes();
        let mut byte_pos = 0;
        while byte_pos < s.len() {
            // エスケープシーケンスの検出（バイトレベル）
            if s_bytes[byte_pos] == 0x1B {
                // ANSIエスケープシーケンス
                let new_pos = self.process_ansi_escape(s_bytes, byte_pos);
                if new_pos > byte_pos {
                    byte_pos = new_pos;
                    continue;
                }
            }
            
            // 文字を取得
            let remaining = &s[byte_pos..];
            if let Some(ch) = remaining.chars().next() {
                let ch_len = ch.len_utf8();
                
                if ch == '\x08' {
                    // バックスペース
                    if self.cursor.col > 0 {
                        let col_to_remove = self.cursor.col - 1;
                        {
                            let line = self.get_line(self.cursor.row);
                            let char_count = line.chars().count();
                            if col_to_remove < char_count {
                                let line_byte_pos = line.char_indices()
                                    .nth(col_to_remove)
                                    .map(|(pos, _)| pos)
                                    .unwrap_or(line.len());
                                if let Some((_, ch)) = line.char_indices().nth(col_to_remove) {
                                    let ch_len = ch.len_utf8();
                                    line.drain(line_byte_pos..line_byte_pos + ch_len);
                                }
                            }
                        }
                        self.cursor.move_left(1);
                    }
                } else if ch == '\r' {
                    // キャリッジリターン
                    self.cursor.col = 0;
                } else if ch == '\n' {
                    // ラインフィード
                    self.cursor.row += 1;
                    self.cursor.col = 0;
                    self.ensure_line(self.cursor.row);
                } else if ch == '\x07' || ch == '\x00' {
                    // ベル文字、ヌル文字は無視
                } else if ch.is_control() {
                    // その他の制御文字は無視
                } else {
                    // 通常の文字
                    self.insert_char(ch);
                }
                
                byte_pos += ch_len;
            } else {
                break;
            }
        }
    }
    
    pub fn output(&self) -> String {
        self.lines.join("\n")
    }
    
    pub fn clear(&mut self) {
        self.lines.clear();
        self.lines.push(String::new());
        self.cursor.set_position(0, 0);
    }
}

