//! The data viewer: a page-at-a-time grid over `r2 --ide`'s `view` request.
//! A frozen header (column name, and its type underneath), a frozen row-label
//! column, then the cells, formatted by R2 as `print()` formats them. Only the
//! pages on screen are asked for, so a million-row frame opens at once. Used
//! by Pro's Source tabs and by Sakha's Data tab.

use std::collections::{HashMap, HashSet};

use r2_ui::{Frame, InputEvent, KeyCode, MouseButton, Rect, Renderer, Theme};
use sakha_engine::{json, Value};

use crate::session::Req;
use crate::ui::{inside, mix, text};
use crate::Gui;

/// Rows in one page asked of R2.
const PAGE: usize = 200;
/// Gap between columns, in cells.
const SEP: usize = 2;

struct Row { label: String, cells: Vec<String> }

/// What an editable grid asks of its owner.
pub(crate) enum GridAsk {
    /// Row (from 0) and column got this typed text.
    Cell { row: usize, col: usize, typed: String },
    /// A column's name was clicked: open its menu here.
    Header { col: usize, x: f32, y: f32 },
}

/// Where the grid was last painted, to find what a click landed on.
struct Geo { head: Rect, body: Rect, line_h: f32, cols: Vec<(usize, f32, f32)> }

impl Default for Geo {
    fn default() -> Geo {
        let none = Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 };
        Geo { head: none, body: none, line_h: 0.0, cols: vec![] }
    }
}

pub(crate) struct DataView {
    /// The R2 expression shown (usually a variable's name).
    pub(crate) code: String,
    shape: String,
    nrow: usize,
    headers: Vec<String>,
    types: Vec<String>,
    widths: Vec<usize>,
    right: Vec<bool>,
    /// Page number → (generation it was read in, its rows). An out-of-date
    /// page stays on screen until the new one arrives.
    pages: HashMap<usize, (u64, Vec<Row>)>,
    gen: u64,
    asked: HashSet<(usize, u64)>,
    /// R2 could not show the expression.
    pub(crate) error: Option<String>,
    top: usize,
    left: usize,
    rows_fit: usize,
    /// Sakha's Data tab: cells can be chosen and typed in. Pro only shows.
    pub(crate) editable: bool,
    /// The chosen cell: row (from 0) and column.
    pub(crate) sel: Option<(usize, usize)>,
    /// What is being typed into it.
    typing: Option<String>,
    geo: Geo,
}

impl DataView {
    pub(crate) fn new(code: &str) -> DataView {
        DataView { code: code.to_string(), shape: String::new(), nrow: 0, headers: vec![], types: vec![], widths: vec![],
                   right: vec![], pages: HashMap::new(), gen: 0, asked: HashSet::new(), error: None, top: 0, left: 0, rows_fit: 40,
                   editable: false, sel: None, typing: None, geo: Geo::default() }
    }

    fn cell_text(&self, row: usize, col: usize) -> String {
        self.row(row).and_then(|r| r.cells.get(col)).map(|s| if s == "NA" { String::new() } else { s.clone() }).unwrap_or_default()
    }

    /// The cell under a point: (row from 0, column).
    fn cell_at(&self, x: f32, y: f32) -> Option<(usize, usize)> {
        let g = &self.geo;
        if g.line_h <= 0.0 || !inside(g.body, x, y) { return None; }
        let row = self.top + ((y - g.body.y) / g.line_h) as usize;
        let col = g.cols.iter().find(|(_, cx, w)| x >= *cx && x < cx + w)?.0;
        (row < self.nrow).then_some((row, col))
    }

    /// Move the chosen cell, keeping it on screen.
    fn move_sel(&mut self, dr: isize, dc: isize) {
        let Some((r, c)) = self.sel else { return };
        let r = (r as isize + dr).clamp(0, self.nrow.saturating_sub(1) as isize) as usize;
        let c = (c as isize + dc).clamp(0, self.headers.len().saturating_sub(1) as isize) as usize;
        self.sel = Some((r, c));
        if r < self.top { self.top = r; }
        if self.rows_fit > 0 && r >= self.top + self.rows_fit { self.top = r + 1 - self.rows_fit; }
        if c < self.left { self.left = c; }
        if dc > 0 && !self.geo.cols.iter().any(|(k, _, _)| *k == c) { self.left = (self.left + 1).min(c); }
    }

    /// What was typed is done: ask the owner to make it so.
    fn commit(&mut self) -> Option<GridAsk> {
        let typed = self.typing.take()?;
        let (row, col) = self.sel?;
        (typed.trim() != self.cell_text(row, col)).then_some(GridAsk::Cell { row, col, typed })
    }

    /// Input for a grid whose cells can be edited.
    pub(crate) fn edit_input(&mut self, events: &[InputEvent]) -> Vec<GridAsk> {
        let mut asks = Vec::new();
        for ev in events {
            match *ev {
                InputEvent::Scroll { dy } => self.wheel(dy),
                InputEvent::MouseDown { button: MouseButton::Left, pos } => {
                    asks.extend(self.commit());
                    if inside(self.geo.head, pos.x, pos.y) {
                        if let Some(&(col, cx, _)) = self.geo.cols.iter().find(|(_, cx, w)| pos.x >= *cx && pos.x < cx + w) {
                            asks.push(GridAsk::Header { col, x: cx, y: self.geo.head.y + self.geo.head.h });
                        }
                    } else if let Some(cell) = self.cell_at(pos.x, pos.y) {
                        // a second click on the chosen cell opens it for typing
                        if self.sel == Some(cell) { self.typing = Some(self.cell_text(cell.0, cell.1)); }
                        self.sel = Some(cell);
                    }
                }
                InputEvent::Char(c) if !c.is_control() && self.sel.is_some() => {
                    self.typing.get_or_insert_with(String::new).push(c);
                }
                InputEvent::Key { code, pressed: true, mods } if !mods.ctrl => match (code, self.typing.is_some()) {
                    (KeyCode::Backspace, true) => { self.typing.as_mut().map(String::pop); }
                    (KeyCode::Escape, true) => self.typing = None,
                    (KeyCode::Enter | KeyCode::Down, true) => { asks.extend(self.commit()); self.move_sel(1, 0); }
                    (KeyCode::Tab, true) => { asks.extend(self.commit()); self.move_sel(0, 1); }
                    (KeyCode::Up, true) => { asks.extend(self.commit()); self.move_sel(-1, 0); }
                    (KeyCode::Right, true) => { asks.extend(self.commit()); self.move_sel(0, 1); }
                    (KeyCode::Left, true) => { asks.extend(self.commit()); self.move_sel(0, -1); }
                    (_, true) => {}
                    (KeyCode::Enter, false) => if let Some((r, c)) = self.sel { self.typing = Some(self.cell_text(r, c)); },
                    (KeyCode::Delete, false) => if let Some((row, col)) = self.sel {
                        if !self.cell_text(row, col).is_empty() { asks.push(GridAsk::Cell { row, col, typed: String::new() }); }
                    },
                    (KeyCode::Escape, false) => self.sel = None,
                    (_, false) if self.sel.is_none() => self.key(code),
                    (KeyCode::Down, false) => self.move_sel(1, 0),
                    (KeyCode::Up, false) => self.move_sel(-1, 0),
                    (KeyCode::Right | KeyCode::Tab, false) => self.move_sel(0, 1),
                    (KeyCode::Left, false) => self.move_sel(0, -1),
                    (KeyCode::PageDown, false) => self.move_sel(self.rows_fit.max(1) as isize, 0),
                    (KeyCode::PageUp, false) => self.move_sel(-(self.rows_fit.max(1) as isize), 0),
                    _ => {}
                },
                _ => {}
            }
        }
        asks
    }

    /// The data may have changed: read what is on screen again.
    pub(crate) fn invalidate(&mut self) { self.gen += 1; }

    /// Pages on screen that are missing or out of date, not yet asked for.
    fn wanted(&self) -> Vec<usize> {
        let (a, b) = (self.top / PAGE, (self.top + self.rows_fit) / PAGE);
        (a..=b).filter(|p| self.pages.get(p).map(|x| x.0) != Some(self.gen) && !self.asked.contains(&(*p, self.gen))).collect()
    }

    fn row(&self, r: usize) -> Option<&Row> {
        self.pages.get(&(r / PAGE)).and_then(|(_, rows)| rows.get(r % PAGE))
    }

    pub(crate) fn wheel(&mut self, dy: f32) {
        self.top = if dy > 0.0 { self.top.saturating_sub(3) } else { (self.top + 3).min(self.nrow.saturating_sub(1)) };
    }

    pub(crate) fn key(&mut self, code: KeyCode) {
        let (nr, nc) = (self.nrow, self.headers.len());
        match code {
            KeyCode::Down => self.top = (self.top + 1).min(nr.saturating_sub(1)),
            KeyCode::Up => self.top = self.top.saturating_sub(1),
            KeyCode::PageDown => self.top = (self.top + self.rows_fit.max(1)).min(nr.saturating_sub(1)),
            KeyCode::PageUp => self.top = self.top.saturating_sub(self.rows_fit.max(1)),
            KeyCode::Home => { self.top = 0; self.left = 0; }
            KeyCode::End => self.top = nr.saturating_sub(1),
            KeyCode::Right => self.left = (self.left + 1).min(nc.saturating_sub(1)),
            KeyCode::Left => self.left = self.left.saturating_sub(1),
            _ => {}
        }
    }

    pub(crate) fn input(&mut self, events: &[InputEvent], wheel_here: bool, keys_here: bool) {
        for ev in events {
            match *ev {
                InputEvent::Scroll { dy } if wheel_here => self.wheel(dy),
                InputEvent::Key { code, pressed: true, .. } if keys_here => self.key(code),
                _ => {}
            }
        }
    }

    pub(crate) fn paint(&mut self, c: Rect, frame: &mut Frame, renderer: &mut Renderer, theme: &Theme) {
        let (cell_w, line_h) = renderer.cell_metrics(theme.fs());
        let (bg, fg, size) = (theme.window_background, theme.console_output, theme.fs());
        frame.paint_rect(c.x, c.y, c.w, c.h, bg);
        let (x0, y0) = (c.x + 6.0, c.y + 4.0);
        let max_x = c.x + c.w - 6.0;
        if let Some(e) = &self.error {
            text(frame, renderer, x0, y0 + line_h * 0.8, e, size, theme.console_error, cell_w, max_x);
            return;
        }
        if self.headers.is_empty() {
            text(frame, renderer, x0, y0 + line_h * 0.8, "asking R2…", size, mix(fg, bg, 0.4), cell_w, max_x);
            return;
        }
        let digits = self.nrow.max(1).to_string().len();
        // wide enough for every row label on screen (car names, not only numbers)
        let labels = (self.top..self.top + self.rows_fit.max(1)).filter_map(|r| self.row(r))
            .map(|r| r.label.chars().count()).max().unwrap_or(0).max(digits).min(28);
        let label_w = (labels.max(2) + SEP) as f32 * cell_w;
        let head_h = 2.0 * line_h + 4.0;
        let foot_h = line_h + 4.0;
        let body_y = y0 + head_h;
        let rows = ((c.h - head_h - foot_h - 8.0) / line_h).floor().max(0.0) as usize;
        self.rows_fit = rows;
        frame.paint_rect(c.x, c.y, c.w, head_h + 4.0, mix(bg, fg, 0.07));
        frame.paint_rect(c.x, body_y, label_w + 6.0, rows as f32 * line_h, mix(bg, fg, 0.04));
        // columns from `left` while they fit
        let mut cols: Vec<(usize, f32)> = Vec::new();
        let mut x = x0 + label_w;
        for col in self.left..self.headers.len() {
            if x >= max_x { break; }
            cols.push((col, x));
            x += (self.widths[col] + SEP) as f32 * cell_w;
        }
        self.geo = Geo {
            head: Rect { x: x0 + label_w, y: c.y, w: (max_x - x0 - label_w).max(0.0), h: head_h + 4.0 },
            body: Rect { x: x0 + label_w, y: body_y, w: (max_x - x0 - label_w).max(0.0), h: rows as f32 * line_h },
            line_h,
            cols: cols.iter().map(|&(col, cx)| (col, cx - cell_w, (self.widths[col] + SEP) as f32 * cell_w)).collect(),
        };
        let place = |s: &str, col: usize, cx: f32| {
            let pad = if self.right[col] { self.widths[col].saturating_sub(s.chars().count()) } else { 0 };
            cx + pad as f32 * cell_w
        };
        for &(col, cx) in &cols {
            let h = &self.headers[col];
            text(frame, renderer, place(h, col, cx), y0 + line_h * 0.8, h, size, theme.console_banner, cell_w, max_x);
            let t = &self.types[col];
            text(frame, renderer, place(t, col, cx), y0 + line_h * 1.8, t, size, mix(fg, bg, 0.5), cell_w, max_x);
        }
        for r in 0..rows {
            let idx = self.top + r;
            if idx >= self.nrow { break; }
            let y = body_y + r as f32 * line_h;
            if idx % 2 == 1 {
                frame.paint_rect(x0 + label_w - cell_w, y, max_x - x0 - label_w + cell_w, line_h, mix(bg, fg, 0.03));
            }
            let Some(row) = self.row(idx) else {
                text(frame, renderer, x0, y + line_h * 0.8, &(idx + 1).to_string(), size, mix(fg, bg, 0.45), cell_w, x0 + label_w);
                continue;
            };
            text(frame, renderer, x0, y + line_h * 0.8, &row.label, size, mix(fg, bg, 0.45), cell_w, x0 + label_w);
            for &(col, cx) in &cols {
                let s = row.cells.get(col).map(String::as_str).unwrap_or("");
                if self.sel == Some((idx, col)) {
                    let w = (self.widths[col] + SEP) as f32 * cell_w;
                    frame.paint_rect(cx - cell_w, y, w, line_h, theme.console_selection_bg);
                    if let Some(t) = &self.typing {
                        // the box grows with what is typed
                        let tw = w.max((t.chars().count() + 3) as f32 * cell_w);
                        frame.paint_rect(cx - cell_w - 1.0, y - 1.0, tw + 2.0, line_h + 2.0, theme.console_banner);
                        frame.paint_rect(cx - cell_w, y, tw, line_h, bg);
                        text(frame, renderer, cx, y + line_h * 0.8, &format!("{t}_"), size, fg, cell_w, max_x);
                    } else {
                        text(frame, renderer, place(s, col, cx), y + line_h * 0.8, s, size, theme.console_selection_fg, cell_w, max_x);
                    }
                    continue;
                }
                let color = if s == "NA" { theme.console_error } else { fg };
                text(frame, renderer, place(s, col, cx), y + line_h * 0.8, s, size, color, cell_w, max_x);
            }
        }
        let last = (self.top + rows).min(self.nrow);
        let how = if self.editable { "click a cell and type to change it; click a variable's name for its menu" } else { "wheel / arrows to move" };
        let status = format!("{}   showing rows {}–{}, columns from {}   ({how})",
            self.shape, if self.nrow == 0 { 0 } else { self.top + 1 }, last, self.left + 1);
        let sy = c.y + c.h - foot_h;
        frame.paint_rect(c.x, sy, c.w, foot_h, mix(bg, fg, 0.06));
        text(frame, renderer, x0, sy + 2.0 + line_h * 0.8, &status, size, mix(fg, bg, 0.3), cell_w, max_x);
    }
}

impl Gui {
    /// Register a new view; its id stays valid until `drop_view`.
    pub(crate) fn new_view(&mut self, code: &str) -> u64 {
        self.next_view += 1;
        self.views.insert(self.next_view, DataView::new(code));
        self.next_view
    }

    /// Ask R2 for the pages view `uid` needs on screen.
    pub(crate) fn fetch_view_pages(&mut self, uid: u64) {
        let Some(v) = self.views.get_mut(&uid) else { return };
        if v.error.is_some() { return; }
        let wanted = v.wanted();
        for page in &wanted { v.asked.insert((*page, v.gen)); }
        let (code, gen) = (v.code.clone(), v.gen);
        for page in wanted {
            self.session.send("view", json!({ "code": code, "offset": page * PAGE, "limit": PAGE }),
                              Req::ViewPage { uid, page, gen });
        }
    }

    pub(crate) fn view_reply(&mut self, uid: u64, page: usize, gen: u64, body: &Value) {
        let Some(v) = self.views.get_mut(&uid) else { return };
        if body["ok"] != Value::Bool(true) {
            v.error = Some(body["error"].as_str().unwrap_or("R2 could not show this").to_string());
            return;
        }
        let strings = |k: &str| -> Vec<String> {
            body[k].as_array().map(|a| a.iter().map(|s| s.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default()
        };
        v.error = None;
        v.shape = body["shape"].as_str().unwrap_or("").to_string();
        v.nrow = body["nrow"].as_u64().unwrap_or(0) as usize;
        v.headers = strings("headers");
        v.types = strings("types");
        v.widths = body["widths"].as_array().map(|a| a.iter().map(|w| w.as_u64().unwrap_or(3) as usize).collect()).unwrap_or_default();
        v.right = body["right"].as_array().map(|a| a.iter().map(|b| b.as_bool().unwrap_or(false)).collect()).unwrap_or_default();
        let rows: Vec<Row> = body["rows"].as_array().map(|a| a.iter().map(|r| Row {
            label: r["label"].as_str().unwrap_or("").to_string(),
            cells: r["cells"].as_array().map(|c| c.iter().map(|s| s.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default(),
        }).collect()).unwrap_or_default();
        v.pages.insert(page, (gen, rows));
        v.top = v.top.min(v.nrow.saturating_sub(1));
        if v.sel.is_some_and(|(r, c)| r >= v.nrow || c >= v.headers.len()) { v.sel = None; }
    }
}
