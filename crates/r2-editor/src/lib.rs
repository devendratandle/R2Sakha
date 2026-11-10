//! Ardon-R2's script editor, without a GUI.
//!
//! [`Editor`] is the whole editing model: a [`Buffer`] of lines, a cursor
//! and an optional selection anchor, undo/redo, and the file it came
//! from. `r2-gui` turns key presses into calls on it and paints what it
//! holds; everything that can be decided without pixels is decided here,
//! so it is tested here, headless.
//!
//! Modules: `buffer` (text and positions), `history` (undo), `highlight`
//! (syntax colours), `statement` (what Ctrl+Enter runs).

pub mod buffer;
pub mod highlight;
pub mod history;
pub mod statement;

pub use buffer::{order, Buffer, Pos};
use history::{EditKind, History};

/// Spaces a Tab inserts — the R style guides' two.
pub const INDENT: &str = "  ";

#[derive(Debug, Default)]
pub struct Editor {
    buf: Buffer,
    /// Where the caret is.
    pub cursor: Pos,
    /// The other end of the selection, when there is one.
    pub anchor: Option<Pos>,
    /// Column to return to when moving up/down through shorter lines.
    goal_col: Option<usize>,
    history: History,
    /// The file this text was loaded from or saved to.
    pub path: Option<std::path::PathBuf>,
    /// Edited since the last load or save.
    pub dirty: bool,
}

impl Editor {
    pub fn new() -> Editor { Editor::default() }

    pub fn from_text(text: &str) -> Editor {
        Editor { buf: Buffer::from_text(text), ..Editor::default() }
    }

    pub fn buffer(&self) -> &Buffer { &self.buf }
    pub fn text(&self) -> String { self.buf.text() }

    /// `name.R`, or "untitled" — for the window title (with `*` if dirty).
    pub fn title(&self) -> String {
        let name = self.path.as_ref()
            .and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "untitled".into());
        if self.dirty { format!("{name} *") } else { name }
    }

    // ── files ──────────────────────────────────────────────────────

    pub fn open(path: &std::path::Path) -> std::io::Result<Editor> {
        let raw = std::fs::read_to_string(path)?;
        let text = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
        let mut e = Editor::from_text(text);
        e.path = Some(path.to_path_buf());
        Ok(e)
    }

    /// Save to `path` (or the current path). Lines are joined with `\n`.
    pub fn save(&mut self, path: Option<&std::path::Path>) -> std::io::Result<()> {
        let p = match path.map(|p| p.to_path_buf()).or_else(|| self.path.clone()) {
            Some(p) => p,
            None => return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "no file name")),
        };
        std::fs::write(&p, self.buf.text())?;
        self.path = Some(p);
        self.dirty = false;
        self.history.break_run();
        Ok(())
    }

    // ── selection ──────────────────────────────────────────────────

    /// The selected range, earlier end first; `None` if nothing (or an
    /// empty range) is selected.
    pub fn selection(&self) -> Option<(Pos, Pos)> {
        self.anchor.filter(|&a| a != self.cursor).map(|a| order(a, self.cursor))
    }

    pub fn selected_text(&self) -> Option<String> {
        self.selection().map(|(a, b)| self.buf.slice(a, b))
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(Pos::default());
        self.cursor = self.buf.end();
    }

    /// Place the caret (a click), optionally extending the selection
    /// (a drag or Shift+click).
    pub fn set_cursor(&mut self, p: Pos, extend: bool) {
        self.begin_move(extend);
        self.cursor = self.buf.clamp(p);
        self.goal_col = None;
    }

    // ── editing ────────────────────────────────────────────────────

    fn record(&mut self, kind: EditKind) {
        let text = self.buf.text();
        self.history.record(&text, self.cursor, kind);
        self.dirty = true;
        self.goal_col = None;
    }

    /// Remove the selection, if any; true when something was removed.
    fn take_selection(&mut self) -> bool {
        match self.selection() {
            Some((a, b)) => { self.cursor = self.buf.delete(a, b); self.anchor = None; true }
            None => { self.anchor = None; false }
        }
    }

    /// Type one character (replacing any selection). A closing bracket
    /// typed where only indentation precedes it on the line drops one
    /// indent level, so `}` lines up with the line that opened the block
    /// and the lines after it are not left indented.
    pub fn type_char(&mut self, ch: char) {
        self.record(if self.selection().is_some() { EditKind::Other } else { EditKind::Typing });
        self.take_selection();
        if matches!(ch, '}' | ')' | ']') {
            let before: String = self.buf.line(self.cursor.line).chars().take(self.cursor.col).collect();
            if !before.is_empty() && before.chars().all(|c| c == ' ') {
                let drop = before.len().min(INDENT.len());
                let at = Pos::new(self.cursor.line, self.cursor.col - drop);
                self.cursor = self.buf.delete(at, self.cursor);
            }
        }
        self.cursor = self.buf.insert(self.cursor, &ch.to_string());
    }

    /// Insert text that may span lines (a paste), replacing any selection.
    pub fn insert_text(&mut self, text: &str) {
        self.record(EditKind::Other);
        self.take_selection();
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        self.cursor = self.buf.insert(self.cursor, &text);
    }

    /// Enter: a new line indented like this one, one level deeper after
    /// an opening `{`, `(` or `[`.
    pub fn newline(&mut self) {
        self.record(EditKind::Other);
        self.take_selection();
        let line = self.buf.line(self.cursor.line);
        let indent: String = line.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        let before: String = line.chars().take(self.cursor.col).collect();
        let deeper = matches!(before.trim_end().chars().last(), Some('{' | '(' | '['));
        let text = format!("\n{indent}{}", if deeper { INDENT } else { "" });
        self.cursor = self.buf.insert(self.cursor, &text);
    }

    /// Tab: indent the selected lines, or insert spaces at the caret.
    pub fn tab(&mut self, outdent: bool) {
        match self.selection() {
            Some((a, b)) => self.shift_lines(a.line, b.line, outdent),
            None if outdent => self.shift_lines(self.cursor.line, self.cursor.line, true),
            None => {
                self.record(EditKind::Other);
                self.cursor = self.buf.insert(self.cursor, INDENT);
            }
        }
    }

    fn shift_lines(&mut self, first: usize, last: usize, outdent: bool) {
        self.record(EditKind::Other);
        let n = INDENT.len();
        for l in first..=last {
            if outdent {
                let lead = self.buf.line(l).chars().take(n).take_while(|c| *c == ' ').count();
                self.buf.delete(Pos::new(l, 0), Pos::new(l, lead));
                if self.cursor.line == l { self.cursor.col = self.cursor.col.saturating_sub(lead); }
                if let Some(a) = self.anchor.as_mut().filter(|a| a.line == l) { a.col = a.col.saturating_sub(lead); }
            } else {
                self.buf.insert(Pos::new(l, 0), INDENT);
                if self.cursor.line == l { self.cursor.col += n; }
                if let Some(a) = self.anchor.as_mut().filter(|a| a.line == l) { a.col += n; }
            }
        }
    }

    pub fn backspace(&mut self) {
        if self.selection().is_some() { self.record(EditKind::Other); self.take_selection(); return; }
        let prev = self.buf.prev(self.cursor);
        if prev == self.cursor { return; }
        self.record(EditKind::Deleting);
        self.cursor = self.buf.delete(prev, self.cursor);
    }

    pub fn delete(&mut self) {
        if self.selection().is_some() { self.record(EditKind::Other); self.take_selection(); return; }
        let next = self.buf.next(self.cursor);
        if next == self.cursor { return; }
        self.record(EditKind::Deleting);
        self.buf.delete(self.cursor, next);
    }

    /// Cut: the selected text, removed. `None` with no selection.
    pub fn cut(&mut self) -> Option<String> {
        let t = self.selected_text()?;
        self.record(EditKind::Other);
        self.take_selection();
        Some(t)
    }

    pub fn undo(&mut self) { self.restore(true) }
    pub fn redo(&mut self) { self.restore(false) }

    fn restore(&mut self, undo: bool) {
        let text = self.buf.text();
        let got = if undo { self.history.undo(&text, self.cursor) } else { self.history.redo(&text, self.cursor) };
        if let Some((t, c)) = got {
            self.buf = Buffer::from_text(&t);
            self.cursor = self.buf.clamp(c);
            self.anchor = None;
            self.dirty = true;
        }
    }

    // ── movement ───────────────────────────────────────────────────

    /// Start a movement: Shift extends (setting the anchor once), a plain
    /// move drops the selection. Any move ends the current undo run.
    fn begin_move(&mut self, extend: bool) {
        if extend { if self.anchor.is_none() { self.anchor = Some(self.cursor); } }
        else { self.anchor = None; }
        self.history.break_run();
    }

    pub fn left(&mut self, extend: bool, word: bool) {
        let collapse = !extend && self.selection().is_some();
        let start = self.selection().map(|s| s.0);
        self.begin_move(extend);
        self.cursor = if collapse { start.unwrap() }
            else if word { self.word_boundary(self.cursor, false) }
            else { self.buf.prev(self.cursor) };
        self.goal_col = None;
    }

    pub fn right(&mut self, extend: bool, word: bool) {
        let collapse = !extend && self.selection().is_some();
        let end = self.selection().map(|s| s.1);
        self.begin_move(extend);
        self.cursor = if collapse { end.unwrap() }
            else if word { self.word_boundary(self.cursor, true) }
            else { self.buf.next(self.cursor) };
        self.goal_col = None;
    }

    /// Up (`delta < 0`) or down by whole lines, keeping the goal column.
    pub fn vertical(&mut self, delta: isize, extend: bool) {
        self.begin_move(extend);
        let goal = *self.goal_col.get_or_insert(self.cursor.col);
        let last = self.buf.line_count() as isize - 1;
        let line = (self.cursor.line as isize + delta).clamp(0, last) as usize;
        self.cursor = self.buf.clamp(Pos::new(line, goal));
    }

    /// Home goes to the first non-blank character, then to column 0;
    /// with `doc`, to the start of the file.
    pub fn home(&mut self, extend: bool, doc: bool) {
        self.begin_move(extend);
        self.goal_col = None;
        if doc { self.cursor = Pos::default(); return; }
        let first = self.buf.line(self.cursor.line).chars().take_while(|c| c.is_whitespace()).count();
        self.cursor.col = if self.cursor.col == first { 0 } else { first };
    }

    pub fn end(&mut self, extend: bool, doc: bool) {
        self.begin_move(extend);
        self.goal_col = None;
        self.cursor = if doc { self.buf.end() } else { Pos::new(self.cursor.line, self.buf.line_len(self.cursor.line)) };
    }

    /// The next word start (forward) or previous word start (back).
    fn word_boundary(&self, p: Pos, forward: bool) -> Pos {
        let is_word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.');
        let mut q = p;
        if forward {
            let w = is_word(self.buf.char_at(q));
            while q != self.buf.end() && is_word(self.buf.char_at(q)) == w && self.buf.char_at(q).is_some() {
                q = self.buf.next(q);
            }
            while q != self.buf.end() && self.buf.char_at(q).is_some_and(|c| c == ' ') { q = self.buf.next(q); }
            if q == p { q = self.buf.next(q); }
        } else {
            q = self.buf.prev(q);
            while q.col > 0 && self.buf.char_at(q).is_some_and(|c| c == ' ') { q = self.buf.prev(q); }
            let w = is_word(self.buf.char_at(q));
            while q.col > 0 && is_word(self.buf.char_at(self.buf.prev(q))) == w
                  && self.buf.char_at(self.buf.prev(q)).is_some_and(|c| c != ' ' || !w) {
                q = self.buf.prev(q);
            }
        }
        q
    }

    // ── running code ───────────────────────────────────────────────

    /// What Ctrl+Enter sends to the console: the selection if there is
    /// one, otherwise the whole statement under the caret — after which
    /// the caret moves to the line below it, so repeated Ctrl+Enter steps
    /// through a script (the RStudio convention).
    pub fn take_run_code(&mut self) -> Option<String> {
        if let Some(t) = self.selected_text() { return Some(t); }
        let (a, b) = statement::statement_at(self.buf.lines(), self.cursor.line)?;
        let code = self.buf.lines()[a..=b].join("\n");
        let next = (b + 1).min(self.buf.line_count() - 1);
        self.set_cursor(Pos::new(if b + 1 < self.buf.line_count() { next } else { b }, 0), false);
        if b + 1 >= self.buf.line_count() { self.end(false, false); }
        Some(code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(s: &str) -> Editor {
        let mut e = Editor::new();
        for ch in s.chars() { if ch == '\n' { e.newline() } else { e.type_char(ch) } }
        e
    }

    #[test]
    fn typing_enter_auto_indents_and_undo_takes_back_a_word() {
        let mut e = typed("f <- function(x) {\nx + 1");
        assert_eq!(e.text(), "f <- function(x) {\n  x + 1");
        assert!(e.dirty);
        e.newline();
        assert_eq!(e.text(), "f <- function(x) {\n  x + 1\n  ");
        e.undo();
        assert_eq!(e.text(), "f <- function(x) {\n  x + 1");
        e.undo();
        assert_eq!(e.text(), "f <- function(x) {\n  ");
        e.redo();
        assert_eq!(e.text(), "f <- function(x) {\n  x + 1");
    }

    #[test]
    fn a_closing_brace_on_its_own_line_outdents() {
        let e = typed("f <- function(v) {\nv * 2\n}\nprint(1)");
        assert_eq!(e.text(), "f <- function(v) {\n  v * 2\n}\nprint(1)");
        // not when there is code before it on the line
        let e = typed("g(a)");
        assert_eq!(e.text(), "g(a)");
        let e = typed("x <- list(\n1)");
        assert_eq!(e.text(), "x <- list(\n  1)");
    }

    #[test]
    fn selection_replace_cut_and_paste() {
        let mut e = Editor::from_text("alpha beta");
        e.set_cursor(Pos::new(0, 0), false);
        e.right(true, true);
        assert_eq!(e.selected_text().as_deref(), Some("alpha "));
        e.type_char('Z');
        assert_eq!(e.text(), "Zbeta");
        e.select_all();
        assert_eq!(e.cut().as_deref(), Some("Zbeta"));
        e.insert_text("a\r\nb");
        assert_eq!(e.text(), "a\nb");
        assert_eq!(e.cursor, Pos::new(1, 1));
    }

    #[test]
    fn movement_keeps_the_goal_column_and_home_toggles() {
        let mut e = Editor::from_text("long line here\nab\n  indented text");
        e.set_cursor(Pos::new(0, 10), false);
        e.vertical(1, false);
        assert_eq!(e.cursor, Pos::new(1, 2));
        e.vertical(1, false);
        assert_eq!(e.cursor, Pos::new(2, 10));
        e.home(false, false);
        assert_eq!(e.cursor.col, 2);
        e.home(false, false);
        assert_eq!(e.cursor.col, 0);
        e.end(false, true);
        assert_eq!(e.cursor, Pos::new(2, 15));
        e.left(false, true);
        assert_eq!(e.cursor, Pos::new(2, 11));
    }

    #[test]
    fn tab_indents_and_outdents_selected_lines() {
        let mut e = Editor::from_text("a\nb\nc");
        e.set_cursor(Pos::new(0, 0), false);
        e.set_cursor(Pos::new(1, 1), true);
        e.tab(false);
        assert_eq!(e.text(), "  a\n  b\nc");
        e.tab(true);
        assert_eq!(e.text(), "a\nb\nc");
    }

    #[test]
    fn backspace_joins_lines_and_delete_at_end_is_a_no_op() {
        let mut e = Editor::from_text("ab\ncd");
        e.set_cursor(Pos::new(1, 0), false);
        e.backspace();
        assert_eq!(e.text(), "abcd");
        e.end(false, true);
        e.delete();
        assert_eq!(e.text(), "abcd");
    }

    #[test]
    fn run_steps_through_statements_and_prefers_a_selection() {
        let mut e = Editor::from_text("x <- 1\n\ny <- c(1,\n  2)\nprint(y)");
        assert_eq!(e.take_run_code().as_deref(), Some("x <- 1"));
        assert_eq!(e.cursor.line, 1);
        assert_eq!(e.take_run_code().as_deref(), Some("y <- c(1,\n  2)"));
        assert_eq!(e.cursor.line, 4);
        assert_eq!(e.take_run_code().as_deref(), Some("print(y)"));
        e.set_cursor(Pos::new(2, 0), false);
        e.set_cursor(Pos::new(2, 6), true);
        assert_eq!(e.take_run_code().as_deref(), Some("y <- c"));
    }

    #[test]
    fn save_and_open_round_trip_and_track_dirty() {
        let dir = std::env::temp_dir().join(format!("r2-editor-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("s.R");
        let mut e = typed("a <- 1");
        assert!(e.save(None).is_err());
        e.save(Some(&p)).unwrap();
        assert!(!e.dirty);
        assert_eq!(e.title(), "s.R");
        std::fs::write(&p, "\u{feff}b <- 2\r\n").unwrap();
        let f = Editor::open(&p).unwrap();
        assert_eq!(f.text(), "b <- 2\n");
        std::fs::remove_dir_all(&dir).ok();
    }
}
