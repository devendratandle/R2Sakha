//! The text: a vector of lines, addressed by (line, column) in CHARACTERS.
//!
//! A script is at most tens of thousands of lines, so one `String` per line
//! is simpler than a rope and fast enough: an edit touches one or two
//! lines. Columns count `char`s, not bytes, so a cursor never lands inside
//! a multi-byte character; [`byte_at`] converts at the edge.

/// A position in the buffer. `col` may equal the line's length (the slot
/// after the last character).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

impl Pos {
    pub fn new(line: usize, col: usize) -> Pos { Pos { line, col } }
}

#[derive(Debug, Clone)]
pub struct Buffer {
    lines: Vec<String>,
}

/// Byte offset of character `col` in `s` (or `s.len()` past the end).
pub fn byte_at(s: &str, col: usize) -> usize {
    s.char_indices().nth(col).map(|(b, _)| b).unwrap_or(s.len())
}

impl Default for Buffer {
    fn default() -> Self { Buffer { lines: vec![String::new()] } }
}

impl Buffer {
    /// From text with `\n` or `\r\n` line endings (the caller strips a BOM).
    pub fn from_text(text: &str) -> Buffer {
        let lines: Vec<String> = text.split('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
            .collect();
        Buffer { lines }
    }

    pub fn text(&self) -> String { self.lines.join("\n") }
    pub fn line_count(&self) -> usize { self.lines.len() }
    pub fn line(&self, i: usize) -> &str { &self.lines[i] }
    pub fn lines(&self) -> &[String] { &self.lines }
    pub fn line_len(&self, i: usize) -> usize { self.lines[i].chars().count() }

    /// `p` moved into the buffer: a line past the end goes to the last
    /// line, a column past the end of its line to the end.
    pub fn clamp(&self, p: Pos) -> Pos {
        let line = p.line.min(self.lines.len() - 1);
        Pos { line, col: p.col.min(self.line_len(line)) }
    }

    pub fn end(&self) -> Pos {
        let line = self.lines.len() - 1;
        Pos { line, col: self.line_len(line) }
    }

    /// Insert `text` (may contain newlines) at `p`; returns the position
    /// just after it.
    pub fn insert(&mut self, p: Pos, text: &str) -> Pos {
        let p = self.clamp(p);
        let cur = &self.lines[p.line];
        let at = byte_at(cur, p.col);
        let (head, tail) = (cur[..at].to_string(), cur[at..].to_string());
        let mut parts = text.split('\n');
        let first = parts.next().unwrap_or("");
        let rest: Vec<&str> = parts.collect();
        if rest.is_empty() {
            self.lines[p.line] = format!("{head}{first}{tail}");
            return Pos { line: p.line, col: p.col + first.chars().count() };
        }
        self.lines[p.line] = format!("{head}{first}");
        let last = rest[rest.len() - 1];
        let mut new_lines: Vec<String> = rest[..rest.len() - 1].iter().map(|s| s.to_string()).collect();
        new_lines.push(format!("{last}{tail}"));
        let end = Pos { line: p.line + new_lines.len(), col: last.chars().count() };
        self.lines.splice(p.line + 1..p.line + 1, new_lines);
        end
    }

    /// The text between two positions, in either order.
    pub fn slice(&self, a: Pos, b: Pos) -> String {
        let (a, b) = order(self.clamp(a), self.clamp(b));
        if a.line == b.line {
            let l = &self.lines[a.line];
            return l[byte_at(l, a.col)..byte_at(l, b.col)].to_string();
        }
        let mut out = self.lines[a.line][byte_at(&self.lines[a.line], a.col)..].to_string();
        for l in &self.lines[a.line + 1..b.line] { out.push('\n'); out.push_str(l); }
        out.push('\n');
        let lb = &self.lines[b.line];
        out.push_str(&lb[..byte_at(lb, b.col)]);
        out
    }

    /// Delete the text between two positions, in either order; returns
    /// the earlier one (where the cursor goes).
    pub fn delete(&mut self, a: Pos, b: Pos) -> Pos {
        let (a, b) = order(self.clamp(a), self.clamp(b));
        let head = self.lines[a.line][..byte_at(&self.lines[a.line], a.col)].to_string();
        let lb = &self.lines[b.line];
        let tail = lb[byte_at(lb, b.col)..].to_string();
        self.lines.splice(a.line..=b.line, [format!("{head}{tail}")]);
        a
    }

    /// One character left of `p` (wrapping to the previous line's end).
    pub fn prev(&self, p: Pos) -> Pos {
        if p.col > 0 { Pos { line: p.line, col: p.col - 1 } }
        else if p.line > 0 { Pos { line: p.line - 1, col: self.line_len(p.line - 1) } }
        else { p }
    }

    /// One character right of `p` (wrapping to the next line's start).
    pub fn next(&self, p: Pos) -> Pos {
        if p.col < self.line_len(p.line) { Pos { line: p.line, col: p.col + 1 } }
        else if p.line + 1 < self.lines.len() { Pos { line: p.line + 1, col: 0 } }
        else { p }
    }

    /// The character at `p`, if any (none at a line end).
    pub fn char_at(&self, p: Pos) -> Option<char> { self.lines[p.line].chars().nth(p.col) }
}

/// The two positions, earlier first.
pub fn order(a: Pos, b: Pos) -> (Pos, Pos) { if a <= b { (a, b) } else { (b, a) } }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_text_and_crlf() {
        let b = Buffer::from_text("a <- 1\r\nb <- 2\n");
        assert_eq!(b.line_count(), 3);
        assert_eq!(b.text(), "a <- 1\nb <- 2\n");
    }

    #[test]
    fn insert_and_delete_across_lines_in_characters() {
        let mut b = Buffer::from_text("héllo world");
        let end = b.insert(Pos::new(0, 5), "\nfoo\nbar");
        assert_eq!(b.text(), "héllo\nfoo\nbar world");
        assert_eq!(end, Pos::new(2, 3));
        assert_eq!(b.slice(Pos::new(0, 1), Pos::new(2, 1)), "éllo\nfoo\nb");
        let at = b.delete(Pos::new(2, 3), Pos::new(0, 5));
        assert_eq!(at, Pos::new(0, 5));
        assert_eq!(b.text(), "héllo world");
    }

    #[test]
    fn movement_wraps_and_stops_at_the_ends() {
        let b = Buffer::from_text("ab\nc");
        assert_eq!(b.next(Pos::new(0, 2)), Pos::new(1, 0));
        assert_eq!(b.prev(Pos::new(1, 0)), Pos::new(0, 2));
        assert_eq!(b.prev(Pos::new(0, 0)), Pos::new(0, 0));
        assert_eq!(b.next(b.end()), b.end());
        assert_eq!(b.clamp(Pos::new(9, 9)), Pos::new(1, 1));
    }
}
