const GLYPHS: Record<string, [string, string]> = {
  O: ["█▀█", "█▄█"],
  H: ["█ █", "█▀█"],
  M: ["█▀▄▀█", "█ ▀ █"],
  Y: ["█▄█", " █ "],
  G: ["█▀▀", "█▄█"],
  I: ["█", "█"],
  A: ["▄▀█", "█▀█"],
  B: ["█▄▄", "█▄█"],
  T: ["▀█▀", " █ "],
  E: ["█▀▀", "██▄"],
  S: ["█▀", "▄█"],
};

export function banner(words: string[]): [string, string] {
  const rows: [string, string] = ["", ""];
  words.forEach((word, w) => {
    if (w > 0) {
      rows[0] += "   ";
      rows[1] += "   ";
    }
    [...word].forEach((c, i) => {
      if (i > 0) {
        rows[0] += " ";
        rows[1] += " ";
      }
      const [a, b] = GLYPHS[c] ?? ["  ", "  "];
      rows[0] += a;
      rows[1] += b;
    });
  });
  return rows;
}
