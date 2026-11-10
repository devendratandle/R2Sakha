//! The data viewer: a page-at-a-time grid over `r2 --ide`'s `view` request.
//! A frozen header (column name, and its type underneath), a frozen row-label
//! column, then the cells, formatted by R2 as `print()` formats them. Only the
//! pages on screen are asked for, so a million-row frame opens at once. Used
//! by the Studio's Source tabs and by Sakha's Data tab.

use std::collections::{HashMap, HashSet};

use r2_ui::{Frame, InputEvent, KeyCode, Rect, Renderer, Theme};
use sakha_engine::{json, Value};

use crate::session::Req;
use crate::ui::{mix, text};
use crate::Gui;

/// Rows in one page asked of R2.
const PAGE: usize = 200;
/// Gap between columns, in cells.
const SEP: usize = 2;

struct Row { label: String, cells: Vec<String> }

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
}

impl DataView {
    pub(crate) fn new(code: &str) -> DataView {
        DataView { code: code.to_string(), shape: String::new(), nrow: 0, headers: vec![], types: vec![], widths: vec![],
                   right: vec![], pages: HashMap::new(), gen: 0, asked: HashSet::new(), error: None, top: 0, left: 0, rows_fit: 40 }
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
        let labels = self.row(self.top).map_or(digits, |r| r.label.chars().count().max(digits));
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
                let color = if s == "NA" { theme.console_error } else { fg };
                text(frame, renderer, place(s, col, cx), y + line_h * 0.8, s, size, color, cell_w, max_x);
            }
        }
        let last = (self.top + rows).min(self.nrow);
        let status = format!("{}   showing rows {}–{}, columns from {}   (wheel / arrows to move)",
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
    }
}
