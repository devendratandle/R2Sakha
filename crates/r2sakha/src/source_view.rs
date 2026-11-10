//! Pro's Source pane: tabs of scripts and data viewers, as in RStudio.
//!
//! A script tab is an `r2_editor::Editor` painted as cells, with a line-number
//! gutter, syntax colours, a selection band and a caret. All editing decisions
//! live in `r2-editor` (tested headless); this file only maps keys and mouse
//! to its methods, keeps the caret in view, and paints. Code the user runs
//! goes to the console line by line through `Gui::submit`, exactly as if typed
//! there, so the transcript shows it and `+` continuations work unchanged.

use r2_editor::highlight::{highlight_line, line_states, Kind};
use r2_editor::{Editor, Pos};
use r2_ui::{Color, Frame, FrameCtx, InputEvent, KeyCode, MouseButton, Rect, Renderer, Theme};

use crate::ui::{button_left_of, inside, mix, tabs};
use crate::Gui;

pub(crate) struct ScriptTab {
    pub(crate) ed: Editor,
    /// First visible line and column.
    top: usize,
    left: usize,
    /// A left-button drag is extending the selection.
    dragging: bool,
    /// Lines that fit, from the last paint — PgUp/PgDn move by this.
    page: usize,
    /// Closing an unsaved script takes a second click on its ×.
    close_armed: bool,
}

pub(crate) enum Tab {
    Script(ScriptTab),
    /// A data viewer: the view's id and the expression shown.
    Data(u64, String),
}

/// The Source pane's state, and last paint's click targets.
#[derive(Default)]
pub(crate) struct SourcePane {
    pub(crate) tabs: Vec<Tab>,
    pub(crate) active: usize,
    tab_rects: Vec<Rect>,
    close_rects: Vec<Rect>,
    run: Option<Rect>,
    run_all: Option<Rect>,
    save: Option<Rect>,
}

/// Inner margin of the text area, px.
const PAD: f32 = 6.0;

/// Syntax colours, chosen for contrast against the window background.
fn palette(theme: &Theme) -> [(Kind, Color); 7] {
    let bg = theme.window_background;
    let dark = (bg.0 as u32 + bg.1 as u32 + bg.2 as u32) < 384;
    if dark {
        [(Kind::Keyword, Color::rgb(198, 120, 221)), (Kind::Constant, Color::rgb(209, 154, 102)),
         (Kind::Str, Color::rgb(152, 195, 121)), (Kind::Number, Color::rgb(209, 154, 102)),
         (Kind::Comment, Color::rgb(110, 118, 129)), (Kind::Operator, Color::rgb(86, 182, 194)),
         (Kind::Call, Color::rgb(97, 175, 239))]
    } else {
        [(Kind::Keyword, Color::rgb(152, 24, 147)), (Kind::Constant, Color::rgb(170, 85, 0)),
         (Kind::Str, Color::rgb(28, 126, 40)), (Kind::Number, Color::rgb(170, 85, 0)),
         (Kind::Comment, Color::rgb(120, 120, 120)), (Kind::Operator, Color::rgb(0, 112, 140)),
         (Kind::Call, Color::rgb(30, 90, 200))]
    }
}

/// The tab strip's height.
fn strip_h(line_h: f32) -> f32 { line_h + 8.0 }

impl ScriptTab {
    /// One frame's keys, clicks and wheel on the editor painted in `body`.
    /// `hover`: the mouse is over it (the wheel scrolls it); `keys`: it has
    /// the keyboard. Commands it cannot do itself (run, copy, save…) are
    /// pushed onto `actions` as `script.*`.
    pub(crate) fn input(&mut self, events: &[InputEvent], body: Rect, hover: bool, keys: bool, (cell_w, line_h): (f32, f32),
                        actions: &mut Vec<&'static str>) {
        let s = self;
        let text = s.text_rect(body, cell_w);
        let rows = (text.h / line_h).floor() as usize;
        let cols = (text.w / cell_w).floor() as usize;
        for ev in events {
            match *ev {
                InputEvent::Scroll { dy } if hover => {
                    let step = (dy.abs().ceil() as usize).max(1) * 3;
                    let max_top = s.ed.buffer().line_count().saturating_sub(1);
                    s.top = if dy > 0.0 { s.top.saturating_sub(step) } else { (s.top + step).min(max_top) };
                    continue;
                }
                InputEvent::Char(ch) if keys => s.ed.type_char(ch),
                InputEvent::Key { code, mods, pressed: true } if keys => {
                    let (sh, c) = (mods.shift, mods.ctrl);
                    match code {
                        KeyCode::Enter if c && sh => actions.push("script.run_all"),
                        KeyCode::Enter if c => actions.push("script.run"),
                        KeyCode::Enter => s.ed.newline(),
                        KeyCode::Backspace => s.ed.backspace(),
                        KeyCode::Delete => s.ed.delete(),
                        KeyCode::Tab => s.ed.tab(sh),
                        KeyCode::Escape => s.ed.anchor = None,
                        KeyCode::Left => s.ed.left(sh, c),
                        KeyCode::Right => s.ed.right(sh, c),
                        KeyCode::Up => s.ed.vertical(-1, sh),
                        KeyCode::Down => s.ed.vertical(1, sh),
                        KeyCode::PageUp => s.ed.vertical(-(s.page as isize), sh),
                        KeyCode::PageDown => s.ed.vertical(s.page as isize, sh),
                        KeyCode::Home => s.ed.home(sh, c),
                        KeyCode::End => s.ed.end(sh, c),
                        KeyCode::KeyA if c => s.ed.select_all(),
                        KeyCode::KeyC if c => actions.push("script.copy"),
                        KeyCode::KeyX if c => actions.push("script.cut"),
                        KeyCode::KeyV if c => actions.push("script.paste"),
                        KeyCode::KeyZ if c => s.ed.undo(),
                        KeyCode::KeyY if c => s.ed.redo(),
                        KeyCode::KeyS if c => actions.push("script.save"),
                        KeyCode::KeyO if c => actions.push("script.open"),
                        KeyCode::KeyN if c => actions.push("script.new"),
                        _ => {}
                    }
                }
                InputEvent::MouseDown { button: MouseButton::Left, pos } if inside(body, pos.x, pos.y) => {
                    let at = s.hit(text, pos.x, pos.y, cell_w, line_h);
                    s.ed.set_cursor(at, false);
                    s.dragging = true;
                }
                InputEvent::MouseMoved(pos) if s.dragging => {
                    let at = s.hit(text, pos.x, pos.y, cell_w, line_h);
                    s.ed.set_cursor(at, true);
                }
                InputEvent::MouseUp { button: MouseButton::Left, .. } => { s.dragging = false; continue; }
                _ => continue,
            }
            s.close_armed = false;
            s.follow_cursor(rows, cols);
        }
    }

    pub(crate) fn new(ed: Editor) -> ScriptTab {
        ScriptTab { ed, top: 0, left: 0, dragging: false, page: 20, close_armed: false }
    }

    /// Gutter width in cells: the line-number digits plus a space.
    fn gutter_cells(&self) -> usize {
        self.ed.buffer().line_count().to_string().len().max(3) + 1
    }

    /// The text area (right of the gutter) inside the tab's body rect.
    fn text_rect(&self, body: Rect, cell_w: f32) -> Rect {
        let g = self.gutter_cells() as f32 * cell_w + PAD;
        Rect { x: body.x + g + PAD, y: body.y + PAD,
               w: (body.w - g - 2.0 * PAD).max(0.0), h: (body.h - 2.0 * PAD).max(0.0) }
    }

    fn hit(&self, text: Rect, x: f32, y: f32, cell_w: f32, line_h: f32) -> Pos {
        let row = ((y - text.y) / line_h).floor().max(0.0) as usize + self.top;
        let col = ((x - text.x) / cell_w).round().max(0.0) as usize + self.left;
        self.ed.buffer().clamp(Pos::new(row, col))
    }

    /// Scroll just enough to keep the caret on screen.
    fn follow_cursor(&mut self, rows: usize, cols: usize) {
        let c = self.ed.cursor;
        if c.line < self.top { self.top = c.line; }
        if rows > 0 && c.line >= self.top + rows { self.top = c.line + 1 - rows; }
        if c.col < self.left { self.left = c.col; }
        if cols > 0 && c.col >= self.left + cols { self.left = c.col + 1 - cols; }
    }
}

impl SourcePane {
    pub(crate) fn script(&mut self) -> Option<&mut ScriptTab> {
        match self.tabs.get_mut(self.active) { Some(Tab::Script(s)) => Some(s), _ => None }
    }

    pub(crate) fn unsaved(&self) -> usize {
        self.tabs.iter().filter(|t| matches!(t, Tab::Script(s) if s.ed.dirty)).count()
    }

    fn label(t: &Tab) -> String {
        match t {
            Tab::Script(s) => s.ed.title(),
            Tab::Data(_, code) => format!("View: {code}"),
        }
    }
}

impl Gui {
    /// Add a script as a new tab, and show it. An empty, untouched script
    /// tab gives way to it.
    pub(crate) fn add_script(&mut self, ed: Editor) {
        let blank = matches!(self.source.tabs.get(self.source.active),
            Some(Tab::Script(s)) if s.ed.path.is_none() && !s.ed.dirty && s.ed.text().is_empty());
        if blank && self.source.tabs.len() == 1 { self.source.tabs.clear(); }
        self.source.tabs.push(Tab::Script(ScriptTab::new(ed)));
        self.source.active = self.source.tabs.len() - 1;
        self.show_window(self.source_id);
        // a script just opened is what the user means to type in
        self.mdi.bring_to_front(self.source_id);
    }

    /// `View(x)` or a click in Environment: a data tab, or the one already
    /// open on the same expression.
    pub(crate) fn open_data_tab(&mut self, code: &str) {
        let found = self.source.tabs.iter().position(|t| matches!(t, Tab::Data(_, c) if c == code));
        self.source.active = match found {
            Some(i) => i,
            None => {
                let uid = self.new_view(code);
                self.source.tabs.push(Tab::Data(uid, code.to_string()));
                self.source.tabs.len() - 1
            }
        };
        self.show_window(self.source_id);
    }

    fn close_tab(&mut self, i: usize) {
        match self.source.tabs.get_mut(i) {
            Some(Tab::Script(s)) if s.ed.dirty && !s.close_armed => {
                s.close_armed = true;
                let name = s.ed.title();
                self.buffer.push_error(&format!("{name} has unsaved changes: save it (Ctrl+S), or click its × again to discard them."));
                self.source.active = i;
                return;
            }
            Some(Tab::Data(uid, _)) => { let uid = *uid; self.views.remove(&uid); }
            Some(_) => {}
            None => return,
        }
        self.source.tabs.remove(i);
        self.source.active = self.source.active.min(self.source.tabs.len().saturating_sub(1));
        if self.source.tabs.is_empty() { self.add_script(Editor::new()); }
    }

    /// Script menu commands (`script.*`).
    pub(crate) fn script_action(&mut self, action: &str, ctx: &mut FrameCtx) {
        match action {
            "script.new" => self.add_script(Editor::new()),
            "script.open" => {
                let picked = rfd::FileDialog::new().set_title("Open script").set_directory(&self.session.cwd)
                    .add_filter("R / R2 scripts", &["R", "r", "R2", "r2"])
                    .add_filter("All files", &["*"]).pick_file();
                if let Some(path) = picked { self.open_script_file(&path); }
            }
            "script.save" => self.save_script(false),
            "script.save_as" => self.save_script(true),
            "script.run" => {
                let code = self.source.script().and_then(|s| s.ed.take_run_code());
                if let Some(code) = code { self.run_code(&code); }
            }
            "script.run_all" => {
                let code = self.source.script().map(|s| s.ed.text());
                if let Some(code) = code { self.run_code(&code); }
            }
            _ => {
                let Some(s) = self.source.script() else { return };
                match action {
                    "script.undo" => s.ed.undo(),
                    "script.redo" => s.ed.redo(),
                    "script.select_all" => s.ed.select_all(),
                    "script.copy" => if let Some(t) = s.ed.selected_text() { ctx.clipboard.set_text(&t) },
                    "script.cut" => if let Some(t) = s.ed.cut() { ctx.clipboard.set_text(&t) },
                    "script.paste" => if let Some(t) = ctx.clipboard.get_text() { s.ed.insert_text(&t) },
                    _ => {}
                }
            }
        }
    }

    pub(crate) fn open_script_file(&mut self, path: &std::path::Path) {
        let already = self.source.tabs.iter().position(|t| matches!(t, Tab::Script(s) if s.ed.path.as_deref() == Some(path)));
        if let Some(i) = already {
            self.source.active = i;
            self.show_window(self.source_id);
            return;
        }
        match Editor::open(path) {
            Ok(ed) => self.add_script(ed),
            Err(e) => self.buffer.push_error(&format!("Could not open {}: {}", path.display(), e)),
        }
    }

    fn save_script(&mut self, ask: bool) {
        let cwd = self.session.cwd.clone();
        let Some(s) = self.source.script() else { return };
        let path = if ask || s.ed.path.is_none() {
            let name = s.ed.path.as_ref().and_then(|x| x.file_name())
                .map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "script.R".into());
            match rfd::FileDialog::new().set_title("Save script").set_directory(cwd).set_file_name(name)
                .add_filter("R / R2 scripts", &["R", "R2"]).save_file() {
                Some(x) => Some(x),
                None => return,
            }
        } else { None };
        let result = s.ed.save(path.as_deref());
        s.close_armed = false;
        let shown = s.ed.path.as_ref().map(|x| x.display().to_string()).unwrap_or_default();
        match result {
            Ok(()) => self.buffer.push_output(&format!("Saved {shown}")),
            Err(e) => self.buffer.push_error(&format!("Save failed: {e}")),
        }
    }

    /// Send code to the console a line at a time, as if typed there.
    pub(crate) fn run_code(&mut self, code: &str) {
        for line in code.split('\n') { self.submit(line.to_string()); }
    }

    /// Clicks on the strip, the wheel under the pointer, and — while the pane
    /// is in front — the keyboard and the mouse in the text.
    pub(crate) fn source_input(&mut self, ui_events: &[InputEvent], ctx: &mut FrameCtx, renderer: &mut Renderer, theme: &Theme) {
        let Some(content) = self.mdi.window(self.source_id).filter(|w| w.visible).map(|w| w.content_rect(theme)) else { return };
        let top = self.mdi.z_order().last() == Some(self.source_id);
        let (cell_w, line_h) = renderer.cell_metrics(theme.fs());
        let strip = strip_h(line_h);
        let body = Rect { x: content.x, y: content.y + strip, w: content.w, h: (content.h - strip).max(0.0) };
        let mouse = self.mouse;

        // the strip first: it can close the tab the rest would act on
        let mut actions: Vec<&'static str> = Vec::new();
        let (mut switch, mut close) = (None, None);
        for ev in ui_events {
            if let InputEvent::MouseDown { button: MouseButton::Left, pos } = *ev {
                let hit = |r: &Option<Rect>| r.is_some_and(|r| inside(r, pos.x, pos.y));
                if let Some(i) = self.source.close_rects.iter().position(|r| inside(*r, pos.x, pos.y)) { close = Some(i); }
                else if let Some(i) = self.source.tab_rects.iter().position(|r| inside(*r, pos.x, pos.y)) { switch = Some(i); }
                else if hit(&self.source.run) { actions.push("script.run"); }
                else if hit(&self.source.run_all) { actions.push("script.run_all"); }
                else if hit(&self.source.save) { actions.push("script.save"); }
            }
        }
        if let Some(i) = switch { self.source.active = i; }
        if let Some(i) = close { self.close_tab(i); }

        let active = self.source.active;
        match self.source.tabs.get_mut(active) {
            Some(Tab::Data(uid, _)) => {
                let uid = *uid;
                if let Some(v) = self.views.get_mut(&uid) { v.input(ui_events, inside(content, mouse.0, mouse.1), top); }
            }
            Some(Tab::Script(s)) => {
                s.input(ui_events, body, inside(content, mouse.0, mouse.1), top, (cell_w, line_h), &mut actions);
            }
            None => {}
        }
        for a in actions { self.script_action(a, ctx); }
    }

    pub(crate) fn paint_source(&mut self, content: Rect, frame: &mut Frame, renderer: &mut Renderer, theme: &Theme) {
        let (cell_w, line_h) = renderer.cell_metrics(theme.fs());
        let (bg, fg) = (theme.window_background, theme.console_output);
        let strip = strip_h(line_h);
        frame.paint_rect(content.x, content.y, content.w, strip, mix(bg, fg, 0.08));

        // right end of the strip: what a script tab can do
        let is_script = matches!(self.source.tabs.get(self.source.active), Some(Tab::Script(_)));
        let (mut run, mut run_all, mut save) = (None, None, None);
        let mut right = content.x + content.w - 4.0;
        if is_script {
            let (y, h) = (content.y + 3.0, strip - 6.0);
            let r = button_left_of(frame, renderer, theme, right, y, h, "Save", false);
            save = Some(r); right = r.x - 4.0;
            let r = button_left_of(frame, renderer, theme, right, y, h, "Run all", false);
            run_all = Some(r); right = r.x - 4.0;
            let r = button_left_of(frame, renderer, theme, right, y, h, "Run", false);
            run = Some(r); right = r.x - 8.0;
        }
        let labels: Vec<String> = self.source.tabs.iter().map(|t| format!("{}  ×", SourcePane::label(t))).collect();
        let rects = tabs(frame, renderer, theme, content.x + 2.0, content.y, strip, right, &labels, self.source.active);
        // the × is the last cells of each label
        self.source.close_rects = rects.iter().map(|r| Rect { x: r.x + r.w - 2.5 * cell_w, y: r.y, w: 2.5 * cell_w, h: r.h }).collect();
        self.source.tab_rects = rects;
        (self.source.run, self.source.run_all, self.source.save) = (run, run_all, save);

        let body = Rect { x: content.x, y: content.y + strip, w: content.w, h: (content.h - strip).max(0.0) };
        let on = (self.frame_counter / 30).is_multiple_of(2);
        let active = self.source.active;
        match self.source.tabs.get_mut(active) {
            Some(Tab::Script(s)) => paint_script(s, body, on, frame, renderer, theme),
            Some(Tab::Data(uid, _)) => {
                let uid = *uid;
                if let Some(v) = self.views.get_mut(&uid) { v.paint(body, frame, renderer, theme); }
                self.fetch_view_pages(uid);
            }
            None => {}
        }
    }
}

pub(crate) fn paint_script(p: &mut ScriptTab, body: Rect, caret_on: bool, frame: &mut Frame, renderer: &mut Renderer, theme: &Theme) {
    let (cell_w, line_h) = renderer.cell_metrics(theme.fs());
    let size = theme.fs();
    let bg = theme.window_background;
    let fg = theme.console_output;
    frame.paint_rect(body.x, body.y, body.w, body.h, bg);
    let text = p.text_rect(body, cell_w);
    let gutter_w = p.gutter_cells() as f32 * cell_w + PAD;
    frame.paint_rect(body.x, body.y, gutter_w, body.h, mix(bg, fg, 0.06));

    let rows = (text.h / line_h).floor() as usize;
    let cols = (text.w / cell_w).floor() as usize;
    p.page = rows.saturating_sub(1).max(1);
    let lines = p.ed.buffer().lines();
    let states = line_states(lines);
    let sel = p.ed.selection();
    let colors = palette(theme);
    let color_of = |k: Kind| colors.iter().find(|(kk, _)| *kk == k).map(|(_, c)| *c).unwrap_or(fg);
    let clip_x = text.x + text.w;
    let base = line_h * 0.8;

    for r in 0..=rows {
        let ln = p.top + r;
        if ln >= lines.len() { break; }
        let y = text.y + r as f32 * line_h;
        if y + line_h > body.y + body.h { break; }
        if ln == p.ed.cursor.line && sel.is_none() {
            frame.paint_rect(text.x - PAD / 2.0, y, text.w + PAD / 2.0, line_h, mix(bg, fg, 0.05));
        }
        // line number, right-aligned in the gutter
        let num = (ln + 1).to_string();
        let num_col = if ln == p.ed.cursor.line { fg } else { mix(fg, bg, 0.55) };
        let nx = body.x + gutter_w - PAD - num.len() as f32 * cell_w;
        for (k, ch) in num.chars().enumerate() {
            frame.paint_glyph(renderer, nx + k as f32 * cell_w, y + base, ch, size, num_col);
        }
        // selection band for this line
        if let Some((a, b)) = sel {
            if a.line <= ln && ln <= b.line {
                let from = if ln == a.line { a.col } else { 0 };
                let to = if ln == b.line { b.col } else { lines[ln].chars().count() + 1 };
                let (from, to) = (from.max(p.left), to.min(p.left + cols));
                if to > from {
                    frame.paint_rect(text.x + (from - p.left) as f32 * cell_w, y,
                                     (to - from) as f32 * cell_w, line_h, theme.console_selection_bg);
                }
            }
        }
        // the text, coloured by span
        let chars: Vec<char> = lines[ln].chars().collect();
        let mut kinds = vec![Kind::Plain; chars.len()];
        for s in highlight_line(&lines[ln], states[ln]).0 {
            for k in kinds.iter_mut().take(s.end).skip(s.start) { *k = s.kind; }
        }
        for col in p.left..chars.len().min(p.left + cols + 1) {
            let ch = chars[col];
            if ch == ' ' { continue; }
            let selected = sel.is_some_and(|(a, b)| Pos::new(ln, col) >= a && Pos::new(ln, col) < b);
            let color = if selected { theme.console_selection_fg } else { color_of(kinds[col]) };
            frame.paint_glyph_clipped(renderer, text.x + (col - p.left) as f32 * cell_w,
                                      y + base, ch, size, color, clip_x);
        }
    }
    // the caret, blinking like the console's
    let c = p.ed.cursor;
    if caret_on && c.line >= p.top && c.line < p.top + rows.max(1) && c.col >= p.left && c.col <= p.left + cols {
        let x = text.x + (c.col - p.left) as f32 * cell_w;
        let y = text.y + (c.line - p.top) as f32 * line_h;
        frame.paint_rect(x, y + line_h * 0.1, 2.0, line_h * 0.8, theme.cursor);
    }
}
