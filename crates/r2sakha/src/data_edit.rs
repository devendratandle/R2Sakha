//! Editing data in Sakha's Data tab, as in jamovi: type in a cell, click a
//! variable's name for its type, sorting, recoding and deleting, and compute,
//! recode or filter from the buttons above the grid. Every change is a line of
//! R2 code (`sakha_core::edit`) that R2 runs and the session script keeps, so
//! the edited data can be rebuilt from its file, and Undo is "load it again
//! and repeat all but the last change".

use r2_ui::{ContextItem, ContextMenu, Frame, InputEvent, KeyCode, MouseButton, Rect, Renderer, Theme};
use sakha_core::edit::{self, Edit};
use sakha_core::import::{to_category_code, to_number_code};
use sakha_core::profile::Kind;

use crate::data_dialogs::{dialog_box, text_box, typed};
use crate::data_view::GridAsk;
use crate::session::After;
use crate::ui::{button, inside, mix, text, wrap};
use crate::Gui;

#[derive(Clone, Copy, PartialEq)]
enum What { Compute, Recode(usize), Filter }

/// Compute, recode or filter: one or two lines to fill in.
pub(crate) struct EditDialog {
    what: What,
    fields: [String; 2],
    focus: usize,
    message: String,
    /// The script step R2 is running for this dialog.
    waiting: Option<String>,
    boxes: [Option<Rect>; 2],
    run: Option<Rect>,
    cancel: Option<Rect>,
}

impl EditDialog {
    fn new(what: What, first: &str) -> EditDialog {
        EditDialog { what, fields: [first.to_string(), String::new()], focus: if first.is_empty() { 0 } else { 1 },
                     message: String::new(), waiting: None, boxes: [None; 2], run: None, cancel: None }
    }
}

impl Gui {
    /// Run an edit in R2 and keep it in the script; its step's key comes back.
    pub(crate) fn apply_edit(&mut self, e: Edit) -> String {
        let s = &mut self.sakha;
        let key = s.script.record_data(&e.title, &e.code);
        s.dirty = true;
        s.status.clear();
        // results already run are of the data as it was
        if !s.results.is_empty() { s.data_changed = true; }
        for line in e.code { self.run_queue.push_back((line, Some(After::DataEdit { key: key.clone() }))); }
        key
    }

    /// R2 has run (or refused) a line of an edit.
    pub(crate) fn data_edit_done(&mut self, key: &str, failed: bool) {
        if failed {
            self.sakha.script.remove(key);
            self.sakha.status = "Could not make that change; the Console in Pro says why.".into();
        }
        if let Some(d) = self.edit_dialog.as_mut().filter(|d| d.waiting.as_deref() == Some(key)) {
            if failed {
                d.waiting = None;
                let why = self.session.last_err.trim();
                d.message = if why.is_empty() { "R2 could not do that. Check the names and the spelling.".into() }
                            else { format!("R2 could not do that. {why}") };
            } else {
                self.edit_dialog = None;
            }
        }
    }

    /// The grid's input while the Data tab is in front.
    pub(crate) fn data_face_input(&mut self, events: &[InputEvent]) {
        let Some(v) = self.sakha.data_uid.and_then(|u| self.views.get_mut(&u)) else { return };
        v.editable = true;
        for ask in v.edit_input(events) {
            match ask {
                GridAsk::Cell { row, col, typed } => {
                    let (Some(df), Some(c)) = (self.sakha.df.clone(), self.sakha.cols.get(col)) else { return };
                    match edit::cell(&df, row + 1, &c.name, c.kind, &typed) {
                        Ok(e) => { self.apply_edit(e); }
                        Err(why) => self.sakha.status = format!("Could not change it: {why}"),
                    }
                }
                GridAsk::Header { col, x, y } => self.open_header_menu(col, x, y),
            }
        }
    }

    /// A variable's menu, under its name: its type first, as in jamovi.
    fn open_header_menu(&mut self, col: usize, x: f32, y: f32) {
        let Some(c) = self.sakha.cols.get(col) else { return };
        let mark = |on: bool, s: &str| format!("{} {s}", if on { "*" } else { " " });
        self.column_menu = ContextMenu::new(vec![
            ContextItem::new(mark(c.kind == Kind::Number, "Number"), format!("dataedit.type:{col}:n")),
            ContextItem::new(mark(c.kind == Kind::Category, "Category"), format!("dataedit.type:{col}:c")),
            ContextItem::separator(),
            ContextItem::new("  Rename…", format!("dataedit.rename:{col}")),
            ContextItem::new("  Recode…", format!("dataedit.recode:{col}")),
            ContextItem::new("  Sort, smallest first", format!("dataedit.sort:{col}:a")),
            ContextItem::new("  Sort, largest first", format!("dataedit.sort:{col}:d")),
            ContextItem::separator(),
            ContextItem::new("  Delete variable", format!("dataedit.delvar:{col}")),
        ]);
        self.column_menu.open_at(x, y);
    }

    /// The chosen cell of the Data tab: (row from 0, column).
    fn chosen_cell(&self) -> Option<(usize, usize)> {
        self.sakha.data_uid.and_then(|u| self.views.get(&u)).and_then(|v| v.sel)
    }

    /// `dataedit.*`: the buttons above the grid, the Data menu, a variable's menu.
    pub(crate) fn data_edit_action(&mut self, a: &str) {
        let Some(df) = self.sakha.df.clone() else {
            self.sakha.choosing = true;
            self.sakha.status = "First load a dataset.".into();
            return;
        };
        self.sakha.face = crate::sakha_view::Face::Data;
        let mut parts = a.split(':');
        let (what, col, arg) = (parts.next().unwrap_or(""), parts.next().and_then(|c| c.parse::<usize>().ok()), parts.next());
        let col = col.or(self.chosen_cell().map(|c| c.1));
        let named = col.and_then(|i| self.sakha.cols.get(i)).map(|c| (c.name.clone(), c.kind));
        let need_cell = "First click a cell of the variable.";
        match (what, named) {
            ("compute", _) => self.edit_dialog = Some(EditDialog::new(What::Compute, "")),
            ("filter", _) => self.edit_dialog = Some(EditDialog::new(What::Filter, "")),
            ("recode", Some((name, _))) => self.edit_dialog = Some(EditDialog::new(What::Recode(col.unwrap_or(0)), &name)),
            ("rename", Some(_)) => self.open_attributes_at(col.unwrap_or(0)),
            ("type", Some((name, kind))) => {
                let to = if arg == Some("n") { Kind::Number } else { Kind::Category };
                if to == kind { return; }
                let line = if to == Kind::Number { to_number_code(&df, &name) } else { to_category_code(&df, &name) };
                self.apply_edit(Edit { title: format!("{name} as a {}", to.label()), code: vec![line] });
            }
            ("sort", Some((name, _))) => { self.apply_edit(edit::sort(&df, &name, arg == Some("d"))); }
            ("delvar", Some((name, _))) => { self.apply_edit(edit::delete_variable(&df, &name)); }
            ("addrow", _) => { self.apply_edit(edit::add_row(&df)); }
            ("delrow", _) => match self.chosen_cell() {
                Some((row, _)) => { self.apply_edit(edit::delete_row(&df, row + 1)); }
                None => self.sakha.status = "First click a cell in the row.".into(),
            },
            ("undo", _) => self.undo_data(&df),
            (_, None) => self.sakha.status = need_cell.into(),
            _ => {}
        }
    }

    /// Take back the latest change: load the data again, repeat the others.
    fn undo_data(&mut self, df: &str) {
        let s = &mut self.sakha;
        let Some(load) = s.load.clone() else {
            s.status = format!("Undo needs the line that loaded {df}, and it was made in the console.");
            return;
        };
        let Some(what) = s.script.undo_data() else {
            s.status = "No change to the data to undo.".into();
            return;
        };
        s.dirty = true;
        s.status = format!("Undone: {what}");
        let lines: Vec<String> = edit::reload(df, &load).into_iter().chain(s.script.data_steps()).collect();
        for line in lines { self.run_queue.push_back((line, None)); }
    }

    /// The buttons above the grid; they go through `hits.links` like any other.
    pub(crate) fn paint_data_face(&mut self, main: Rect, frame: &mut Frame, renderer: &mut Renderer, theme: &Theme) {
        let (cw, lh) = renderer.cell_metrics(theme.fs());
        let h = lh + 6.0;
        let mut x = main.x + 4.0;
        for (label, action) in [("Compute…", "dataedit.compute"), ("Recode…", "dataedit.recode"), ("Filter rows…", "dataedit.filter"),
                                ("Add row", "dataedit.addrow"), ("Delete row", "dataedit.delrow"), ("Undo", "dataedit.undo")] {
            let r = Rect { x, y: main.y, w: (label.chars().count() + 2) as f32 * cw, h };
            button(frame, renderer, theme, r, label, false);
            self.sakha.hits.links.push((r, action.into()));
            x += r.w + 6.0;
        }
        let grid = Rect { x: main.x, y: main.y + h + 6.0, w: main.w, h: (main.h - h - 6.0).max(0.0) };
        if let Some(uid) = self.sakha.data_uid {
            if let Some(v) = self.views.get_mut(&uid) { v.paint(grid, frame, renderer, theme); }
            self.fetch_view_pages(uid);
        }
    }

    pub(crate) fn edit_dialog_input(&mut self, events: &[InputEvent]) {
        let Some(df) = self.sakha.df.clone() else { self.edit_dialog = None; return };
        let Some(d) = self.edit_dialog.as_mut() else { return };
        let two = d.what != What::Filter;
        let (mut close, mut run) = (false, false);
        for ev in events {
            if d.waiting.is_none() {
                let before = d.fields[d.focus].len();
                typed(&mut d.fields[d.focus], ev);
                if d.fields[d.focus].len() != before { d.message.clear(); }
            }
            match *ev {
                InputEvent::Key { code: KeyCode::Escape, pressed: true, .. } => close = true,
                InputEvent::Key { code: KeyCode::Enter, pressed: true, .. } => run = true,
                InputEvent::Key { code: KeyCode::Tab, pressed: true, .. } if two => d.focus = 1 - d.focus,
                InputEvent::MouseDown { button: MouseButton::Left, pos } => {
                    let hit = |r: &Option<Rect>| r.is_some_and(|r| inside(r, pos.x, pos.y));
                    if hit(&d.cancel) { close = true; }
                    else if hit(&d.run) { run = true; }
                    else if let Some(k) = d.boxes.iter().position(hit) { d.focus = k; }
                }
                _ => {}
            }
        }
        if close { self.edit_dialog = None; return; }
        if !run || d.waiting.is_some() { return; }
        let cols = &self.sakha.cols;
        let built = match d.what {
            What::Compute => {
                if cols.iter().any(|c| c.name == d.fields[0].trim()) { Err(format!("There is already a variable called {}.", d.fields[0].trim())) }
                else { edit::compute(&df, &d.fields[0], &d.fields[1]) }
            }
            What::Recode(i) => match cols.get(i) {
                Some(c) => edit::recode(&df, &c.name, c.kind, &d.fields[0], &d.fields[1]),
                None => Err("That variable is gone.".into()),
            },
            What::Filter => edit::filter(&df, &d.fields[0]),
        };
        match built {
            Err(why) => d.message = why,
            Ok(e) => {
                d.message = "R2 is working…".into();
                let key = self.apply_edit(e);
                if let Some(d) = self.edit_dialog.as_mut() { d.waiting = Some(key); }
            }
        }
    }

    pub(crate) fn paint_edit_dialog(&mut self, area: Rect, frame: &mut Frame, renderer: &mut Renderer, theme: &Theme) {
        let cols: Vec<(String, Kind, Vec<String>)> = self.sakha.cols.iter()
            .map(|c| (c.name.clone(), c.kind, c.levels.iter().take(12).map(|l| l.0.clone()).collect())).collect();
        let Some(d) = self.edit_dialog.as_mut() else { return };
        let (cw, lh) = renderer.cell_metrics(theme.fs());
        let (bg, fg, size) = (theme.window_background, theme.console_output, theme.fs());
        let dim = mix(fg, bg, 0.4);
        let names = cols.iter().map(|c| c.0.as_str()).collect::<Vec<_>>().join(", ");
        // (title, the two labels, the help under them)
        let (title, labels, help): (String, [&str; 2], String) = match d.what {
            What::Compute => ("Compute a new variable".into(), ["Name", "Formula"],
                format!("A formula over the variables, as R2 writes it: (a + b) / 2, log(x), x > 10. Variables: {names}.")),
            What::Filter => ("Filter rows".into(), ["Keep rows where", ""],
                format!("A condition, as R2 writes it: age >= 18, group == \"a\", x > 2 & y < 5. Rows where it does not hold are removed; Undo brings them back. Variables: {names}.")),
            What::Recode(i) => {
                let (name, kind, levels) = cols.get(i).cloned().unwrap_or((String::new(), Kind::Text, vec![]));
                if kind == Kind::Number {
                    (format!("Recode {name} into groups"), ["Into", "Cut at"],
                     format!("Give the points to cut at: 10, 20 makes three groups (up to 10, 10 to 20, above 20). Into {name} itself replaces it; another name makes a new variable."))
                } else {
                    (format!("Recode {name}"), ["Into", "Changes"],
                     format!("Give the changes as old = new, old2 = new. Values not named stay as they are. {name} has: {}. Into {name} itself replaces it; another name makes a new variable.", levels.join(", ")))
                }
            }
        };
        let b = dialog_box(area, 76.0, 17.0, frame, renderer, theme, &title);
        let x = b.x + 10.0;
        let lw = 17.0 * cw;
        let mut y = b.y + lh + 18.0;
        for (k, label) in labels.iter().enumerate() {
            if label.is_empty() { d.boxes[k] = None; continue; }
            text(frame, renderer, x, y + 3.0 + lh * 0.8, label, size, dim, cw, x + lw);
            let r = Rect { x: x + lw, y, w: b.w - 20.0 - lw, h: lh + 6.0 };
            text_box(r, &d.fields[k], d.focus == k, frame, renderer, theme);
            d.boxes[k] = Some(r);
            y += lh + 14.0;
        }
        for line in wrap(&help, ((b.w - 20.0) / cw) as usize) {
            text(frame, renderer, x, y + lh * 0.8, &line, size, dim, cw, b.x + b.w - 8.0);
            y += lh;
        }
        let fy = b.y + b.h - lh - 10.0;
        let c = Rect { x: b.x + b.w - 10.0 * cw - 10.0, y: fy, w: 10.0 * cw, h: lh + 6.0 };
        let r = Rect { x: c.x - 10.0 * cw - 8.0, y: fy, w: 10.0 * cw, h: lh + 6.0 };
        button(frame, renderer, theme, c, "Cancel", false);
        button(frame, renderer, theme, r, "Apply", true);
        (d.cancel, d.run) = (Some(c), Some(r));
        // what went wrong, under the help and clear of the buttons
        y += lh * 0.5;
        for line in wrap(&d.message, ((b.w - 20.0) / cw) as usize).into_iter().take(4) {
            text(frame, renderer, x, y + lh * 0.8, &line, size, theme.console_error, cw, b.x + b.w - 8.0);
            y += lh;
        }
    }
}

