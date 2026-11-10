//! Undo and redo as snapshots of the whole text.
//!
//! Snapshots, not inverse operations: a script is small, a snapshot is
//! one `String`, and a snapshot cannot be subtly wrong the way a
//! hand-written inverse of every edit can. Consecutive typing within a
//! line is merged into one step (one Ctrl+Z removes the word, not a
//! letter), and the depth is capped so a long session stays bounded.

use crate::buffer::Pos;

const MAX_DEPTH: usize = 500;

#[derive(Debug, Clone)]
struct Snapshot { text: String, cursor: Pos }

#[derive(Debug, Default)]
pub struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// The kind of the last recorded edit and the line it was on, so a
    /// run of typing on one line becomes one undo step.
    last: Option<(EditKind, usize)>,
}

/// What an edit was, for deciding whether it merges with the previous one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind { Typing, Deleting, Other }

impl History {
    /// Call BEFORE applying an edit, with the text and cursor as they are
    /// now. Typing (or deleting) that continues the previous edit on the
    /// same line is not recorded again.
    pub fn record(&mut self, text: &str, cursor: Pos, kind: EditKind) {
        let merge = kind != EditKind::Other && self.last == Some((kind, cursor.line));
        self.last = Some((kind, cursor.line));
        self.redo.clear();
        if merge { return; }
        self.undo.push(Snapshot { text: text.to_string(), cursor });
        if self.undo.len() > MAX_DEPTH { self.undo.remove(0); }
    }

    /// Ends the current merge run (a cursor move, a save): the next edit
    /// starts a new undo step.
    pub fn break_run(&mut self) { self.last = None; }

    /// The state to restore, given the current one (which goes to redo).
    pub fn undo(&mut self, text: &str, cursor: Pos) -> Option<(String, Pos)> {
        let s = self.undo.pop()?;
        self.redo.push(Snapshot { text: text.to_string(), cursor });
        self.last = None;
        Some((s.text, s.cursor))
    }

    pub fn redo(&mut self, text: &str, cursor: Pos) -> Option<(String, Pos)> {
        let s = self.redo.pop()?;
        self.undo.push(Snapshot { text: text.to_string(), cursor });
        self.last = None;
        Some((s.text, s.cursor))
    }

    pub fn clear(&mut self) { *self = History::default(); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_on_one_line_is_one_step_and_redo_replays() {
        let mut h = History::default();
        let p = Pos::new(0, 0);
        h.record("", p, EditKind::Typing);
        h.record("a", Pos::new(0, 1), EditKind::Typing);
        h.record("ab", Pos::new(0, 2), EditKind::Typing);
        assert_eq!(h.undo("abc", Pos::new(0, 3)), Some((String::new(), p)));
        assert_eq!(h.undo("", p), None);
        assert_eq!(h.redo("", p), Some(("abc".to_string(), Pos::new(0, 3))));
    }

    #[test]
    fn a_new_edit_clears_redo_and_other_edits_never_merge() {
        let mut h = History::default();
        h.record("x", Pos::new(0, 1), EditKind::Other);
        h.record("xy", Pos::new(0, 2), EditKind::Other);
        assert!(h.undo("xyz", Pos::new(0, 3)).is_some());
        h.record("xy", Pos::new(0, 2), EditKind::Typing);
        assert!(h.redo("xyq", Pos::new(0, 3)).is_none());
        assert!(h.undo("xyq", Pos::new(0, 3)).is_some());
        assert!(h.undo("xy", Pos::new(0, 2)).is_some());
    }
}
