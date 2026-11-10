//! Gview: graphs in their own window, where they can be edited and saved.
//!
//! ```text
//!   ┌ Gview ────────────────────────────────────────────── _ □ × ┐
//!   │ [Histogram of mpg ×] [Box plot ×]   [Undo] [Script] [Copy] [Save…] │
//!   │ ┌──────────── graph ────────────┐  Title                      │
//!   │ │      My title  ← click to edit │  [My title              ]   │
//!   │ │                                │  Fill colour  ■ ■ ■ ■ ■ ■   │
//!   │ └──────── X axis name ───────────┘  Point shape  ○ ● ■ ▲       │
//! ```
//!
//! The window floats over Ardon Studio: it can be moved, resized, maximized
//! and closed, and it stays open while Sakha or Pro is used underneath. Each
//! graph is R2 code (`sakha_core::gview::Graph`), edited two ways: on the
//! graph and its settings panel, or as code in the Script view. Both change
//! the same code, which R2 runs again; Undo steps back through the changes.

use r2_ui::{Color, Frame, FrameCtx, GraphPanel, InputEvent, InputField, KeyCode, MdiHost, MouseButton, Rect, Renderer, Theme,
            WindowId};
use r2_editor::Editor;
use sakha_core::gview::{colour_name, part_at, quote, Graph, Part, Setting, COLOURS, SHAPES};

use crate::report::svg_aspect;
use crate::session::Req;
use crate::source_view::{paint_script, ScriptTab};
use crate::ui::{button, button_left_of, inside, mix, text, wrap};
use crate::Gui;

/// The settings panel's width, in cells.
const PANEL_W: usize = 36;
/// Graphs open at once.
const MAX_DOCS: usize = 8;

/// One graph in Gview.
pub(crate) struct GraphDoc {
    title: String,
    graph: Graph,
    svg: Option<String>,
    /// Bumped with each new picture, so the window draws it once.
    drawn: u64,
    /// Bumped with each run: an older reply is dropped.
    gen: u64,
    running: bool,
    /// What R2 said when the code failed.
    error: String,
    /// Earlier versions, for Undo.
    undo: Vec<Graph>,
}

#[derive(Clone, Copy, PartialEq)]
enum Cmd { Script, Run, Undo, Copy, Save }

#[derive(Default)]
struct Hits {
    tabs: Vec<(Rect, usize)>,
    closes: Vec<(Rect, usize)>,
    buttons: Vec<(Rect, Cmd)>,
    /// The graph on screen, and its SVG's size.
    plot: Option<(Rect, f32, f32)>,
    /// A text setting's box.
    fields: Vec<(Rect, &'static str)>,
    /// (swatch, argument, colour code or None for R2's default)
    swatches: Vec<(Rect, &'static str, Option<String>)>,
    /// (button, argument, new value or None for the default)
    values: Vec<(Rect, &'static str, Option<f64>)>,
    panel: Option<Rect>,
    /// The Script view's editor.
    script: Option<Rect>,
}

pub(crate) struct Gview {
    pub(crate) mdi: MdiHost,
    id: WindowId,
    docs: Vec<GraphDoc>,
    active: usize,
    panel: GraphPanel,
    /// (doc index, gen) the panel shows.
    holds: Option<(usize, u64)>,
    /// The Script view, while it is showing.
    script: Option<ScriptTab>,
    /// The text setting being typed in.
    field: Option<(&'static str, InputField, bool)>,
    /// The last click was in Gview: the keyboard is Gview's.
    focused: bool,
    hint: String,
    /// Cell width and line height at the last paint.
    cell: (f32, f32),
    hits: Hits,
}

impl Default for Gview {
    fn default() -> Self {
        let mut mdi = MdiHost::new();
        let id = mdi.add_window("Gview", Rect { x: 80.0, y: 80.0, w: 1100.0, h: 700.0 });
        if let Some(w) = mdi.window_mut(id) { w.visible = false; }
        Gview { mdi, id, docs: vec![], active: 0, panel: GraphPanel::with_slot(1020, 760), holds: None, script: None,
                field: None, focused: false, hint: String::new(), cell: (8.0, 16.0), hits: Hits::default() }
    }
}

impl Gview {
    pub(crate) fn visible(&self) -> bool { self.mdi.window(self.id).is_some_and(|w| w.visible) }

    /// The mouse is over the window, or it is being dragged: input is Gview's.
    pub(crate) fn takes(&self, mouse: (f32, f32)) -> bool {
        self.visible() && self.mdi.window(self.id).is_some_and(|w| inside(w.bounds, mouse.0, mouse.1))
    }

    fn doc(&mut self) -> Option<&mut GraphDoc> { self.docs.get_mut(self.active) }
}

impl Gui {
    /// Open this graph code in Gview, as a new graph or the one it already is.
    pub(crate) fn open_in_gview(&mut self, title: &str, code: &[String]) {
        let Some(graph) = Graph::from_code(code) else {
            self.sakha.status = "There is no graph in this to edit.".into();
            return;
        };
        let g = &mut self.gview;
        let at = match g.docs.iter().position(|d| d.title == title) {
            Some(i) => i,
            None => {
                if g.docs.len() == MAX_DOCS { g.docs.remove(0); }
                g.docs.push(GraphDoc { title: title.to_string(), graph, svg: None, drawn: 0, gen: 0, running: false, error: String::new(), undo: vec![] });
                g.docs.len() - 1
            }
        };
        g.active = at;
        g.holds = None;
        g.script = None;
        g.field = None;
        g.focused = true;
        g.hint = "Click the title or an axis name to change it; or use Script.".into();
        // a fresh window fits the space it opens in
        let ws = self.workspace;
        if let Some(w) = g.mdi.window_mut(g.id) {
            if !w.visible { w.bounds = Rect { x: ws.x + ws.w * 0.12, y: ws.y + ws.h * 0.08, w: ws.w * 0.76, h: ws.h * 0.84 }; }
            w.visible = true;
        }
        self.gview_run(at);
    }

    /// Run graph `i`'s code; its whole plot is read from a file afterwards,
    /// as `--ide` replies leave out what is added after the first call.
    fn gview_run(&mut self, i: usize) {
        let Some(d) = self.gview.docs.get_mut(i) else { return };
        d.gen += 1;
        d.running = true;
        d.error.clear();
        let (gen, code) = (d.gen, d.graph.code());
        self.session.send("eval", sakha_engine::json!({ "code": code }), Req::GviewRun { doc: i, gen });
    }

    pub(crate) fn gview_reply(&mut self, doc: usize, gen: u64, body: &sakha_engine::Value) {
        let Some(d) = self.gview.docs.get_mut(doc).filter(|d| d.gen == gen) else { return };
        if body["error"] == sakha_engine::Value::Bool(true) {
            d.running = false;
            if d.error.is_empty() { d.error = "R2 could not draw this.".into(); }
            return;
        }
        let file = gview_file();
        let code = format!("save.plot(\"{}\")", sakha_core::document::r_path(&file));
        self.session.value(&code, Req::GviewPlot { doc, gen });
    }

    pub(crate) fn gview_plot(&mut self, doc: usize, gen: u64) {
        let file = gview_file();
        let svg = std::fs::read_to_string(&file).ok();
        let _ = std::fs::remove_file(&file);
        let Some(d) = self.gview.docs.get_mut(doc).filter(|d| d.gen == gen) else { return };
        d.running = false;
        match svg {
            Some(s) => { d.svg = Some(s); d.drawn += 1; }
            None => d.error = "R2 drew nothing.".into(),
        }
        // the session script keeps the graph as it now is
        if self.sakha.df.is_some() {
            let (key, title, code) = (format!("graph:{}", d.title), format!("Graph: {}", d.title), vec![d.graph.code()]);
            self.sakha.script.set_step(&key, &title, &code);
            self.sakha.dirty = true;
        }
    }

    pub(crate) fn gview_error_text(&mut self, doc: usize, text: &str) {
        if let Some(d) = self.gview.docs.get_mut(doc) { d.error.push_str(text); }
    }

    /// A change to the active graph: kept for Undo, then drawn again.
    fn gview_change(&mut self, f: impl FnOnce(&mut Graph)) {
        let i = self.gview.active;
        let Some(d) = self.gview.doc() else { return };
        let before = d.graph.clone();
        f(&mut d.graph);
        if d.graph == before { return; }
        d.undo.push(before);
        if let Some(s) = &mut self.gview.script { s.ed = Editor::from_text(&self.gview.docs[i].graph.script()); }
        self.gview_run(i);
    }

    /// Input for Gview; returns whether it took the events.
    pub(crate) fn gview_input(&mut self, events: &[InputEvent], ctx: &mut FrameCtx, theme: &Theme) -> bool {
        if !self.gview.visible() { return false; }
        let mouse = self.mouse;
        // a click decides who has the keyboard
        for ev in events {
            if let InputEvent::MouseDown { pos, .. } = ev { self.gview.focused = self.gview.takes((pos.x, pos.y)); }
        }
        let over = self.gview.takes(mouse);
        if !over && !self.gview.focused { return false; }
        self.gview.mdi.handle_events(events, theme);
        if self.gview.mdi.take_close_requested(self.gview.id) {
            if let Some(w) = self.gview.mdi.window_mut(self.gview.id) { w.visible = false; }
            self.gview.focused = false;
            return true;
        }
        // typing in a setting
        if let Some((arg, f, fresh)) = &mut self.gview.field {
            let arg = *arg;
            // the first key typed replaces the old text, as in a spreadsheet cell
            if *fresh {
                for ev in events {
                    match ev {
                        InputEvent::Char(_) | InputEvent::Key { code: KeyCode::Backspace | KeyCode::Delete, pressed: true, .. } => { f.clear(); *fresh = false; break; }
                        InputEvent::Key { pressed: true, code, .. } if *code != KeyCode::Enter && *code != KeyCode::Escape => { *fresh = false; break; }
                        _ => {}
                    }
                }
            }
            let r = f.handle_events(events, ctx.clipboard);
            if let Some(line) = r.submitted.or_else(|| r.auto_submit_lines.first().cloned()) {
                self.gview.field = None;
                self.gview_change(|g| g.set_text(arg, line.trim()));
                self.gview.hint.clear();
                return true;
            }
            if r.escaped { self.gview.field = None; self.gview.hint.clear(); }
        }
        // the Script view's editor
        let mut script_actions: Vec<&'static str> = vec![];
        if let (Some(s), Some(body)) = (&mut self.gview.script, self.gview.hits.script) {
            let keys = self.gview.focused && self.gview.field.is_none();
            s.input(events, body, inside(body, mouse.0, mouse.1), keys, self.gview.cell, &mut script_actions);
        }
        if script_actions.contains(&"script.run") || script_actions.contains(&"script.run_all") { self.gview_script_run(); }

        let mut cmd = None;
        for ev in events {
            let InputEvent::MouseDown { button: MouseButton::Left, pos } = *ev else {
                if let InputEvent::Key { code: KeyCode::KeyZ, mods, pressed: true } = *ev {
                    if mods.ctrl && self.gview.script.is_none() && self.gview.field.is_none() { cmd = Some(Cmd::Undo); }
                }
                continue;
            };
            let (x, y) = (pos.x, pos.y);
            let h = &self.gview.hits;
            if let Some(&(_, i)) = h.closes.iter().find(|(r, _)| inside(*r, x, y)) {
                self.gview.docs.remove(i);
                self.gview.active = self.gview.active.min(self.gview.docs.len().saturating_sub(1));
                self.gview.holds = None;
                self.gview.script = None;
                if self.gview.docs.is_empty() { if let Some(w) = self.gview.mdi.window_mut(self.gview.id) { w.visible = false; } }
                return true;
            }
            if let Some(&(_, i)) = h.tabs.iter().find(|(r, _)| inside(*r, x, y)) {
                self.gview.active = i;
                self.gview.field = None;
                if let Some(s) = &mut self.gview.script { s.ed = Editor::from_text(&self.gview.docs[i].graph.script()); }
            } else if let Some(&(_, c)) = h.buttons.iter().find(|(r, _)| inside(*r, x, y)) {
                cmd = Some(c);
            } else if let Some(&(_, arg)) = h.fields.iter().find(|(r, _)| inside(*r, x, y)) {
                self.gview_edit_text(arg);
            } else if let Some((_, arg, v)) = h.swatches.iter().find(|(r, _, _)| inside(*r, x, y)).cloned() {
                self.gview_change(|g| g.set(arg, v));
            } else if let Some(&(_, arg, v)) = h.values.iter().find(|(r, _, _)| inside(*r, x, y)) {
                self.gview_change(|g| match v { Some(n) => g.set_num(arg, n), None => g.set(arg, None) });
            } else if let Some((p, sw, sh)) = h.plot.filter(|(p, _, _)| inside(*p, x, y)) {
                // where on the graph, in the SVG's own coordinates
                let part = part_at((x - p.x) / p.w * sw, (y - p.y) / p.h * sh, sw, sh);
                match part {
                    Part::Title => self.gview_edit_text("main"),
                    Part::XLabel => self.gview_edit_text("xlab"),
                    Part::YLabel => self.gview_edit_text("ylab"),
                    Part::Drawing => self.gview.hint = "Colours, shapes and sizes are on the right.".into(),
                }
            }
        }
        match cmd {
            Some(Cmd::Script) => {
                self.gview.field = None;
                self.gview.script = match self.gview.script.take() {
                    Some(_) => None,
                    None => self.gview.docs.get(self.gview.active).map(|d| ScriptTab::new(Editor::from_text(&d.graph.script()))),
                };
            }
            Some(Cmd::Run) => self.gview_script_run(),
            Some(Cmd::Undo) => {
                let i = self.gview.active;
                if let Some(prev) = self.gview.doc().and_then(|d| d.undo.pop()) {
                    self.gview.docs[i].graph = prev;
                    if let Some(s) = &mut self.gview.script { s.ed = Editor::from_text(&self.gview.docs[i].graph.script()); }
                    self.gview_run(i);
                }
            }
            Some(Cmd::Copy) => self.gview_copy(ctx),
            Some(Cmd::Save) => self.gview_save(),
            None => {}
        }
        true
    }

    fn gview_edit_text(&mut self, arg: &'static str) {
        self.gview.script = None;
        let now = self.gview.docs.get(self.gview.active).and_then(|d| d.graph.get_text(arg)).unwrap_or_default();
        let mut f = InputField::new().prompt("");
        f.set_line(now);
        self.gview.field = Some((arg, f, true));
        self.gview.hint = "Type, then Enter. Esc leaves it as it was; empty is R2's own.".into();
    }

    /// Run the code as edited in the Script view; its settings show again.
    fn gview_script_run(&mut self) {
        let i = self.gview.active;
        let Some(code) = self.gview.script.as_ref().map(|s| s.ed.text()) else { return };
        match Graph::from_code(&[code]) {
            Some(g) => {
                let Some(d) = self.gview.docs.get_mut(i) else { return };
                if d.graph != g { d.undo.push(std::mem::replace(&mut d.graph, g)); }
                self.gview_run(i);
            }
            None => self.gview.hint = "The script needs a hist, boxplot, barplot or plot call.".into(),
        }
    }

    fn gview_copy(&mut self, ctx: &mut FrameCtx) {
        let Some(svg) = self.gview.docs.get(self.gview.active).and_then(|d| d.svg.clone()) else { return };
        let (w, h) = pixels(&svg);
        self.gview.hint = match r2_ui::graph::rasterize_plot(svg.as_bytes(), w, h) {
            Some(rgba) if ctx.clipboard.set_image(w, h, &rgba) => format!("Copied as a {w}×{h} image: paste it into Word."),
            _ => "Could not copy the graph.".into(),
        };
    }

    fn gview_save(&mut self) {
        let Some(d) = self.gview.docs.get(self.gview.active) else { return };
        let Some(svg) = d.svg.clone() else { return };
        let name: String = d.title.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
        let name = format!("{}.png", name.trim_matches('_'));
        if let Some(msg) = self.save_svg(&svg, &name, pixels(&svg), false) { self.gview.hint = msg; }
    }

    /// The window over everything else, painted last.
    pub(crate) fn paint_gview(&mut self, frame: &mut Frame, renderer: &mut Renderer, theme: &Theme) {
        if !self.gview.visible() { return; }
        self.gview.mdi.set_workspace(self.workspace);
        let id = self.gview.id;
        self.gview.mdi.paint_body(id, frame, theme);
        if let Some(c) = self.gview.mdi.window(id).map(|w| w.content_rect(theme)) { self.paint_gview_content(c, frame, renderer, theme); }
        self.gview.mdi.paint_titlebar(id, frame, renderer, theme);
    }

    fn paint_gview_content(&mut self, c: Rect, frame: &mut Frame, renderer: &mut Renderer, theme: &Theme) {
        let (cw, lh) = renderer.cell_metrics(theme.fs());
        let (bg, fg, size) = (theme.window_background, theme.console_output, theme.fs());
        let dim = mix(fg, bg, 0.45);
        let g = &mut self.gview;
        g.hits = Hits::default();
        g.cell = (cw, lh);
        frame.paint_rect(c.x, c.y, c.w, c.h, bg);
        if g.docs.is_empty() {
            text(frame, renderer, c.x + 10.0, c.y + lh * 1.2, "No graphs. Open one from a result, or a variable's page in Sakha.", size, dim, cw, c.x + c.w);
            return;
        }
        let active = g.active.min(g.docs.len() - 1);

        // ── tabs and commands
        let strip = lh + 10.0;
        frame.paint_rect(c.x, c.y, c.w, strip, mix(bg, fg, 0.06));
        let (by, bh) = (c.y + 3.0, strip - 6.0);
        let mut right = c.x + c.w - 6.0;
        let d = &g.docs[active];
        for (label, cmd, on) in [("Save…", Cmd::Save, false), ("Copy image", Cmd::Copy, false),
                                 ("Script", Cmd::Script, g.script.is_some()), ("Undo", Cmd::Undo, false)] {
            if cmd == Cmd::Undo && d.undo.is_empty() { continue; }
            if matches!(cmd, Cmd::Save | Cmd::Copy) && d.svg.is_none() { continue; }
            let r = button_left_of(frame, renderer, theme, right, by, bh, label, on);
            g.hits.buttons.push((r, cmd));
            right = r.x - 6.0;
        }
        let mut x = c.x + 4.0;
        for (i, d) in g.docs.iter().enumerate() {
            let name: String = d.title.chars().take(24).collect();
            let w = (name.chars().count() + 4) as f32 * cw;
            if x + w > right - 8.0 { break; }
            let r = Rect { x, y: by, w, h: bh };
            frame.paint_rect(r.x, r.y, r.w, r.h, if i == active { theme.console_selection_bg } else { mix(bg, fg, 0.12) });
            let ink = if i == active { theme.console_selection_fg } else { fg };
            text(frame, renderer, x + cw * 0.5, by + (bh - lh) / 2.0 + lh * 0.8, &name, size, ink, cw, x + w - 2.0 * cw);
            let close = Rect { x: x + w - 2.0 * cw, y: by, w: 2.0 * cw, h: bh };
            text(frame, renderer, close.x + cw * 0.5, by + (bh - lh) / 2.0 + lh * 0.8, "×", size, ink, cw, close.x + close.w);
            g.hits.tabs.push((r, i));
            g.hits.closes.push((close, i));
            x += w + 4.0;
        }

        // ── the panel on the right: settings, or the script
        // the script needs room for its lines
        let pw = if g.script.is_some() { (c.w * 0.42).max(PANEL_W as f32 * cw) } else { PANEL_W as f32 * cw }.min(c.w * 0.5);
        let panel = Rect { x: c.x + c.w - pw, y: c.y + strip + 6.0, w: pw - 6.0, h: (c.h - strip - 12.0).max(0.0) };
        g.hits.panel = Some(panel);
        frame.paint_rect(panel.x, panel.y, panel.w, panel.h, mix(bg, fg, 0.03));
        let foot = lh * 3.2;
        let hint = if !g.docs[active].error.is_empty() { g.docs[active].error.trim().to_string() }
                   else if g.docs[active].running { "R2 is drawing…".to_string() } else { g.hint.clone() };
        let err = !g.docs[active].error.is_empty();
        for (k, l) in wrap(&hint, ((panel.w - 12.0) / cw) as usize).into_iter().take(3).enumerate() {
            text(frame, renderer, panel.x + 6.0, panel.y + panel.h - foot + lh * (0.8 + k as f32), &l, size,
                 if err { theme.console_error } else { dim }, cw, panel.x + panel.w - 4.0);
        }
        if let Some(s) = &mut g.script {
            let body = Rect { x: panel.x, y: panel.y, w: panel.w, h: (panel.h - foot - lh - 14.0).max(0.0) };
            g.hits.script = Some(body);
            paint_script(s, body, g.focused && g.field.is_none(), frame, renderer, theme);
            let run = Rect { x: panel.x + 6.0, y: body.y + body.h + 6.0, w: 26.0 * cw, h: lh + 6.0 };
            button(frame, renderer, theme, run, "Run (Ctrl+Enter)", true);
            g.hits.buttons.push((run, Cmd::Run));
        } else {
            let graph = g.docs[active].graph.clone();
            let mut y = panel.y + 6.0;
            let x0 = panel.x + 8.0;
            let x1 = panel.x + panel.w - 8.0;
            for ctl in graph.controls() {
                if y + lh * 2.6 > panel.y + panel.h - foot { break; }
                text(frame, renderer, x0, y + lh * 0.8, ctl.label, size, dim, cw, x1);
                y += lh * 1.1;
                match ctl.kind {
                    Setting::Text => {
                        let r = Rect { x: x0, y, w: x1 - x0, h: lh + 6.0 };
                        frame.paint_rect(r.x, r.y, r.w, r.h, mix(bg, fg, 0.10));
                        match &g.field {
                            Some((arg, f, _)) if *arg == ctl.arg => {
                                frame.paint_rect(r.x, r.y + r.h - 2.0, r.w, 2.0, theme.cursor);
                                f.paint(frame, renderer, Rect { x: r.x + 4.0, y: r.y + 3.0, w: r.w - 8.0, h: lh }, cw, lh, size, theme, true);
                            }
                            _ => match graph.get_text(ctl.arg) {
                                Some(v) => text(frame, renderer, r.x + 6.0, r.y + 3.0 + lh * 0.8, &v, size, fg, cw, r.x + r.w - 4.0),
                                None => text(frame, renderer, r.x + 6.0, r.y + 3.0 + lh * 0.8, "(R2's own)", size, dim, cw, r.x + r.w - 4.0),
                            },
                        }
                        g.hits.fields.push((r, ctl.arg));
                        y += r.h + lh * 0.5;
                    }
                    Setting::Colour => {
                        let now = graph.get(ctl.arg);
                        let s = lh + 4.0;
                        let mut sx = x0;
                        let auto = Rect { x: sx, y, w: 6.0 * cw, h: s };
                        button(frame, renderer, theme, auto, "Auto", now.is_none());
                        g.hits.swatches.push((auto, ctl.arg, None));
                        sx += auto.w + 6.0;
                        for (_, hex) in COLOURS {
                            if sx + s > x1 { sx = x0; y += s + 4.0; }
                            let r = Rect { x: sx, y, w: s, h: s };
                            let on = now.as_deref().and_then(colour_name) == COLOURS.iter().find(|(_, h)| h == hex).map(|(n, _)| *n);
                            if on { frame.paint_rect(r.x - 2.0, r.y - 2.0, r.w + 4.0, r.h + 4.0, theme.cursor); }
                            frame.paint_rect(r.x, r.y, r.w, r.h, mix(bg, fg, 0.5));
                            frame.paint_rect(r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 2.0, hex_colour(hex));
                            g.hits.swatches.push((r, ctl.arg, Some(quote(hex))));
                            sx += s + 4.0;
                        }
                        y += s + lh * 0.6;
                    }
                    Setting::Shape => {
                        let now = graph.get_num(ctl.arg);
                        let mut sx = x0;
                        for (name, pch) in SHAPES {
                            let w = (name.chars().count() + 2) as f32 * cw;
                            if sx + w > x1 { sx = x0; y += lh + 10.0; }
                            let r = Rect { x: sx, y, w, h: lh + 6.0 };
                            button(frame, renderer, theme, r, name, now == Some(*pch as f64));
                            g.hits.values.push((r, ctl.arg, Some(*pch as f64)));
                            sx += w + 4.0;
                        }
                        y += lh + 6.0 + lh * 0.6;
                    }
                    Setting::Number { min, max, step, default } => {
                        let now = graph.get_num(ctl.arg);
                        let v = now.unwrap_or(default);
                        let h = lh + 6.0;
                        let minus = Rect { x: x0, y, w: 3.0 * cw, h };
                        button(frame, renderer, theme, minus, "−", false);
                        g.hits.values.push((minus, ctl.arg, Some((v - step).max(min))));
                        let shown = if now.is_some() { format!("{}", (v * 100.0).round() / 100.0) } else { "auto".into() };
                        text(frame, renderer, minus.x + minus.w + cw, y + 3.0 + lh * 0.8, &shown, size, fg, cw, x1);
                        let plus = Rect { x: minus.x + minus.w + 7.0 * cw, y, w: 3.0 * cw, h };
                        button(frame, renderer, theme, plus, "+", false);
                        g.hits.values.push((plus, ctl.arg, Some((v + step).min(max))));
                        let auto = Rect { x: plus.x + plus.w + cw, y, w: 6.0 * cw, h };
                        button(frame, renderer, theme, auto, "Auto", now.is_none());
                        g.hits.values.push((auto, ctl.arg, None));
                        y += h + lh * 0.6;
                    }
                }
            }
        }

        // ── the graph, on white, as large as fits
        let room = Rect { x: c.x + 6.0, y: c.y + strip + 6.0, w: (c.w - pw - 18.0).max(0.0), h: (c.h - strip - 12.0).max(0.0) };
        frame.paint_rect(room.x, room.y, room.w, room.h, Color::WHITE);
        let d = &g.docs[active];
        let Some(svg) = &d.svg else {
            let msg = if d.running { "R2 is drawing…" } else { "No graph yet." };
            text(frame, renderer, room.x + 10.0, room.y + lh * 1.2, msg, size, Color::rgb(90, 90, 90), cw, room.x + room.w);
            return;
        };
        if g.holds != Some((active, d.drawn)) {
            g.panel.set_svg(svg.clone().into_bytes());
            g.holds = Some((active, d.drawn));
        }
        let aspect = svg_aspect(svg).unwrap_or(2.0 / 3.0);
        let (sw, sh) = svg_size(svg);
        let (mut w, mut h) = (room.w - 20.0, (room.w - 20.0) * aspect);
        if h > room.h - 20.0 { h = room.h - 20.0; w = h / aspect; }
        let p = Rect { x: room.x + (room.w - w) / 2.0, y: room.y + (room.h - h) / 2.0, w: w.max(0.0), h: h.max(0.0) };
        g.panel.paint(frame, renderer, p, theme);
        g.hits.plot = Some((p, sw, sh));
    }
}

/// Where Gview has R2 write the whole plot.
fn gview_file() -> std::path::PathBuf { std::env::temp_dir().join(format!("ardon-gview-{}.svg", std::process::id())) }

/// The SVG's own width and height.
fn svg_size(svg: &str) -> (f32, f32) {
    let head = &svg[..svg.find('>').unwrap_or(svg.len()).min(svg.len())];
    let num = |k: &str| head.find(&format!("{k}=\"")).and_then(|i| {
        let rest = &head[i + k.len() + 2..];
        rest[..rest.find('"')?].trim_end_matches("px").parse::<f32>().ok()
    });
    (num("width").unwrap_or(600.0), num("height").unwrap_or(400.0))
}

/// Pixels for a saved or copied graph: twice its size, for print.
fn pixels(svg: &str) -> (u32, u32) {
    let aspect = svg_aspect(svg).unwrap_or(2.0 / 3.0);
    (1600, (1600.0 * aspect).round() as u32)
}

fn hex_colour(hex: &str) -> Color {
    let v = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0);
    Color::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}
