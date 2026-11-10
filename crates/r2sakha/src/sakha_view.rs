//! Sakha: the menu-driven face of the app, over the whole workspace.
//!
//! ```text
//!   trial: 120 rows · 9 variables · 2 with missing values    [Change data] [Script] [Export]
//!   ┌ Variables ┐  Overview | Data | Results (3)
//!   │ # score   │  ┌ score ───┐ ┌ grp ─────┐ ┌ age ─────┐
//!   │ ◆ grp     │  │ ▁▃▅█▇▃▁  │ │ a ███    │ │ ▂▅█▆▃    │
//!   └───────────┘  └──────────┘ └──────────┘ └──────────┘
//!   Sakha says: score: 118 values, average 80.0 ...
//!   Code  summary(trial$score); ...                                [Run in Studio]
//! ```
//!
//! The philosophy is the opposite of the Studio's: the data comes first, and
//! the user chooses from menus (Summarize, Compare, Relate, Plot), never
//! typing code. Each menu item opens a dialog asking only for the variables
//! it needs; the result is a card with R2's output, its plot and a line in
//! plain words. Sakha computes nothing: all of it is R2 code run by `r2 --ide`,
//! on the same session as the Studio, and everything done is recorded in the
//! session script. On-screen cards are described first, a few columns at a
//! time, so a large dataset never freezes the window.

use std::collections::HashMap;

use r2_ui::{Color, ContextItem, ContextMenu, Frame, GraphPanel, InputEvent, KeyCode, MouseButton, Rect, Renderer, Theme};
use sakha_core::analysis::{self, Analysis, Built};
use sakha_core::profile::{self, code_as_category, code_for, dataset_line, fill, fmt, fmt_short, queries, ColumnProfile, Kind};
use sakha_core::script::{default_script_name, load_line_from_history, SessionScript};
use sakha_engine::{json, Value};

use crate::sakha_dialog::AnalysisDialog;
use crate::session::{After, Req};
use crate::ui::{button, button_left_of, inside, mix, tabs, text, today, wrap};
use crate::Gui;

/// Card size in character cells.
const CARD_W: usize = 24;
const CARD_H: usize = 7;
/// The Variables list's width in cells.
const LIST_W: usize = 22;
/// The script panel's width in cells (when shown).
const SCRIPT_W: usize = 44;
/// Columns being described at once.
const IN_FLIGHT: usize = 2;
/// The script file extension R2 documents for scripts.
const EXT: &str = "r2";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Face { Overview, Data, Results }

struct Job { answers: Vec<Option<Value>>, remaining: usize }

/// One analysis the user ran: a card on the Results tab.
pub(crate) struct SakhaResult {
    pub(crate) title: String,
    pub(crate) a: &'static Analysis,
    pub(crate) code: Vec<String>,
    pub(crate) statistic: Option<String>,
    pub(crate) says: String,
    pub(crate) output: String,
    /// The plot it drew.
    pub(crate) svg: Option<String>,
    pub(crate) running: bool,
}

/// Click targets from the last paint.
#[derive(Default)]
struct Hits {
    cards: Vec<(Rect, usize)>,
    list: Vec<(Rect, usize)>,
    faces: Vec<Rect>,
    back: Option<Rect>,
    run: Option<Rect>,
    as_category: Option<Rect>,
    density: Option<Rect>,
    cards_area: Option<Rect>,
    export: Option<Rect>,
    script_toggle: Option<Rect>,
    script_area: Option<Rect>,
    dataset: Option<Rect>,
    prev: Option<Rect>,
    next: Option<Rect>,
    open_csv: Option<Rect>,
    frames: Vec<(Rect, String)>,
    save: Option<Rect>,
    to_studio: Option<Rect>,
    rerun: Option<Rect>,
    open_doc: Option<Rect>,
    recover: Option<Rect>,
    /// Welcome-screen rows, each with the action it runs.
    links: Vec<(Rect, String)>,
}

pub(crate) struct Sakha {
    pub(crate) df: Option<String>,
    pub(crate) cols: Vec<ColumnProfile>,
    pub(crate) selected: Option<usize>,
    /// A saved selection, chosen again once the columns are back.
    pub(crate) reselect: Option<String>,
    pub(crate) face: Face,
    /// First visible card row.
    pub(crate) scroll: usize,
    /// The line that loaded the data frame.
    pub(crate) load: Option<String>,
    /// The `.sakha` file this analysis is saved in.
    pub(crate) document: Option<std::path::PathBuf>,
    /// Something has changed since the last save.
    pub(crate) dirty: bool,
    /// The data file differs from when the results were saved.
    pub(crate) data_changed: bool,
    /// A file just loaded with `load()`: the next dataset chosen came from it.
    pub(crate) pending_load: Option<String>,
    /// Bumped whenever the columns are asked for again: older answers drop.
    gen: u64,
    jobs: HashMap<usize, Job>,
    pub(crate) script: SessionScript,
    show_script: bool,
    script_scroll: usize,
    pub(crate) status: String,
    pub(crate) results: Vec<SakhaResult>,
    pub(crate) current: usize,
    pub(crate) result_scroll: usize,
    panel: GraphPanel,
    /// Which result the panel holds.
    pub(crate) panel_for: Option<usize>,
    /// The user ran something; re-read the columns when next in front.
    pub(crate) stale: bool,
    pub(crate) dialog: Option<AnalysisDialog>,
    pub(crate) data_uid: Option<u64>,
    /// The welcome screen, to choose or load a dataset.
    pub(crate) choosing: bool,
    hits: Hits,
}

impl Default for Sakha {
    fn default() -> Self {
        Sakha { df: None, cols: vec![], selected: None, reselect: None, face: Face::Overview, scroll: 0, load: None, document: None,
                dirty: false, data_changed: false, pending_load: None, gen: 0, jobs: HashMap::new(),
                script: SessionScript::default(), show_script: false, script_scroll: 0, status: String::new(), results: vec![],
                current: 0, result_scroll: 0, panel: GraphPanel::new(), panel_for: None, stale: false, dialog: None,
                data_uid: None, choosing: false, hits: Hits::default() }
    }
}

impl Sakha {
    /// R2 restarted: the data is gone, the results stay readable.
    pub(crate) fn engine_lost(&mut self) {
        self.df = None;
        self.cols.clear();
        self.jobs.clear();
        self.selected = None;
        self.dialog = None;
        self.gen += 1;
    }

    fn code(&self) -> Vec<String> {
        let Some(df) = &self.df else { return vec![] };
        match (self.face, self.selected) {
            (Face::Results, _) => self.results.get(self.current).map(|r| r.code.clone()).unwrap_or_default(),
            (_, Some(i)) => code_for(df, &self.cols[i]),
            _ => vec![format!("str({df})"), format!("summary({df})")],
        }
    }

    fn says(&self) -> String {
        match (self.face, self.selected) {
            (Face::Results, _) => self.results.get(self.current).map(|r| r.says.clone())
                .unwrap_or_else(|| "Choose an analysis from the menus above: Summarize, Compare, Relate or Plot.".into()),
            (_, Some(i)) => self.cols[i].says(),
            _ => "Click a card or a variable for its detail, or choose an analysis from the menus above. Everything you do is recorded in the session script.".into(),
        }
    }
}

impl Gui {
    /// Show `name` in Sakha. `load` is the line that loaded it (it starts the
    /// session script); the same data frame keeps its session.
    pub(crate) fn open_sakha(&mut self, name: &str, load: Option<String>) {
        self.set_mode(true);
        self.sakha.choosing = false;
        if self.sakha.df.as_deref() == Some(name) { return; }
        let load = load.or_else(|| self.sakha.pending_load.take()).or_else(|| load_line_from_history(name, self.buffer.history()));
        let s = &mut self.sakha;
        s.df = Some(name.to_string());
        s.cols.clear();
        s.selected = None;
        s.scroll = 0;
        s.face = Face::Overview;
        s.script = SessionScript::new(name, &today(), load.as_deref());
        s.load = load;
        // an analysis is of one dataset
        s.results.clear();
        s.current = 0;
        s.panel_for = None;
        s.document = None;
        s.dirty = false;
        s.data_changed = false;
        s.status.clear();
        if let Some(old) = s.data_uid.take() { self.views.remove(&old); }
        self.sakha.data_uid = Some(self.new_view(name));
        self.ask_columns();
    }

    fn ask_columns(&mut self) {
        let s = &mut self.sakha;
        let Some(df) = s.df.clone() else { return };
        s.gen += 1;
        s.jobs.clear();
        s.stale = false;
        let gen = s.gen;
        self.session.send("view", json!({ "code": df, "offset": 0, "limit": 0 }), Req::SakhaCols { df, gen });
    }

    /// The column list: unchanged columns keep their old figures on screen
    /// until the new ones arrive.
    pub(crate) fn sakha_cols_reply(&mut self, df: &str, gen: u64, body: &Value) {
        let s = &mut self.sakha;
        if gen != s.gen || s.df.as_deref() != Some(df) { return; }
        if body["ok"] != Value::Bool(true) || body["headers"].as_array().is_none() {
            s.status = format!("{df} cannot be shown: {}", body["error"].as_str().unwrap_or("it is not a data frame"));
            // a saved analysis keeps its results, even with no data to read
            if s.document.is_none() {
                s.df = None;
                s.choosing = true;
            }
            return;
        }
        let names = body["headers"].as_array().cloned().unwrap_or_default();
        let types = body["types"].as_array().cloned().unwrap_or_default();
        let mut cols: Vec<ColumnProfile> = names.iter().zip(&types)
            .map(|(n, t)| ColumnProfile::new(n.as_str().unwrap_or(""), Kind::from_view_type(t.as_str().unwrap_or("")))).collect();
        for c in &mut cols {
            // a text column may have become a category when it was read
            if let Some(old) = s.cols.iter().find(|o| o.name == c.name && (o.kind == c.kind || c.kind == Kind::Text)) {
                *c = ColumnProfile { ready: false, ..old.clone() };
            }
        }
        if s.selected.is_some_and(|i| i >= cols.len()) { s.selected = None; }
        s.cols = cols;
        if let Some(name) = s.reselect.take() { s.selected = s.cols.iter().position(|c| c.name == name); }
    }

    pub(crate) fn sakha_col_reply(&mut self, gen: u64, col: usize, slot: usize, body: &Value) {
        let s = &mut self.sakha;
        if gen != s.gen { return; }
        let Some(job) = s.jobs.get_mut(&col) else { return };
        if body["ok"] == Value::Bool(true) { job.answers[slot] = Some(body["value"].clone()); }
        job.remaining -= 1;
        if job.remaining == 0 {
            if let Some(job) = s.jobs.remove(&col) {
                if let Some(c) = s.cols.get_mut(col) { fill(c, &job.answers); }
            }
        }
    }

    /// Ask R2 about columns not yet described: the open detail first, then
    /// the cards on screen, then the rest in order.
    fn sakha_compute(&mut self) {
        let s = &self.sakha;
        let Some(df) = s.df.clone() else { return };
        let room = IN_FLIGHT.saturating_sub(s.jobs.len());
        if room == 0 { return; }
        let on_screen = s.hits.cards.iter().map(|&(_, i)| i);
        let order: Vec<usize> = s.selected.into_iter().chain(on_screen).chain(0..s.cols.len()).collect();
        let mut todo: Vec<usize> = vec![];
        for i in order {
            if s.cols.get(i).is_some_and(|c| !c.ready) && !s.jobs.contains_key(&i) && !todo.contains(&i) { todo.push(i); }
            if todo.len() == room { break; }
        }
        let gen = s.gen;
        for col in todo {
            let asks = queries(&df, &self.sakha.cols[col]);
            self.sakha.jobs.insert(col, Job { answers: vec![None; asks.len()], remaining: asks.len() });
            for (slot, q) in asks.iter().enumerate() {
                self.session.value(q, Req::SakhaCol { gen, col, slot });
            }
        }
    }

    /// The detail page's button: this variable's density, straight away.
    fn run_density(&mut self, col: usize) {
        let (Some(a), Some(df)) = (analysis::find("density"), self.sakha.df.clone()) else { return };
        if let Ok(built) = a.build(&df, &self.sakha.cols, &[vec![col]], "") { self.run_analysis(a, built); }
    }

    /// A menu item: open its dialog.
    pub(crate) fn sakha_menu(&mut self, id: &str) {
        let Some(a) = analysis::find(id) else { return };
        if self.sakha.df.is_none() {
            self.sakha.choosing = true;
            self.sakha.status = "First open a dataset.".into();
            return;
        }
        self.sakha.dialog = Some(AnalysisDialog::new(a, &self.sakha.cols, self.sakha.selected));
    }

    pub(crate) fn run_analysis(&mut self, a: &'static Analysis, built: Built) {
        let code = built.code.join("\n");
        let s = &mut self.sakha;
        s.script.record(&format!("run:{code}"), &built.title, &built.code);
        s.dirty = true;
        s.results.push(SakhaResult { title: built.title, a, code: built.code, statistic: built.statistic,
                                     says: "R2 is working…".into(), output: String::new(), svg: None, running: true });
        s.current = s.results.len() - 1;
        s.result_scroll = 0;
        s.face = Face::Results;
        s.selected = None;
        let rid = s.current;
        self.session.send("eval", json!({ "code": code }), Req::Analysis { rid });
    }

    pub(crate) fn result_output(&mut self, rid: usize, chunk: &str, _err: bool) {
        if let Some(r) = self.sakha.results.get_mut(rid) { r.output.push_str(chunk); }
    }

    pub(crate) fn result_done(&mut self, rid: usize, body: &Value) {
        let plotted = body["plot"].as_str().map(str::to_string);
        if let Some(svg) = &plotted { self.add_plot(svg.clone()); }
        // the same plot again is not sent twice: it is the current one
        let draws = self.sakha.results.get(rid).is_some_and(|r| r.a.menu == "Plot");
        let svg = plotted.or_else(|| if draws { self.plots.current().map(str::to_string) } else { None });
        let failed = body["error"] == Value::Bool(true);
        self.sakha.dirty = true;
        let Some(r) = self.sakha.results.get_mut(rid) else { return };
        r.running = false;
        r.svg = if failed { None } else { svg };
        r.says = if failed { "R2 could not run this; its message is below.".into() }
                 else if body["interrupted"] == Value::Bool(true) { "Stopped.".into() }
                 else { r.a.note().map(str::to_string).unwrap_or_else(|| format!("{}.", r.title)) };
        let ask = if failed { None } else { r.statistic.clone() };
        if self.sakha.current == rid { self.sakha.panel_for = None; }
        if draws && !failed { self.ask_whole_plot(Some(rid)); }
        if let Some(code) = ask { self.session.value(&code, Req::Statistic { rid }); }
    }

    /// The whole plot arrived: a result that drew it shows it.
    pub(crate) fn result_plot(&mut self, rid: usize, svg: String) {
        if let Some(r) = self.sakha.results.get_mut(rid) { r.svg = Some(svg); }
        if self.sakha.current == rid { self.sakha.panel_for = None; }
    }

    pub(crate) fn result_statistic(&mut self, rid: usize, body: &Value) {
        let Some(r) = self.sakha.results.get_mut(rid) else { return };
        let stat = profile::reals(&body["value"]).first().copied().unwrap_or(f64::NAN);
        if let Some(line) = r.a.says(stat) { r.says = line; }
        self.sakha.dirty = true;
    }

    /// One of R2's own datasets, bound in the session so Environment lists it.
    pub(crate) fn load_builtin(&mut self, name: &str) {
        let line = format!("{name} <- {name}");
        self.run_load(line, name.to_string());
    }

    /// An R2 data file (`.r2s`): it may hold several variables, so the user
    /// chooses among them. R2 currently loses factor columns in save and load,
    /// so the Console says so each time.
    pub(crate) fn load_r2_file(&mut self, path: &std::path::Path) {
        let p = sakha_core::document::r_path(path.strip_prefix(&self.session.cwd).unwrap_or(path));
        let line = format!("load(\"{p}\")");
        let note = "Note: R2's save() and load() currently lose factor columns. Check the variables; a CSV keeps them.";
        self.buffer.push_error(note);
        if self.sakha_mode { self.sakha.status = note.into(); }
        let after = self.sakha_mode.then(|| After::ChooseInSakha { load: line.clone() });
        self.run_queue.push_back((line, after));
    }

    /// The button in Sakha's header, as Rcmdr's data-set button: what is loaded, and more.
    fn open_dataset_menu(&mut self, button: Rect) {
        let current = self.sakha.df.clone();
        let mut items: Vec<ContextItem> = self.env.rows.iter().filter(|r| r.is_data_frame()).map(|r| {
            let mark = if current.as_deref() == Some(r.name.as_str()) { "* " } else { "  " };
            ContextItem::new(format!("{mark}{}  ({})", r.name, r.summary), format!("sakha.df:{}", r.name))
        }).collect();
        if !items.is_empty() { items.push(ContextItem::separator()); }
        items.push(ContextItem::new("Load new dataset…", "data.load"));
        items.push(ContextItem::new("Variable names and types…", "data.attributes"));
        self.dataset_menu = ContextMenu::new(items);
        self.dataset_menu.open_at(button.x, button.y + button.h);
    }

    pub(crate) fn sakha_action(&mut self, action: &str) {
        match action {
            "sakha.choose" => self.sakha.choosing = true,
            "sakha.export" => self.export_session(),
            "sakha.to_studio" => {
                // a new script to fine-tune: not saved until the user saves it
                let mut ed = r2_editor::Editor::from_text(&self.sakha.script.text());
                ed.dirty = true;
                self.set_mode(false);
                self.add_script(ed);
            }
            "sakha.script" => self.sakha.show_script = !self.sakha.show_script,
            "sakha.overview" => { self.sakha.face = Face::Overview; self.sakha.selected = None; }
            "sakha.data" => self.sakha.face = Face::Data,
            "sakha.results" => self.sakha.face = Face::Results,
            a => {
                if let Some(id) = a.strip_prefix("sakha.run.") { self.sakha_menu(id); }
                if let Some(df) = a.strip_prefix("sakha.df:") { self.open_sakha(df, None); }
            }
        }
    }

    fn export_session(&mut self) {
        let Some(df) = self.sakha.df.clone() else { return };
        let Some(mut path) = rfd::FileDialog::new().set_title("Export Sakha script").set_directory(&self.session.cwd)
            .set_file_name(format!("{}.{EXT}", default_script_name(&df)))
            .add_filter("R2 script", &[EXT]).save_file() else { return };
        if path.extension().is_none() { path.set_extension(EXT); }
        let shown = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.sakha.status = match std::fs::write(&path, self.sakha.script.text()) {
            Ok(()) => format!("Saved {shown}"),
            Err(e) => format!("Could not save {shown}: {e}"),
        };
    }

    pub(crate) fn sakha_input(&mut self, events: &[InputEvent]) {
        if self.sakha.stale && self.sakha.df.is_some() { self.ask_columns(); }
        self.sakha_compute();
        let mouse = self.mouse;
        let busy = self.session.busy();
        let mut apply: Option<String> = None;
        let mut to_studio: Option<Vec<String>> = None;
        let mut queued: Option<String> = None;
        let (mut export, mut save, mut rerun) = (false, false, false);
        let mut density: Option<usize> = None;
        let mut open_dataset: Option<Rect> = None;
        {
            let s = &mut self.sakha;
            for ev in events {
                match *ev {
                    InputEvent::MouseDown { button: MouseButton::Left, pos } => {
                        let (x, y) = (pos.x, pos.y);
                        let hit = |r: &Option<Rect>| r.is_some_and(|r| inside(r, x, y));
                        if let Some((_, name)) = s.hits.frames.iter().find(|(r, _)| inside(*r, x, y)) { queued = Some(format!("sakha.df:{name}")); }
                        else if let Some((_, a)) = s.hits.links.iter().find(|(r, _)| inside(*r, x, y)) { queued = Some(a.clone()); }
                        else if hit(&s.hits.open_csv) { queued = Some("data.load".into()); }
                        else if hit(&s.hits.open_doc) { queued = Some("doc.open".into()); }
                        else if hit(&s.hits.recover) { queued = Some("doc.recover".into()); }
                        else if hit(&s.hits.save) { save = true; }
                        else if hit(&s.hits.to_studio) { queued = Some("sakha.to_studio".into()); }
                        else if hit(&s.hits.rerun) { rerun = true; }
                        else if hit(&s.hits.dataset) { open_dataset = s.hits.dataset; }
                        else if let Some(k) = s.hits.faces.iter().position(|r| inside(*r, x, y)) {
                            s.face = [Face::Overview, Face::Data, Face::Results][k];
                        } else if let Some(&(_, i)) = s.hits.cards.iter().chain(s.hits.list.iter()).find(|(r, _)| inside(*r, x, y)) {
                            s.selected = Some(i);
                            s.face = Face::Overview;
                            let c = &s.cols[i];
                            let what = match c.kind { Kind::Number => "summary and 95% CI of the mean",
                                                      Kind::Category => "counts per group", Kind::Text => "distinct values" };
                            if let Some(df) = &s.df {
                                if s.script.record(&format!("col:{}", c.name), &format!("{}: {what}", c.name), &code_for(df, c)) { s.dirty = true; }
                            }
                        } else if hit(&s.hits.back) { s.selected = None; }
                        else if hit(&s.hits.script_toggle) { s.show_script = !s.show_script; }
                        else if hit(&s.hits.export) { export = true; }
                        else if hit(&s.hits.run) { to_studio = Some(s.code()); }
                        else if hit(&s.hits.prev) { s.current = s.current.saturating_sub(1); s.result_scroll = 0; }
                        else if hit(&s.hits.next) { s.current = (s.current + 1).min(s.results.len().saturating_sub(1)); s.result_scroll = 0; }
                        else if hit(&s.hits.density) { density = s.selected; }
                        else if hit(&s.hits.as_category) {
                            if let (Some(c), Some(df)) = (s.selected.map(|i| &s.cols[i]), &s.df) {
                                let code = code_as_category(df, c);
                                s.script.record(&format!("cat:{}", c.name), &format!("{} treated as a category", c.name), std::slice::from_ref(&code));
                                s.dirty = true;
                                apply = Some(code);
                            }
                        }
                    }
                    InputEvent::Key { code: KeyCode::KeyS, mods, pressed: true } if mods.ctrl => save = true,
                    InputEvent::Key { code: KeyCode::Escape, pressed: true, .. } if !busy => { s.selected = None; s.choosing = false; }
                    InputEvent::Scroll { dy } if s.hits.script_area.is_some_and(|r| inside(r, mouse.0, mouse.1)) => {
                        s.script_scroll = if dy > 0.0 { s.script_scroll.saturating_sub(3) } else { s.script_scroll + 3 };
                    }
                    InputEvent::Scroll { dy } if s.face == Face::Results && s.hits.cards_area.is_some_and(|r| inside(r, mouse.0, mouse.1)) => {
                        s.result_scroll = if dy > 0.0 { s.result_scroll.saturating_sub(3) } else { s.result_scroll + 3 };
                    }
                    InputEvent::Scroll { dy } if s.face == Face::Overview && s.hits.cards_area.is_some_and(|r| inside(r, mouse.0, mouse.1)) => {
                        s.scroll = if dy > 0.0 { s.scroll.saturating_sub(1) } else { s.scroll + 1 };
                    }
                    _ => {}
                }
            }
        }
        if self.sakha.face == Face::Data {
            if let Some(v) = self.sakha.data_uid.and_then(|u| self.views.get_mut(&u)) { v.input(events, true, true); }
        }
        if queued.is_some() { self.queued = queued; }
        if let Some(r) = open_dataset { self.open_dataset_menu(r); }
        if save { self.save_document(false); }
        if rerun { self.rerun_results(); }
        if let Some(col) = density { self.run_density(col); }
        if export { self.export_session(); }
        if let Some(code) = apply { self.submit(code); }
        if let Some(code) = to_studio {
            self.set_mode(false);
            self.run_code(&code.join("\n"));
        }
    }

    pub(crate) fn paint_sakha(&mut self, c: Rect, frame: &mut Frame, renderer: &mut Renderer, theme: &Theme) {
        let (cw, lh) = renderer.cell_metrics(theme.fs());
        let (bg, fg, size) = (theme.window_background, theme.console_output, theme.fs());
        let dim = mix(fg, bg, 0.45);
        frame.paint_rect(c.x, c.y, c.w, c.h, bg);
        self.sakha.hits = Hits::default();

        if self.sakha.df.is_none() || self.sakha.choosing {
            self.paint_welcome(c, frame, renderer, theme);
            return;
        }

        // ── header
        let head_h = lh + 10.0;
        frame.paint_rect(c.x, c.y, c.w, head_h, mix(bg, fg, 0.06));
        let df = self.sakha.df.clone().unwrap_or_default();
        // the file's name, with * while there are unsaved changes
        let doc = self.sakha.document.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "not saved".into());
        let line = if self.sakha.cols.is_empty() { "data not loaded".to_string() } else { dataset_line(&self.sakha.cols) };
        let head = format!("{line}  |  {doc}{}", if self.sakha.dirty { " *" } else { "" });
        let (by, bh) = (c.y + 2.0, head_h - 4.0);
        let mut right = c.x + c.w - 6.0;
        let steps = self.sakha.script.steps();
        let r = button_left_of(frame, renderer, theme, right, by, bh, &format!("Export script · {steps}"), false);
        self.sakha.hits.export = Some(r); right = r.x - 6.0;
        let r = button_left_of(frame, renderer, theme, right, by, bh, "Save", self.sakha.dirty);
        self.sakha.hits.save = Some(r); right = r.x - 6.0;
        let show = self.sakha.show_script;
        let r = button_left_of(frame, renderer, theme, right, by, bh, "Script", show);
        self.sakha.hits.script_toggle = Some(r); right = r.x - 6.0;
        let r = button_left_of(frame, renderer, theme, right, by, bh, "Edit in Studio", false);
        self.sakha.hits.to_studio = Some(r); right = r.x - 6.0;
        right -= 2.0;
        if self.sakha.data_changed {
            let r = button_left_of(frame, renderer, theme, right, by, bh, "Run all again", true);
            self.sakha.hits.rerun = Some(r); right = r.x - 8.0;
        }
        if !self.sakha.status.is_empty() {
            let w = (self.sakha.status.chars().count() + 1) as f32 * cw;
            let color = if self.sakha.status.starts_with("Could not") { theme.console_error } else { dim };
            text(frame, renderer, right - w, c.y + 5.0 + lh * 0.8, &self.sakha.status, size, color, cw, right);
            right -= w;
        }
        // the dataset button leads the line, as the data leads Sakha
        let label = format!("Dataset: {df} ▾");
        let w = (label.chars().count() + 2) as f32 * cw;
        let dataset = Rect { x: c.x + 4.0, y: by, w, h: bh };
        button(frame, renderer, theme, dataset, &label, self.dataset_menu.is_open());
        self.sakha.hits.dataset = Some(dataset);
        text(frame, renderer, dataset.x + w + cw, c.y + 5.0 + lh * 0.8, &head, size, theme.console_banner, cw, right);

        // ── bottom: "Sakha says" and the code strip
        let foot_h = 3.0 * lh + 14.0;
        let body = Rect { x: c.x, y: c.y + head_h + 2.0, w: c.w, h: (c.h - head_h - 2.0 - foot_h).max(0.0) };
        let fy = c.y + c.h - foot_h;
        frame.paint_rect(c.x, fy, c.w, foot_h, mix(bg, fg, 0.05));
        let max_chars = ((c.w - 16.0) / cw).floor().max(10.0) as usize;
        for (k, line) in wrap(&self.sakha.says(), max_chars).into_iter().take(2).enumerate() {
            text(frame, renderer, c.x + 8.0, fy + 4.0 + lh * (0.8 + k as f32), &line, size, fg, cw, c.x + c.w - 8.0);
        }
        let code = self.sakha.code().join("; ");
        let cy = fy + 6.0 + lh * 2.0;
        frame.paint_rect(c.x + 4.0, cy - 2.0, c.w - 8.0, lh + 4.0, mix(bg, fg, 0.10));
        let run = button_left_of(frame, renderer, theme, c.x + c.w - 6.0, cy - 2.0, lh + 4.0, "Run in Studio", false);
        self.sakha.hits.run = Some(run);
        text(frame, renderer, c.x + 8.0, cy + lh * 0.8, &format!("Code  {code}"), size, theme.console_input, cw, run.x - cw);

        // ── Variables list
        let lw = LIST_W as f32 * cw;
        frame.paint_rect(body.x, body.y, lw, body.h, mix(bg, fg, 0.03));
        text(frame, renderer, body.x + 8.0, body.y + lh * 0.8, "Variables", size, dim, cw, body.x + lw);
        let rows = ((body.h - lh) / lh).floor().max(0.0) as usize;
        for k in 0..self.sakha.cols.len().min(rows) {
            let col = &self.sakha.cols[k];
            let y = body.y + lh * (k + 1) as f32;
            let r = Rect { x: body.x, y, w: lw, h: lh };
            let sel = self.sakha.selected == Some(k);
            if sel { frame.paint_rect(r.x, r.y, r.w, r.h, theme.console_selection_bg); }
            let mark = match col.kind { Kind::Number => '#', Kind::Category => '*', Kind::Text => '~' };
            let name: String = col.name.chars().take(LIST_W - 4).collect();
            text(frame, renderer, body.x + 8.0, y + lh * 0.8, &format!("{mark} {name}"), size,
                 if sel { theme.console_selection_fg } else { fg }, cw, body.x + lw);
            self.sakha.hits.list.push((r, k));
        }

        // ── the script panel on the right, only while it is on
        let mut area = Rect { x: body.x + lw + 10.0, y: body.y, w: body.w - lw - 18.0, h: body.h };
        if self.sakha.show_script {
            let sw = (SCRIPT_W as f32 * cw).min(area.w * 0.4);
            area.w -= sw + 8.0;
            self.paint_script_panel(Rect { x: area.x + area.w + 8.0, y: body.y, w: sw, h: body.h }, frame, renderer, theme);
        }

        // ── the tabs, and what they show
        let strip = lh + 8.0;
        let labels = vec!["Overview".to_string(), "Data".to_string(), format!("Results ({})", self.sakha.results.len())];
        let active = match self.sakha.face { Face::Overview => 0, Face::Data => 1, Face::Results => 2 };
        self.sakha.hits.faces = tabs(frame, renderer, theme, area.x, area.y, strip, area.x + area.w, &labels, active);
        let main = Rect { x: area.x, y: area.y + strip + 4.0, w: area.w, h: (area.h - strip - 4.0).max(0.0) };
        self.sakha.hits.cards_area = Some(main);
        match self.sakha.face {
            Face::Overview => {
                if let Some(i) = self.sakha.selected { self.paint_detail(i, main, strip, area, frame, renderer, theme); }
                else { self.paint_cards(main, frame, renderer, theme); }
            }
            Face::Data => {
                if let Some(uid) = self.sakha.data_uid {
                    if let Some(v) = self.views.get_mut(&uid) { v.paint(main, frame, renderer, theme); }
                    self.fetch_view_pages(uid);
                }
            }
            Face::Results => self.paint_results(main, strip, area, frame, renderer, theme),
        }
    }

    /// No dataset yet: open a file, or choose a data frame in the session.
    fn paint_welcome(&mut self, c: Rect, frame: &mut Frame, renderer: &mut Renderer, theme: &Theme) {
        let (cw, lh) = renderer.cell_metrics(theme.fs());
        let (bg, fg, size) = (theme.window_background, theme.console_output, theme.fs());
        let dim = mix(fg, bg, 0.45);
        let w = (64.0 * cw).min(c.w - 40.0);
        let x = c.x + (c.w - w) / 2.0;
        let mut y = c.y + c.h * 0.16;
        text(frame, renderer, x, y, "Sakha", size * 1.6, theme.console_banner, cw * 1.6, x + w);
        y += lh * 1.6;
        for line in wrap("Statistics from menus: open your data, choose an analysis, and read the result. No code to write; R2 does the computing, and everything you do is kept as a script.", (w / cw) as usize) {
            text(frame, renderer, x, y, &line, size, fg, cw, x + w);
            y += lh;
        }
        y += lh;
        let r = Rect { x, y, w: 24.0 * cw, h: lh + 10.0 };
        button(frame, renderer, theme, r, "Load new dataset…", true);
        self.sakha.hits.open_csv = Some(r);
        let r2 = Rect { x: r.x + r.w + 10.0, ..r };
        button(frame, renderer, theme, r2, "Open saved analysis…", false);
        self.sakha.hits.open_doc = Some(r2);
        y += r.h + lh;
        if self.recover.is_some() {
            let r = Rect { x, y, w, h: lh + 6.0 };
            button(frame, renderer, theme, r, "Recover the analysis left unsaved last time", false);
            self.sakha.hits.recover = Some(r);
            y += r.h + lh;
        }
        if !self.sakha.status.is_empty() {
            text(frame, renderer, x, y, &self.sakha.status, size, theme.console_error, cw, x + w);
            y += lh * 1.5;
        }
        let frames: Vec<(String, String)> = self.env.rows.iter().filter(|r| r.is_data_frame())
            .map(|r| (r.name.clone(), r.summary.clone())).collect();
        if !frames.is_empty() {
            text(frame, renderer, x, y + lh * 0.8, "Or use data already in this session:", size, dim, cw, x + w);
            y += lh * 1.3;
            for (name, summary) in frames {
                let r = Rect { x, y, w, h: lh + 6.0 };
                frame.paint_rect(r.x, r.y, r.w, r.h, mix(bg, fg, 0.07));
                text(frame, renderer, x + 6.0, y + 3.0 + lh * 0.8, &format!("{name}  {summary}"), size, theme.console_input, cw, x + w);
                self.sakha.hits.frames.push((r, name));
                y += r.h + 4.0;
            }
            y += lh;
        }
        let saved: Vec<String> = self.recent.analyses.iter().filter(|p| std::path::Path::new(p).is_file()).cloned().collect();
        if !saved.is_empty() {
            text(frame, renderer, x, y + lh * 0.8, "Saved analyses, latest first:", size, dim, cw, x + w);
            y += lh * 1.3;
            for p in saved {
                let r = Rect { x, y, w, h: lh + 6.0 };
                frame.paint_rect(r.x, r.y, r.w, r.h, mix(bg, fg, 0.07));
                // the file's name and its folder, which matter more than the full path
                let path = std::path::Path::new(&p);
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let folder = path.parent().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                text(frame, renderer, x + 6.0, y + 3.0 + lh * 0.8, &format!("{name}   in {folder}"), size, theme.console_input, cw, x + w);
                self.sakha.hits.links.push((r, format!("doc.open:{p}")));
                y += r.h + 4.0;
            }
        }
    }

    /// The session script, read-only, long lines wrapped.
    fn paint_script_panel(&mut self, r: Rect, frame: &mut Frame, renderer: &mut Renderer, theme: &Theme) {
        let (cw, lh) = renderer.cell_metrics(theme.fs());
        let (bg, fg, size) = (theme.window_background, theme.console_output, theme.fs());
        let dim = mix(fg, bg, 0.45);
        frame.paint_rect(r.x, r.y, r.w, r.h, mix(bg, fg, 0.04));
        frame.paint_rect(r.x, r.y, r.w, lh + 6.0, mix(bg, fg, 0.10));
        text(frame, renderer, r.x + 6.0, r.y + 3.0 + lh * 0.8, &format!("Session script (.{EXT})"), size, theme.console_banner, cw, r.x + r.w);
        let top = r.y + lh + 10.0;
        let height = (r.h - lh - 10.0).max(0.0);
        self.sakha.hits.script_area = Some(Rect { x: r.x, y: top, w: r.w, h: height });
        let width = ((r.w - 12.0) / cw).floor().max(10.0) as usize;
        let lines: Vec<String> = self.sakha.script.text().lines().flat_map(|l| {
            // a wrapped comment keeps its "#" so it still reads as one
            let comment = l.starts_with('#');
            wrap(l, width).into_iter().enumerate()
                .map(move |(k, w)| match (k, comment) { (0, _) => w, (_, true) => format!("#   {w}"), _ => format!("    {w}") })
        }).collect();
        let fit = (height / lh).floor().max(1.0) as usize;
        self.sakha.script_scroll = self.sakha.script_scroll.min(lines.len().saturating_sub(fit));
        for (k, l) in lines.iter().skip(self.sakha.script_scroll).take(fit).enumerate() {
            let color = if l.starts_with('#') { dim } else { fg };
            text(frame, renderer, r.x + 6.0, top + lh * (0.8 + k as f32), l, size, color, cw, r.x + r.w - 4.0);
        }
    }

    fn paint_cards(&mut self, area: Rect, frame: &mut Frame, renderer: &mut Renderer, theme: &Theme) {
        let (cw, lh) = renderer.cell_metrics(theme.fs());
        let (bg, fg, size) = (theme.window_background, theme.console_output, theme.fs());
        let (card_w, card_h, gap) = (CARD_W as f32 * cw, CARD_H as f32 * lh, 10.0);
        let per_row = (((area.w + gap) / (card_w + gap)).floor() as usize).max(1);
        let rows_fit = (((area.h + gap) / (card_h + gap)).floor() as usize).max(1);
        let s = &mut self.sakha;
        let total_rows = s.cols.len().div_ceil(per_row);
        s.scroll = s.scroll.min(total_rows.saturating_sub(rows_fit));
        for (k, col) in s.cols.iter().enumerate().skip(s.scroll * per_row).take(per_row * rows_fit) {
            let slot = k - s.scroll * per_row;
            let x = area.x + (slot % per_row) as f32 * (card_w + gap);
            let y = area.y + (slot / per_row) as f32 * (card_h + gap);
            frame.paint_rect(x, y, card_w, card_h, mix(bg, fg, 0.07));
            frame.paint_rect(x, y, card_w, lh + 4.0, mix(bg, fg, 0.12));
            // the name, then its kind, unless a flag needs the room
            let flagged = col.missing > 0 || col.looks_categorical;
            let title = if flagged { col.name.clone() } else { format!("{}  {}", col.name, col.kind.label()) };
            let title: String = title.chars().take(if flagged { CARD_W - 11 } else { CARD_W - 1 }).collect();
            text(frame, renderer, x + 6.0, y + 2.0 + lh * 0.8, &title, size, theme.console_banner, cw, x + card_w - 4.0);
            let chart = Rect { x: x + 8.0, y: y + lh + 10.0, w: card_w - 16.0, h: 2.6 * lh };
            let mut line = String::new();
            match (&col.num, col.kind) {
                (Some(n), Kind::Number) => {
                    bars(frame, chart, &n.bins, theme.console_input);
                    line = format!("mean {}  sd {}", fmt_short(n.mean), fmt_short(n.sd));
                }
                _ if col.kind == Kind::Text => {
                    for (j, (lv, _)) in col.levels.iter().take(3).enumerate() {
                        let label: String = lv.chars().take(CARD_W - 4).collect();
                        text(frame, renderer, chart.x, chart.y + j as f32 * (chart.h / 3.0) + lh * 0.75, &label, size, mix(fg, bg, 0.35), cw, chart.x + chart.w);
                    }
                    line = format!("{} distinct", col.levels.len());
                }
                _ if !col.levels.is_empty() => {
                    // up to three horizontal proportion bars
                    let present = (col.n - col.missing).max(1) as f32;
                    for (j, (lv, n)) in col.levels.iter().take(3).enumerate() {
                        let by = chart.y + j as f32 * (chart.h / 3.0);
                        let label: String = lv.chars().take(6).collect();
                        text(frame, renderer, chart.x, by + lh * 0.75, &label, size * 0.85, fg, cw, chart.x + 7.0 * cw);
                        let bw = (chart.w - 8.0 * cw) * *n as f32 / present;
                        frame.paint_rect(chart.x + 7.0 * cw, by + 3.0, bw.max(1.0), chart.h / 3.0 - 6.0, theme.console_input);
                    }
                    line = format!("{} groups", col.levels.len());
                }
                _ => {}
            }
            if !col.ready && line.is_empty() { line = "asking R2…".into(); }
            text(frame, renderer, x + 6.0, y + card_h - lh * 1.2, &line, size, fg, cw, x + card_w - 4.0);
            // flags ride on the title bar: missing values, or a number that
            // looks like a category
            if flagged {
                let flag = if col.looks_categorical { "category?".to_string() } else { format!("{} NA", col.missing) };
                let fx = x + card_w - (flag.chars().count() + 1) as f32 * cw;
                frame.paint_rect(fx - cw * 0.5, y, card_w - (fx - x) + cw * 0.5, lh + 4.0, mix(bg, fg, 0.12));
                text(frame, renderer, fx, y + 2.0 + lh * 0.8, &flag, size, theme.console_error, cw, x + card_w);
            }
            s.hits.cards.push((Rect { x, y, w: card_w, h: card_h }, k));
        }
        if total_rows > rows_fit {
            let more = format!("rows {}–{} of {}", s.scroll + 1, (s.scroll + rows_fit).min(total_rows), total_rows);
            text(frame, renderer, area.x + area.w - (more.len() + 1) as f32 * cw, area.y + area.h - 4.0, &more, size, mix(fg, bg, 0.4), cw, area.x + area.w);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_detail(&mut self, i: usize, area: Rect, strip: f32, tabs_area: Rect, frame: &mut Frame, renderer: &mut Renderer, theme: &Theme) {
        let (cw, lh) = renderer.cell_metrics(theme.fs());
        let (bg, fg, size) = (theme.window_background, theme.console_output, theme.fs());
        let dim = mix(fg, bg, 0.45);
        let back = button_left_of(frame, renderer, theme, tabs_area.x + tabs_area.w, tabs_area.y + 2.0, strip - 4.0, "← All variables", false);
        self.sakha.hits.back = Some(back);
        let s = &mut self.sakha;
        let col = &s.cols[i];
        if !col.ready && col.num.is_none() && col.levels.is_empty() {
            text(frame, renderer, area.x, area.y + lh * 0.8, &format!("{}: asking R2…", col.name), size, dim, cw, area.x + area.w);
            return;
        }
        let title = format!("{}  ({}, {} rows, {} missing)", col.name, col.kind.label(), col.n, col.missing);
        text(frame, renderer, area.x, area.y + lh * 0.8, &title, size, theme.console_banner, cw, area.x + area.w);
        let stats_w = 36.0 * cw;
        let chart = Rect { x: area.x, y: area.y + lh * 1.6, w: (area.w - stats_w - 20.0).max(80.0), h: (area.h - lh * 3.4).max(40.0) };
        frame.paint_rect(chart.x, chart.y, chart.w, chart.h, mix(bg, fg, 0.04));
        let sx = chart.x + chart.w + 20.0;
        let stats: Vec<(&str, String)>;
        match (&col.num, col.kind) {
            (Some(n), Kind::Number) => {
                let plot = Rect { x: chart.x + 6.0, y: chart.y + 6.0, w: chart.w - 12.0, h: chart.h - 12.0 };
                // bars only, as `hist()` draws them: the mean and its interval are
                // numbers in the list beside it, not marks on a histogram
                let lo = n.breaks.first().copied().unwrap_or(n.min);
                let hi = n.breaks.last().copied().unwrap_or(n.max);
                bars(frame, plot, &n.bins, theme.console_input);
                text(frame, renderer, chart.x, chart.y + chart.h + lh * 0.9, &fmt(lo), size, dim, cw, chart.x + chart.w);
                let mx = fmt(hi);
                text(frame, renderer, chart.x + chart.w - mx.len() as f32 * cw, chart.y + chart.h + lh * 0.9, &mx, size, dim, cw, chart.x + chart.w + 4.0 * cw);
                stats = vec![("mean", fmt(n.mean)), ("95% CI", format!("{} – {}", fmt(n.ci.0), fmt(n.ci.1))),
                             ("sd", fmt(n.sd)), ("min", fmt(n.min)), ("1st qu.", fmt(n.q1)), ("median", fmt(n.median)),
                             ("3rd qu.", fmt(n.q3)), ("max", fmt(n.max)), ("distinct", n.distinct.to_string())];
                let a = Rect { x: sx, y: chart.y + lh * 14.2, w: 30.0 * cw, h: lh + 6.0 };
                button(frame, renderer, theme, a, "Name and type…", false);
                s.hits.links.push((a, "data.attributes".into()));
                let r = Rect { x: sx, y: chart.y + lh * 12.4, w: 30.0 * cw, h: lh + 6.0 };
                button(frame, renderer, theme, r, "Density curve with 95% CI", false);
                s.hits.density = Some(r);
                if col.looks_categorical {
                    let r = Rect { x: sx, y: chart.y + lh * 10.4, w: 22.0 * cw, h: lh + 6.0 };
                    button(frame, renderer, theme, r, "Treat as category", false);
                    s.hits.as_category = Some(r);
                }
            }
            _ => {
                // every level (up to what fits) as a bar with count and share
                let present = (col.n - col.missing).max(1) as f32;
                let fit = ((chart.h - 8.0) / lh).floor().max(1.0) as usize;
                for (k, (lv, n)) in col.levels.iter().take(fit).enumerate() {
                    let y = chart.y + 4.0 + k as f32 * lh;
                    let label: String = lv.chars().take(14).collect();
                    text(frame, renderer, chart.x + 6.0, y + lh * 0.8, &label, size, fg, cw, chart.x + 16.0 * cw);
                    let room = chart.w - 30.0 * cw;
                    frame.paint_rect(chart.x + 16.0 * cw, y + 3.0, (room * *n as f32 / present).max(1.0), lh - 6.0, theme.console_input);
                    let lab = format!("{n} ({:.0}%)", 100.0 * *n as f32 / present);
                    text(frame, renderer, chart.x + chart.w - 13.0 * cw, y + lh * 0.8, &lab, size, dim, cw, chart.x + chart.w);
                }
                stats = vec![("groups", col.levels.len().to_string()), ("rows", col.n.to_string()), ("missing", col.missing.to_string())];
            }
        }
        for (k, (label, val)) in stats.iter().enumerate() {
            let y = chart.y + lh * (0.8 + k as f32);
            text(frame, renderer, sx, y, label, size, dim, cw, sx + 12.0 * cw);
            text(frame, renderer, sx + 12.0 * cw, y, val, size, fg, cw, area.x + area.w);
        }
    }

    /// The current result card: its title, R2's own output, and its plot.
    #[allow(clippy::too_many_arguments)]
    fn paint_results(&mut self, area: Rect, strip: f32, tabs_area: Rect, frame: &mut Frame, renderer: &mut Renderer, theme: &Theme) {
        let (cw, lh) = renderer.cell_metrics(theme.fs());
        let (bg, fg, size) = (theme.window_background, theme.console_output, theme.fs());
        let dim = mix(fg, bg, 0.45);
        let n = self.sakha.results.len();
        if n == 0 {
            text(frame, renderer, area.x, area.y + lh * 0.8, "No results yet. Choose an analysis from the Summarize, Compare, Relate or Plot menus.", size, dim, cw, area.x + area.w);
            return;
        }
        // step through the results, at the tab strip's right end
        let (by, bh) = (tabs_area.y + 2.0, strip - 4.0);
        let mut right = tabs_area.x + tabs_area.w;
        let r = button_left_of(frame, renderer, theme, right, by, bh, "›", false);
        self.sakha.hits.next = Some(r); right = r.x - 2.0;
        let label = format!("{}/{n}", self.sakha.current + 1);
        let w = (label.chars().count() + 1) as f32 * cw;
        text(frame, renderer, right - w, by + (bh - lh) / 2.0 + lh * 0.8, &label, size, dim, cw, right);
        right -= w + 2.0;
        let r = button_left_of(frame, renderer, theme, right, by, bh, "‹", false);
        self.sakha.hits.prev = Some(r);

        let current = self.sakha.current;
        // the panel holds one result's plot; re-rasterise only on a change
        if self.sakha.panel_for != Some(current) {
            self.sakha.panel_for = Some(current);
            match self.sakha.results[current].svg.clone() {
                Some(svg) => self.sakha.panel.set_svg(svg.into_bytes()),
                None => self.sakha.panel.clear(),
            }
        }
        let s = &mut self.sakha;
        let res = &s.results[current];
        text(frame, renderer, area.x, area.y + lh * 0.8, &res.title, size, theme.console_banner, cw, area.x + area.w);
        let top = area.y + lh * 1.4;
        let h = (area.h - lh * 1.4).max(0.0);
        let lines: Vec<&str> = res.output.lines().collect();
        let has_plot = res.svg.is_some();
        // a plot with no output takes the whole card; with output, half of it
        let text_w = match (has_plot, lines.is_empty()) { (true, true) => 0.0, (true, false) => area.w * 0.5, _ => area.w };
        if text_w > 0.0 {
            let fit = (h / lh).floor().max(1.0) as usize;
            s.result_scroll = s.result_scroll.min(lines.len().saturating_sub(fit));
            for (k, l) in lines.iter().skip(s.result_scroll).take(fit).enumerate() {
                let color = if l.starts_with("Error") { theme.console_error } else { fg };
                text(frame, renderer, area.x, top + lh * (0.8 + k as f32), l, size, color, cw, area.x + text_w - 6.0);
            }
        }
        if has_plot {
            let p = Rect { x: area.x + text_w, y: top, w: area.w - text_w, h };
            frame.paint_rect(p.x, p.y, p.w, p.h, Color::WHITE);
            s.panel.paint(frame, renderer, Rect { x: p.x + 8.0, y: p.y + 8.0, w: (p.w - 16.0).max(0.0), h: (p.h - 16.0).max(0.0) }, theme);
        }
    }
}

/// Bars for counts in `r`, bottom-aligned (a histogram).
fn bars(frame: &mut Frame, r: Rect, counts: &[usize], color: Color) {
    let max = counts.iter().copied().max().unwrap_or(0).max(1) as f32;
    let w = r.w / counts.len().max(1) as f32;
    for (k, &n) in counts.iter().enumerate() {
        let h = r.h * n as f32 / max;
        frame.paint_rect(r.x + k as f32 * w + 1.0, r.y + r.h - h, (w - 2.0).max(1.0), h, color);
    }
}
