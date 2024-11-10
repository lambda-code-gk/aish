import re
import sys
import io

class Cursor:
    def __init__(self):
        self._row = 0
        self._col = 0
        self.saved_row = 0
        self.saved_col = 0

    @property
    def row(self):
        return self._row

    @property
    def col(self):
        return self._col

    def save_position(self):
        self.saved_row = self._row
        self.saved_col = self._col

    def restore_position(self):
        self._row = self.saved_row
        self._col = self.saved_col

    def move_left(self, steps=1):
        self._col = max(0, self._col - steps)

    def move_right(self, steps=1, max_col=None):
        if max_col is not None:
            self._col = min(max_col, self._col + steps)
        else:
            self._col += steps

    def move_up(self, steps=1):
        self._row = max(0, self._row - steps)

    def move_down(self, steps=1, max_row=None):
        if max_row is not None:
            self._row = min(max_row, self._row + steps)
        else:
            self._row += steps

    def set_position(self, row, col):
        self._row = row
        self._col = col

class Buffer:
    def __init__(self):
        self.lines = ['']
        self.cursor = Cursor()

    def get_line(self, row):
        while len(self.lines) <= row:
            self.lines.append('')
        return self.lines[row]

    def set_line(self, row, content):
        while len(self.lines) <= row:
            self.lines.append('')
        self.lines[row] = content

    def delete_to_end_of_line(self):
        line = self.get_line(self.cursor.row)
        self.set_line(self.cursor.row, line[:self.cursor.col])

    def delete_from_start_of_line(self):
        line = self.get_line(self.cursor.row)
        self.set_line(self.cursor.row, line[self.cursor.col:])

    def delete_line(self):
        self.set_line(self.cursor.row, '')
        self.cursor.set_position(self.cursor.row, 0)

    def delete_to_end_of_screen(self):
        self.lines = self.lines[:self.cursor.row + 1]
        self.delete_to_end_of_line()

    def delete_from_start_of_screen(self):
        self.lines = [''] * (self.cursor.row + 1)
        self.delete_from_start_of_line()

    def delete_screen(self):
        self.lines = ['']
        self.cursor.set_position(0, 0)

    # カーソル操作の移譲関数
    def move_cursor_left(self, steps=1):
        self.cursor.move_left(steps)

    def move_cursor_right(self, steps=1):
        line = self.get_line(self.cursor.row)
        max_col = len(line)
        self.cursor.move_right(steps, max_col=max_col)

    def move_cursor_up(self, steps=1):
        self.cursor.move_up(steps)
        line = self.get_line(self.cursor.row)
        max_col = len(line)
        self.cursor.move_right(0, max_col=max_col)

    def move_cursor_down(self, steps=1):
        self.cursor.move_down(steps, max_row=len(self.lines) - 1)
        line = self.get_line(self.cursor.row)
        max_col = len(line)
        self.cursor.move_right(0, max_col=max_col)

    def save_cursor_position(self):
        self.cursor.save_position()

    def restore_cursor_position(self):
        self.cursor.restore_position()

    def set_cursor_position(self, row, col):
        self.cursor.set_position(row, col)
        while len(self.lines) <= self.cursor.row:
            self.lines.append('')
        line = self.get_line(self.cursor.row)
        self.cursor.move_right(0, max_col=len(line))

def clean_script_log(input_stream=sys.stdin, output_stream=sys.stdout):
    """
    scriptコマンドのログからエスケープシーケンスを除去し、
    実際の表示テキストを取得する
    
    Args:
        input_stream: 入力ストリーム（デフォルト: sys.stdin）
        output_stream: 出力ストリーム（デフォルト: sys.stdout）
    """
    
    try:
        # エスケープシーケンスを除去するための正規表現パターン
        ansi_escape = re.compile(r'\x1B[@-_][0-?]*[ -/]*[@-~]')
        
        # バッファの初期化
        buffer = Buffer()

        # 入力を1行ずつ処理
        for line in input_stream:
            i = 0
            while i < len(line):
                if line[i] == '\x1B':
                    # エスケープシーケンスの処理
                    if i + 1 < len(line) and line[i + 1] == ']':
                        # OSCシーケンスの処理
                        i += 2
                        while i < len(line) and line[i] != '\x07':
                            i += 1
                        if i < len(line) and line[i] == '\x07':
                            i += 1
                    else:
                        match = ansi_escape.match(line, i)
                        if match:
                            seq = match.group()
                            steps = int(seq[2:-1]) if seq[2:-1].isdigit() else 1
                            if seq.endswith('D'):  # カーソル左移動
                                buffer.move_cursor_left(steps)
                            elif seq.endswith('C'):  # カーソル右移動
                                buffer.move_cursor_right(steps)
                            elif seq.endswith('A'):  # カーソル上移動
                                buffer.move_cursor_up(steps)
                            elif seq.endswith('B'):  # カーソル下移動
                                buffer.move_cursor_down(steps)
                            elif seq.endswith('s'):  # カーソル位置の保存
                                buffer.save_cursor_position()
                            elif seq.endswith('u'):  # 保存したカーソル位置の復元
                                buffer.restore_cursor_position()
                            elif seq.endswith('K'):  # 行の消去
                                if seq == '\x1B[K':  # カーソル位置から行末まで消去
                                    buffer.delete_to_end_of_line()
                                elif seq == '\x1B[1K':  # 行の先頭からカーソル位置まで消去
                                    buffer.delete_from_start_of_line()
                                elif seq == '\x1B[2K':  # 行全体を消去
                                    buffer.delete_line()
                            elif seq.endswith('J'):  # 画面の消去
                                if seq == '\x1B[J':  # カーソル位置から画面末尾まで消去
                                    buffer.delete_to_end_of_screen()
                                elif seq == '\x1B[1J':  # 画面の先頭からカーソル位置まで消去
                                    buffer.delete_from_start_of_screen()
                                elif seq == '\x1B[2J':  # 画面全体を消去
                                    buffer.delete_screen()
                            elif 'H' in seq:  # カーソル位置の設定
                                parts = seq[2:-1].split(';')
                                if len(parts) == 2 and parts[0].isdigit() and parts[1].isdigit():
                                    buffer.set_cursor_position(int(parts[0]) - 1, int(parts[1]) - 1)
                            i = match.end()
                        else:
                            i += 1
                elif line[i] == '\x08':  # バックスペースの処理
                    if buffer.cursor.col > 0:
                        buffer.move_cursor_left()
                        line_content = buffer.get_line(buffer.cursor.row)
                        buffer.set_line(buffer.cursor.row, line_content[:buffer.cursor.col] + line_content[buffer.cursor.col + 1:])
                    i += 1
                elif line[i] == '\r':  # キャリッジリターンの処理
                    i += 1  # 無視して次の文字へ
                elif line[i] == '\n':  # ラインフィードの処理
                    buffer.set_cursor_position(buffer.cursor.row + 1, 0)
                    if buffer.cursor.row >= len(buffer.lines):
                        buffer.lines.append('')
                    i += 1
                elif line[i] == '\x07':  # ベル文字の処理
                    i += 1  # 無視して次の文字へ
                elif line[i] == '\x00':  # ヌル文字の処理
                    i += 1  # 無視して次の文字へ
                else:
                    line_content = buffer.get_line(buffer.cursor.row)
                    if buffer.cursor.col < len(line_content):
                        buffer.set_line(buffer.cursor.row, line_content[:buffer.cursor.col] + line[i] + line_content[buffer.cursor.col + 1:])
                    else:
                        buffer.set_line(buffer.cursor.row, line_content + line[i])
                    buffer.move_cursor_right()
                    i += 1
        
        # 出力バッファをまとめて出力
        output_stream.write('\n'.join(buffer.lines) + '\n')
        
    except (IOError, OSError) as e:
        print(f"I/Oエラーが発生しました: {str(e)}", file=sys.stderr)
        sys.exit(1)
    except Exception as e:
        print(f"エラーが発生しました: {str(e)}", file=sys.stderr)
        sys.exit(1)

if __name__ == '__main__':
    # コマンドライン引数がある場合はファイルとして処理
    if len(sys.argv) > 1:
        with open(sys.argv[1], 'r', encoding='utf-8') as f:
            clean_script_log(input_stream=f)
    else:
        clean_script_log(input_stream=io.TextIOWrapper(sys.stdin.buffer, encoding='utf-8'),
                         output_stream=io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8'))
