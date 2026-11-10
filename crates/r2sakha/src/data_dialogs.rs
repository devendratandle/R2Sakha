//! The Data menu's dialogs, in both faces, on the lines of Rcmdr's data menu:
//! a list to choose from (where a new dataset comes from, R2's datasets however
//! many, the data already in the session), importing a text file, and a
//! variable's name and type. A loaded dataset is edited only through these
//! attributes, never cell by cell. Each change is R2 code, kept in the script.

use std::path::{Path, PathBuf};

use r2_ui::{Frame, InputEvent, KeyCode, MouseButton, Rect, Renderer, Theme};
use sakha_core::document::r_path;
use sakha_core::import::{clean_name, dataset_names, guess_separator, import_line, parquet_line, rename_code,
                         rename_problem, to_category_code, to_number_code, Sep, DATASETS_HELD_BACK};
use sakha_core::profile::Kind;
use sakha_engine::Value;

use crate::session::{After, Req};
use crate::ui::{button, inside, mix, text};
use crate::Gui;

/// Lines of a file shown before importing it.
const PREVIEW: usize = 6;

/// A box in the middle of the workspace, over a dimmed background.
fn dialog_box(area: Rect, cols: f32, rows: f32, frame: &mut Frame, renderer: &mut Renderer, theme: &Theme, title: &str) -> Rect {
    let (cw, lh) = renderer.cell_metrics(theme.fs());
    let (bg, fg) = (theme.window_background, theme.console_output);
    frame.paint_rect(area.x, area.y, area.w, area.h, r2_ui::Color::rgba(0, 0, 0, 120));
    let (w, h) = ((cols * cw).min(area.w - 20.0), (rows * lh).min(area.h - 20.0));
    let b = Rect { x: area.x + (area.w - w) / 2.0, y: area.y + (area.h - h) / 2.0, w, h };
    frame.paint_rect(b.x - 1.0, b.y - 1.0, b.w + 2.0, b.h + 2.0, mix(bg, fg, 0.3));
    frame.paint_rect(b.x, b.y, b.w, b.h, bg);
    frame.paint_rect(b.x, b.y, b.w, lh + 10.0, mix(bg, fg, 0.1));
    text(frame, renderer, b.x + 10.0, b.y + 5.0 + lh * 0.8, title, theme.fs(), theme.console_banner, cw, b.x + b.w);
    b
}

/// A one-line text box; the caret is a trailing `_` while it has the keyboard.
#[allow(clippy::too_many_arguments)]
fn text_box(r: Rect, s: &str, active: bool, frame: &mut Frame, renderer: &mut Renderer, theme: &Theme) {
    let (cw, lh) = renderer.cell_metrics(theme.fs());
    let (bg, fg) = (theme.window_background, theme.console_output);
    frame.paint_rect(r.x, r.y, r.w, r.h, mix(bg, fg, if active { 0.16 } else { 0.08 }));
    let shown = if active { format!("{s}_") } else { s.to_string() };
    text(frame, renderer, r.x + 6.0, r.y + (r.h - lh) / 2.0 + lh * 0.8, &shown, theme.fs(), fg, cw, r.x + r.w - 4.0);
}

fn typed(buf: &mut String, ev: &InputEvent) {
    match *ev {
        InputEvent::Char(c) if !c.is_control() => buf.push(c),
        InputEvent::Key { code: KeyCode::Backspace, pressed: true, .. } => { buf.pop(); }
        _ => {}
    }
}

/// A list to choose from, narrowed by typing: fit for 3 entries or for 100.
pub(crate) struct ListDialog {
    pub(crate) title: String,
    /// What the list is, shown above it.
    pub(crate) hint: String,
    /// (label, detail, action).
    pub(crate) items: Vec<(String, String, String)>,
    filter: String,
    scroll: usize,
    rows: Vec<(Rect, String)>,
    cancel: Option<Rect>,
    list: Option<Rect>,
}

impl ListDialog {
    pub(crate) fn new(title: &str, hint: &str, items: Vec<(String, String, String)>) -> ListDialog {
        ListDialog { title: title.into(), hint: hint.into(), items, filter: String::new(), scroll: 0, rows: vec![], cancel: None, list: None }
    }

    fn shown(&self) -> Vec<&(String, String, String)> {
        let f = self.filter.to_lowercase();
        self.items.iter().filter(|(l, d, _)| f.is_empty() || l.to_lowercase().contains(&f) || d.to_lowercase().contains(&f)).collect()
    }

    fn paint(&mut self, area: Rect, frame: &mut Frame, renderer: &mut Renderer, theme: &Theme) {
        let (cw, lh) = renderer.cell_metrics(theme.fs());
        let (bg, fg, size) = (theme.window_background, theme.console_output, theme.fs());
        let title = self.title.clone();
        let b = dialog_box(area, 64.0, 24.0, frame, renderer, theme, &title);
        let mut y = b.y + lh + 16.0;
        text(frame, renderer, b.x + 10.0, y + lh * 0.8, &self.hint, size, mix(fg, bg, 0.35), cw, b.x + b.w - 8.0);
        y += lh + 6.0;
        text_box(Rect { x: b.x + 10.0, y, w: b.w - 20.0, h: lh + 6.0 }, &self.filter, true, frame, renderer, theme);
        y += lh + 12.0;
        let foot = lh + 16.0;
        let list = Rect { x: b.x + 10.0, y, w: b.w - 20.0, h: (b.y + b.h - foot - y).max(0.0) };
        frame.paint_rect(list.x, list.y, list.w, list.h, mix(bg, fg, 0.04));
        self.list = Some(list);
        let row_h = lh + 4.0;
        let fit = (list.h / row_h).floor().max(1.0) as usize;
        let items: Vec<(String, String, String)> = self.shown().into_iter().cloned().collect();
        self.scroll = self.scroll.min(items.len().saturating_sub(fit));
        self.rows.clear();
        if items.is_empty() {
            text(frame, renderer, list.x + 6.0, list.y + lh * 0.9, "Nothing matches.", size, mix(fg, bg, 0.45), cw, list.x + list.w);
        }
        for (k, (label, detail, action)) in items.iter().skip(self.scroll).take(fit).enumerate() {
            let r = Rect { x: list.x, y: list.y + k as f32 * row_h, w: list.w, h: row_h };
            text(frame, renderer, r.x + 6.0, r.y + 2.0 + lh * 0.8, label, size, theme.console_input, cw, r.x + r.w);
            let dx = r.x + 6.0 + (label.chars().count() + 3) as f32 * cw;
            text(frame, renderer, dx, r.y + 2.0 + lh * 0.8, detail, size, mix(fg, bg, 0.4), cw, r.x + r.w);
            self.rows.push((r, action.clone()));
        }
        let c = Rect { x: b.x + b.w - 10.0 * cw - 10.0, y: b.y + b.h - foot + 6.0, w: 10.0 * cw, h: lh + 6.0 };
        button(frame, renderer, theme, c, "Cancel", false);
        self.cancel = Some(c);
    }
}

/// Importing a text file: the file's first lines, and how to read them.
pub(crate) struct ImportDialog {
    path: PathBuf,
    /// The path as the R2 line will say it.
    r_path: String,
    name: String,
    header: bool,
    sep: Sep,
    preview: Vec<String>,
    taken: Vec<String>,
    header_rect: Option<Rect>,
    sep_rects: Vec<(Rect, Sep)>,
    run: Option<Rect>,
    cancel: Option<Rect>,
}

/// A variable's name and type.
pub(crate) struct AttributesDialog {
    /// The column being edited.
    col: usize,
    name: String,
    kind: Kind,
    message: String,
    rows: Vec<(Rect, usize)>,
    type_rects: Vec<(Rect, Kind)>,
    apply: Option<Rect>,
    close: Option<Rect>,
}

enum Choice { Stay, Close, Run }

impl AttributesDialog {
    /// R2 made the change.
    pub(crate) fn applied(&mut self) { self.message = "Done.".into(); }
}

impl Gui {
    /// File ▸ Load new dataset…: every source, in one place.
    pub(crate) fn open_load_chooser(&mut self) {
        let entry = |l: &str, d: &str, a: &str| (l.to_string(), d.to_string(), a.to_string());
        let mut items = vec![
            entry("Text file", "CSV, tab, semicolon or space separated", "data.import"),
            entry("Parquet file", "a columnar data file", "data.import_parquet"),
            entry("R2 data file", ".r2s, saved with save()", "data.load_r2s"),
            entry("R2 datasets", "the examples that come with R2", "data.r2_list"),
        ];
        if self.env.rows.iter().any(|r| r.is_data_frame()) {
            items.push(entry("In this session", "data loaded here or in Pro", "data.select"));
        }
        self.list_dialog = Some(ListDialog::new("Load new dataset", "Where is the data?", items));
    }

    /// Data ▸ Select dataset…: what is already loaded, wherever it was loaded.
    pub(crate) fn open_select_dialog(&mut self) {
        let sakha = self.sakha_mode;
        let items: Vec<_> = self.env.rows.iter().filter(|r| r.is_data_frame()).map(|r| {
            let action = if sakha { format!("sakha.df:{}", r.name) } else { format!("data.view:{}", r.name) };
            (r.name.clone(), r.summary.clone(), action)
        }).collect();
        self.list_dialog = Some(ListDialog::new("Select dataset", "Data frames in this session, from Pro or Sakha:", items));
    }

    /// Data ▸ R2 datasets…: ask R2 what it has, so the menu never needs to change.
    pub(crate) fn open_r2_datasets(&mut self) {
        self.list_dialog = Some(ListDialog::new("R2 datasets", "Examples for learning. Type to narrow the list:", vec![]));
        self.session.send("eval", sakha_engine::json!({ "code": "data()" }), Req::DatasetList);
    }

    /// What `data()` printed: the list, then each one's size.
    pub(crate) fn dataset_list_text(&mut self, printed: &str) {
        let names: Vec<String> = dataset_names(printed).into_iter()
            .filter(|n| !DATASETS_HELD_BACK.contains(&n.as_str())).collect();
        if let Some(d) = self.list_dialog.as_mut() {
            d.items = names.iter().map(|n| (n.clone(), String::new(), format!("data.builtin:{n}"))).collect();
        }
        for n in names { self.session.value(&format!("dim({n})"), Req::DatasetSize { name: n.clone() }); }
    }

    pub(crate) fn dataset_size_reply(&mut self, name: &str, body: &Value) {
        let dims = sakha_core::profile::reals(&body["value"]);
        if let (Some(d), [rows, cols, ..]) = (self.list_dialog.as_mut(), dims.as_slice()) {
            if let Some(item) = d.items.iter_mut().find(|i| i.0 == name) {
                item.1 = format!("{} rows, {} variables", *rows as u64, *cols as u64);
            }
        }
    }

    /// Open the import dialog on a chosen file, guessing how it is laid out.
    pub(crate) fn open_import_dialog(&mut self, path: &Path) {
        let content = std::fs::read_to_string(path);
        let preview: Vec<String> = content.as_deref().unwrap_or("").lines().take(PREVIEW).map(str::to_string).collect();
        let sep = guess_separator(preview.first().map_or("", String::as_str));
        let stem = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "data".into());
        self.import_dialog = Some(ImportDialog {
            path: path.to_path_buf(),
            r_path: r_path(path.strip_prefix(&self.session.cwd).unwrap_or(path)),
            name: clean_name(&stem),
            header: true,
            sep,
            preview,
            taken: self.env.rows.iter().map(|r| r.name.clone()).collect(),
            header_rect: None, sep_rects: vec![], run: None, cancel: None,
        });
    }

    /// A Parquet file has its own structure: nothing to ask.
    pub(crate) fn import_parquet(&mut self, path: &Path) {
        let stem = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "data".into());
        let name = clean_name(&stem);
        let p = r_path(path.strip_prefix(&self.session.cwd).unwrap_or(path));
        self.run_load(parquet_line(&name, &p), name);
    }

    /// A line that makes the data frame `name`, then show it in the face in front.
    pub(crate) fn run_load(&mut self, line: String, name: String) {
        let after = if self.sakha_mode { After::OpenInSakha { df: name, load: line.clone() } } else { After::ViewData { df: name } };
        self.run_queue.push_back((line, Some(after)));
    }

    /// Data ▸ Variable names and types….
    pub(crate) fn open_attributes(&mut self) {
        if self.sakha.df.is_none() || self.sakha.cols.is_empty() {
            self.sakha.choosing = true;
            self.sakha.status = "First load a dataset.".into();
            return;
        }
        let col = self.sakha.selected.unwrap_or(0);
        let c = &self.sakha.cols[col];
        self.attr_dialog = Some(AttributesDialog { col, name: c.name.clone(), kind: c.kind, message: String::new(),
                                                   rows: vec![], type_rects: vec![], apply: None, close: None });
    }

    fn apply_attributes(&mut self) {
        let Some(df) = self.sakha.df.clone() else { return };
        let Some(d) = self.attr_dialog.as_mut() else { return };
        let Some(c) = self.sakha.cols.get(d.col) else { return };
        let (mut code, mut title) = (Vec::new(), Vec::new());
        // the type first, under the old name
        if d.kind != c.kind {
            let line = if d.kind == Kind::Number { to_number_code(&df, &c.name) } else { to_category_code(&df, &c.name) };
            title.push(format!("{} as a {}", c.name, d.kind.label()));
            code.push(line);
        }
        if d.name != c.name {
            let others: Vec<&str> = self.sakha.cols.iter().map(|x| x.name.as_str()).collect();
            if let Some(problem) = rename_problem(&d.name, &others) {
                d.message = problem;
                return;
            }
            title.push(format!("{} renamed {}", c.name, d.name));
            code.push(rename_code(&df, d.col, &d.name));
        }
        if code.is_empty() {
            d.message = "Nothing has changed.".into();
            return;
        }
        // kept with the steps that rebuild the data on reopening; each change is its own step
        let n = self.sakha.script.steps();
        self.sakha.script.record(&format!("cat:{n}"), &title.join("; "), &code);
        self.sakha.dirty = true;
        self.sakha.reselect = Some(d.name.clone());
        d.message = "Applied. R2 is updating the variable.".into();
        for line in code { self.run_queue.push_back((line, Some(After::ColumnChange))); }
    }

    /// Is one of these dialogs open? It then owns the input.
    pub(crate) fn data_dialog_open(&self) -> bool {
        self.list_dialog.is_some() || self.import_dialog.is_some() || self.attr_dialog.is_some()
    }

    pub(crate) fn data_dialog_input(&mut self, events: &[InputEvent]) {
        let mouse = self.mouse;
        if let Some(d) = self.list_dialog.as_mut() {
            let mut picked: Option<String> = None;
            let mut close = false;
            for ev in events {
                typed(&mut d.filter, ev);
                match *ev {
                    InputEvent::Key { code: KeyCode::Escape, pressed: true, .. } => close = true,
                    InputEvent::Key { code: KeyCode::Enter, pressed: true, .. } => {
                        // Enter takes the only match left
                        let shown = d.shown();
                        if shown.len() == 1 { picked = Some(shown[0].2.clone()); }
                    }
                    InputEvent::Scroll { dy } if d.list.is_some_and(|r| inside(r, mouse.0, mouse.1)) => {
                        d.scroll = if dy > 0.0 { d.scroll.saturating_sub(2) } else { d.scroll + 2 };
                    }
                    InputEvent::MouseDown { button: MouseButton::Left, pos } => {
                        if d.cancel.is_some_and(|r| inside(r, pos.x, pos.y)) { close = true; }
                        else if let Some((_, a)) = d.rows.iter().find(|(r, _)| inside(*r, pos.x, pos.y)) { picked = Some(a.clone()); }
                    }
                    _ => {}
                }
            }
            if close || picked.is_some() { self.list_dialog = None; }
            if picked.is_some() { self.queued = picked; }
            return;
        }
        if let Some(d) = self.import_dialog.as_mut() {
            let mut choice = Choice::Stay;
            for ev in events {
                typed(&mut d.name, ev);
                match *ev {
                    InputEvent::Key { code: KeyCode::Escape, pressed: true, .. } => choice = Choice::Close,
                    InputEvent::Key { code: KeyCode::Enter, pressed: true, .. } => choice = Choice::Run,
                    InputEvent::MouseDown { button: MouseButton::Left, pos } => {
                        let hit = |r: &Option<Rect>| r.is_some_and(|r| inside(r, pos.x, pos.y));
                        if hit(&d.cancel) { choice = Choice::Close; }
                        else if hit(&d.run) { choice = Choice::Run; }
                        else if hit(&d.header_rect) { d.header = !d.header; }
                        else if let Some((_, s)) = d.sep_rects.iter().find(|(r, _)| inside(*r, pos.x, pos.y)) { d.sep = *s; }
                    }
                    _ => {}
                }
            }
            match choice {
                Choice::Stay => {}
                Choice::Close => self.import_dialog = None,
                Choice::Run => {
                    let name = clean_name(&d.name);
                    let line = import_line(&name, &d.r_path, d.header, d.sep);
                    self.import_dialog = None;
                    self.run_load(line, name);
                }
            }
            return;
        }
        let cols: Vec<(String, Kind)> = self.sakha.cols.iter().map(|c| (c.name.clone(), c.kind)).collect();
        if let Some(d) = self.attr_dialog.as_mut() {
            let mut choice = Choice::Stay;
            for ev in events {
                let before = d.name.len();
                typed(&mut d.name, ev);
                if d.name.len() != before { d.message.clear(); }
                match *ev {
                    InputEvent::Key { code: KeyCode::Escape, pressed: true, .. } => choice = Choice::Close,
                    InputEvent::Key { code: KeyCode::Enter, pressed: true, .. } => choice = Choice::Run,
                    InputEvent::MouseDown { button: MouseButton::Left, pos } => {
                        let hit = |r: &Option<Rect>| r.is_some_and(|r| inside(r, pos.x, pos.y));
                        if hit(&d.close) { choice = Choice::Close; }
                        else if hit(&d.apply) { choice = Choice::Run; }
                        else if let Some((_, k)) = d.type_rects.iter().find(|(r, _)| inside(*r, pos.x, pos.y)) { d.kind = *k; d.message.clear(); }
                        else if let Some(&(_, i)) = d.rows.iter().find(|(r, _)| inside(*r, pos.x, pos.y)) {
                            if let Some((n, k)) = cols.get(i) {
                                d.col = i;
                                d.name = n.clone();
                                d.kind = *k;
                                d.message.clear();
                            }
                        }
                    }
                    _ => {}
                }
            }
            match choice {
                Choice::Stay => {}
                Choice::Close => self.attr_dialog = None,
                Choice::Run => self.apply_attributes(),
            }
        }
    }

    pub(crate) fn paint_data_dialogs(&mut self, area: Rect, frame: &mut Frame, renderer: &mut Renderer, theme: &Theme) {
        if let Some(d) = self.list_dialog.as_mut() { d.paint(area, frame, renderer, theme); }
        if let Some(d) = self.import_dialog.as_mut() { paint_import(d, area, frame, renderer, theme); }
        let cols: Vec<(String, Kind)> = self.sakha.cols.iter().map(|c| (c.name.clone(), c.kind)).collect();
        if let Some(d) = self.attr_dialog.as_mut() { paint_attributes(d, &cols, area, frame, renderer, theme); }
    }
}

fn paint_import(d: &mut ImportDialog, area: Rect, frame: &mut Frame, renderer: &mut Renderer, theme: &Theme) {
    let (cw, lh) = renderer.cell_metrics(theme.fs());
    let (bg, fg, size) = (theme.window_background, theme.console_output, theme.fs());
    let dim = mix(fg, bg, 0.4);
    let b = dialog_box(area, 76.0, 24.0, frame, renderer, theme, "Import text file");
    let x = b.x + 10.0;
    let mut y = b.y + lh + 16.0;
    let shown = d.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    text(frame, renderer, x, y + lh * 0.8, &format!("File: {shown}"), size, fg, cw, b.x + b.w - 8.0);
    y += lh + 8.0;
    // the name
    text(frame, renderer, x, y + 3.0 + lh * 0.8, "Name in R2", size, dim, cw, x + 14.0 * cw);
    text_box(Rect { x: x + 14.0 * cw, y, w: 30.0 * cw, h: lh + 6.0 }, &d.name, true, frame, renderer, theme);
    let name = clean_name(&d.name);
    let note = if d.taken.contains(&name) { format!("replaces the {name} in this session") }
               else if name != d.name { format!("will be {name}") } else { String::new() };
    text(frame, renderer, x + 45.0 * cw, y + 3.0 + lh * 0.8, &note, size, theme.console_error, cw, b.x + b.w - 8.0);
    y += lh + 12.0;
    // the header
    let h = Rect { x: x + 14.0 * cw, y, w: 30.0 * cw, h: lh + 6.0 };
    text(frame, renderer, x, y + 3.0 + lh * 0.8, "First line", size, dim, cw, x + 14.0 * cw);
    button(frame, renderer, theme, h, if d.header { "holds variable names" } else { "is data" }, d.header);
    d.header_rect = Some(h);
    y += lh + 12.0;
    // the separator
    text(frame, renderer, x, y + 3.0 + lh * 0.8, "Separated by", size, dim, cw, x + 14.0 * cw);
    d.sep_rects.clear();
    for (k, s) in Sep::ALL.iter().enumerate() {
        let r = Rect { x: x + 14.0 * cw + k as f32 * (12.0 * cw + 4.0), y, w: 12.0 * cw, h: lh + 6.0 };
        button(frame, renderer, theme, r, s.label(), *s == d.sep);
        d.sep_rects.push((r, *s));
    }
    y += lh + 14.0;
    // the file itself, to compare against the choices
    text(frame, renderer, x, y + lh * 0.8, "The file begins:", size, dim, cw, b.x + b.w);
    y += lh + 2.0;
    let foot = lh + 16.0;
    let view = Rect { x, y, w: b.w - 20.0, h: (b.y + b.h - foot - y).max(0.0) };
    frame.paint_rect(view.x, view.y, view.w, view.h, mix(bg, fg, 0.04));
    let fit = ((view.h - 4.0) / lh).floor().max(0.0) as usize;
    for (k, l) in d.preview.iter().take(fit).enumerate() {
        text(frame, renderer, view.x + 6.0, view.y + 2.0 + lh * (0.8 + k as f32), &l.replace('\t', "    "), size, fg, cw, view.x + view.w - 4.0);
    }
    if d.preview.is_empty() {
        text(frame, renderer, view.x + 6.0, view.y + 2.0 + lh * 0.8, "This file could not be read as text.", size, theme.console_error, cw, view.x + view.w);
    }
    let fy = b.y + b.h - foot + 6.0;
    let c = Rect { x: b.x + b.w - 10.0 * cw - 10.0, y: fy, w: 10.0 * cw, h: lh + 6.0 };
    let r = Rect { x: c.x - 10.0 * cw - 8.0, y: fy, w: 10.0 * cw, h: lh + 6.0 };
    button(frame, renderer, theme, c, "Cancel", false);
    button(frame, renderer, theme, r, "Import", true);
    (d.cancel, d.run) = (Some(c), Some(r));
}

fn paint_attributes(d: &mut AttributesDialog, cols: &[(String, Kind)], area: Rect, frame: &mut Frame, renderer: &mut Renderer, theme: &Theme) {
    let (cw, lh) = renderer.cell_metrics(theme.fs());
    let (bg, fg, size) = (theme.window_background, theme.console_output, theme.fs());
    let dim = mix(fg, bg, 0.4);
    let b = dialog_box(area, 76.0, 24.0, frame, renderer, theme, "Variable names and types");
    let foot = lh + 16.0;
    let top = b.y + lh + 16.0;
    let list = Rect { x: b.x + 10.0, y: top, w: 28.0 * cw, h: (b.y + b.h - foot - top).max(0.0) };
    frame.paint_rect(list.x, list.y, list.w, list.h, mix(bg, fg, 0.04));
    d.rows.clear();
    let fit = (list.h / lh).floor().max(1.0) as usize;
    // keep the chosen variable in the visible part
    let first = (d.col + 1).saturating_sub(fit);
    for (row, (i, (name, _))) in cols.iter().enumerate().skip(first).take(fit).enumerate() {
        let r = Rect { x: list.x, y: list.y + row as f32 * lh, w: list.w, h: lh };
        if i == d.col { frame.paint_rect(r.x, r.y, r.w, r.h, theme.console_selection_bg); }
        text(frame, renderer, r.x + 6.0, r.y + lh * 0.8, name, size, if i == d.col { theme.console_selection_fg } else { fg }, cw, r.x + r.w);
        d.rows.push((r, i));
    }
    let x = list.x + list.w + 14.0;
    let mut y = top;
    text(frame, renderer, x, y + 3.0 + lh * 0.8, "Name", size, dim, cw, x + 8.0 * cw);
    text_box(Rect { x: x + 8.0 * cw, y, w: 30.0 * cw, h: lh + 6.0 }, &d.name, true, frame, renderer, theme);
    y += lh + 14.0;
    text(frame, renderer, x, y + 3.0 + lh * 0.8, "Type", size, dim, cw, x + 8.0 * cw);
    d.type_rects.clear();
    for (k, t) in [Kind::Number, Kind::Category].iter().enumerate() {
        let r = Rect { x: x + 8.0 * cw + k as f32 * (14.0 * cw + 4.0), y, w: 14.0 * cw, h: lh + 6.0 };
        button(frame, renderer, theme, r, t.label(), *t == d.kind);
        d.type_rects.push((r, *t));
    }
    y += lh + 16.0;
    let note = "The data's values are never edited here. Text that is not a number becomes NA when changed to a number. Every change is kept in the session script.";
    for line in crate::ui::wrap(note, ((b.x + b.w - x - 8.0) / cw) as usize) {
        text(frame, renderer, x, y + lh * 0.8, &line, size, dim, cw, b.x + b.w - 8.0);
        y += lh;
    }
    let fy = b.y + b.h - foot + 6.0;
    let c = Rect { x: b.x + b.w - 10.0 * cw - 10.0, y: fy, w: 10.0 * cw, h: lh + 6.0 };
    let r = Rect { x: c.x - 10.0 * cw - 8.0, y: fy, w: 10.0 * cw, h: lh + 6.0 };
    button(frame, renderer, theme, c, "Close", false);
    button(frame, renderer, theme, r, "Apply", true);
    (d.close, d.apply) = (Some(c), Some(r));
    text(frame, renderer, x, fy + 3.0 + lh * 0.8, &d.message, size, theme.console_error, cw, r.x - 6.0);
}
