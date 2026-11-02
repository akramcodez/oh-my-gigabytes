"""Just enough of a terminal to read what omg's interface drew, shared by the Unix PTY and the Windows
ConPTY tests. Not a test module (the discovery pattern is test_*.py)."""
import codecs
import re


class Screen:
    """The interface redraws only changed cells, so words arrive split by cursor moves and the raw
    stream cannot be searched. ConPTY also sends window titles (OSC), erase-character and
    column-position sequences, which are handled here too."""

    ESCAPE = re.compile(
        r"\x1b\[([0-9;?]*)[ -/]*([@-~])"
        r"|\x1b\](?:[^\x07\x1b]|\x1b(?!\\))*(?:\x07|\x1b\\)"
        r"|\x1b[()][0-9A-Z]|\x1b[=>78]"
    )
    PARTIAL = re.compile(r"\x1b\](?:[^\x07\x1b]|\x1b(?!\\))*\x1b?$|\x1b(\[[0-9;?]*[ -/]*)?$")

    def __init__(self, rows, cols):
        self.rows, self.cols = rows, cols
        self.cells = [[" "] * cols for _ in range(rows)]
        self.row = self.col = 0
        self.decoder = codecs.getincrementaldecoder("utf-8")("replace")
        self.held = ""

    def text(self):
        return "\n".join("".join(line).rstrip() for line in self.cells)

    def feed(self, data):
        text = self.held + (data if isinstance(data, str) else self.decoder.decode(data))
        partial = self.PARTIAL.search(text)
        self.held = text[partial.start():] if partial else ""
        if partial:
            text = text[:partial.start()]
        position = 0
        for match in self.ESCAPE.finditer(text):
            self.print(text[position:match.start()])
            position = match.end()
            if match.group(2):
                self.control(match.group(1), match.group(2))
        self.print(text[position:])

    def print(self, text):
        for character in text:
            if character == "\r":
                self.col = 0
            elif character == "\n":
                self.row = min(self.row + 1, self.rows - 1)
            elif character >= " " and self.row < self.rows and self.col < self.cols:
                self.cells[self.row][self.col] = character
                self.col += 1

    def control(self, parameters, final):
        numbers = [int(part) if part.isdigit() else 0 for part in parameters.lstrip("?").split(";")]
        first = numbers[0] or 1
        if final in "Hf" and not parameters.startswith("?"):
            self.row = min((numbers[0] or 1) - 1, self.rows - 1)
            self.col = min(((numbers[1] if len(numbers) > 1 else 0) or 1) - 1, self.cols - 1)
        elif final == "A":
            self.row = max(self.row - first, 0)
        elif final == "B":
            self.row = min(self.row + first, self.rows - 1)
        elif final == "C":
            self.col = min(self.col + first, self.cols - 1)
        elif final == "D":
            self.col = max(self.col - first, 0)
        elif final == "G":
            self.col = min(first - 1, self.cols - 1)
        elif final == "d":
            self.row = min(first - 1, self.rows - 1)
        elif final == "X":
            stop = min(self.col + first, self.cols)
            self.cells[self.row][self.col:stop] = [" "] * (stop - self.col)
        elif final == "J":
            # 0 (or none): from the cursor to the end; 2 and 3: the whole screen.
            if numbers[0] in (2, 3):
                self.cells = [[" "] * self.cols for _ in range(self.rows)]
            elif numbers[0] == 0:
                self.cells[self.row][self.col:] = [" "] * (self.cols - self.col)
                for row in range(self.row + 1, self.rows):
                    self.cells[row] = [" "] * self.cols
        elif final == "K":
            start, stop = (0, self.cols) if numbers[0] == 2 else (self.col, self.cols)
            self.cells[self.row][start:stop] = [" "] * (stop - start)
