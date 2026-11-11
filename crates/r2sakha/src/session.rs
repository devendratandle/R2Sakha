//! The R2 session behind both faces of the app. Every request to `r2 --ide`
//! is recorded here with what its reply is for, and replies are routed once
//! per frame, so the window never freezes while R2 computes.

use std::collections::HashMap;
use std::path::PathBuf;

use sakha_engine::{find_r2, json, Engine, Msg, Value};

use crate::console::{classify, Typed};
use crate::Gui;

/// What a request's reply is for.
pub(crate) enum Req {
    /// Console input; its output goes to the transcript.
    Console { src: String, after: Option<After> },
    Env,
    Getwd,
    /// A page of the data view `uid`.
    ViewPage { uid: u64, page: usize, gen: u64 },
    /// The column list of Sakha's data frame.
    SakhaCols { df: String, gen: u64 },
    /// One answer about a column.
    SakhaCol { gen: u64, col: usize, slot: usize },
    /// A menu analysis; its output goes to result `rid`.
    Analysis { rid: u64 },
    /// The statistic its plain-words line reads.
    Statistic { rid: u64 },
    /// The numbers its table reads.
    Numbers { rid: u64 },
    /// A Gview graph drawn again; then its whole plot, written to a file.
    GviewRun { doc: usize, gen: u64 },
    GviewPlot { doc: usize, gen: u64 },
    /// `data()`: what it prints is the list of R2's datasets.
    DatasetList,
    /// One dataset's size, for that list.
    DatasetSize { name: String },
    /// The whole plot was written to a file; `rid` is the result that drew it.
    PlotFile { rid: Option<u64> },
}

/// What to do once a console command has succeeded.
pub(crate) enum After {
    /// A dataset was loaded from Sakha: show it there.
    OpenInSakha { df: String, load: String },
    /// A dataset was loaded from Pro: open it in the data viewer.
    ViewData { df: String },
    /// A variable's name or type changed: say so if R2 refused.
    ColumnChange,
    /// An R2 data file was loaded from Sakha: choose among what it held.
    ChooseInSakha { load: String },
}

pub(crate) struct Session {
    pub(crate) engine: Option<Engine>,
    /// R2's working folder.
    pub(crate) cwd: PathBuf,
    pub(crate) pending: HashMap<u64, Req>,
    /// R2 quit on the user's own `q(...)`: close the IDE too.
    pub(crate) exit_now: bool,
    pub(crate) version: String,
}

impl Session {
    pub(crate) fn new(cwd: PathBuf) -> Session {
        Session { engine: None, cwd, pending: HashMap::new(), exit_now: false, version: String::new() }
    }

    /// Is code the user asked for running (not a quiet query)?
    pub(crate) fn busy(&self) -> bool {
        self.pending.values().any(|r| matches!(r, Req::Console { .. } | Req::Analysis { .. }))
    }

    /// Is a console statement waiting for its reply?
    pub(crate) fn console_running(&self) -> bool {
        self.pending.values().any(|r| matches!(r, Req::Console { .. }))
    }

    /// Send a request, remembering what its reply is for.
    pub(crate) fn send(&mut self, op: &str, fields: Value, req: Req) {
        if let Some(e) = self.engine.as_mut() {
            let id = e.send(op, fields);
            self.pending.insert(id, req);
        }
    }

    pub(crate) fn value(&mut self, code: &str, req: Req) {
        self.send("value", json!({ "code": code }), req);
    }
}

impl Gui {
    /// Start R2 and greet the user with what is running.
    pub(crate) fn start_engine(&mut self) {
        let Some(exe) = find_r2() else {
            self.buffer.push_error(&format!(
                "No R2 engine found. Install Ardon-R2, or set {} to the path of r2.exe.",
                sakha_engine::R2_PATH_VAR));
            return;
        };
        match Engine::connect(&exe, &self.session.cwd) {
            Ok((engine, hello)) => {
                // R2's banner as R2 writes it, then Ardon Studio's one hint
                // line, as r2-console allows any frontend to add
                if hello.banner.is_empty() {
                    self.buffer.push_banner(&format!("Ardon-R2 {} ({})", hello.r2_version, exe.display()));
                } else {
                    for l in &hello.banner { self.buffer.push_banner(l); }
                }
                self.buffer.push_banner(&format!(
                    "Ardon Studio {}: Pro for scripts, Sakha for menus (top right). Ctrl+Enter runs a line; Esc stops.",
                    env!("CARGO_PKG_VERSION")));
                self.buffer.push_banner("");
                if let Some((main, more)) = &hello.prompts { self.buffer.set_prompts(main, more); }
                self.session.version = hello.r2_version;
                self.session.engine = Some(engine);
                self.refresh_session_panes();
            }
            Err(e) => self.buffer.push_error(&e),
        }
    }

    /// Ask R2 again for what the panes show about the session.
    pub(crate) fn refresh_session_panes(&mut self) {
        self.session.send("env", json!({}), Req::Env);
        self.session.value("getwd()", Req::Getwd);
    }

    /// A complete statement from the console prompt or a script.
    pub(crate) fn run_source(&mut self, src: String) {
        let after = self.next_after.take();
        match classify(&src) {
            Typed::Quit => self.quit_requested = true,
            Typed::Clear => self.buffer.clear(),
            Typed::View(expr) => self.open_data_tab(&expr),
            Typed::Code => {
                if self.session.engine.is_none() {
                    self.buffer.push_error("No R2 engine is running.");
                    return;
                }
                self.session.send("eval", json!({ "code": src }), Req::Console { src: src.clone(), after });
            }
        }
    }

    /// Esc while R2 computes: stop it and the lines still waiting, as in R.
    pub(crate) fn interrupt(&mut self) {
        self.run_queue.clear();
        if self.session.busy() {
            if let Some(e) = self.session.engine.as_mut() { e.interrupt(); }
        }
    }

    /// Take in everything R2 wrote since last frame.
    pub(crate) fn pump_engine(&mut self) {
        let Some(engine) = self.session.engine.as_ref() else { return };
        for msg in engine.poll() {
            match msg {
                Msg::Stream { id, text, err } => match self.session.pending.get(&id) {
                    Some(Req::Analysis { rid }) => { let rid = *rid; self.result_output(rid, &text, err); }
                    Some(Req::Console { .. }) => self.buffer.push_stream(&text, err),
                    Some(Req::DatasetList) => self.dataset_list_text(&text),
                    Some(Req::GviewRun { doc, .. }) if err => { let doc = *doc; self.gview_error_text(doc, &text); }
                    _ => {}
                },
                Msg::Reply { id, body } => {
                    if let Some(req) = self.session.pending.remove(&id) { self.on_reply(req, &body); }
                }
                Msg::Exited => { self.on_exit(); return; }
            }
        }
    }

    fn on_reply(&mut self, req: Req, body: &Value) {
        let ok = body["ok"] == Value::Bool(true);
        match req {
            Req::Console { src, after } => {
                if body["incomplete"] == Value::Bool(true) {
                    self.buffer.resume_continuation(&src);
                    return;
                }
                if let Some(svg) = body["plot"].as_str() { self.add_plot(svg.to_string()); }
                self.ask_whole_plot(None);
                self.data_may_have_changed();
                let failed = body["error"] == Value::Bool(true);
                match after {
                    Some(After::OpenInSakha { df, load }) if !failed => self.open_sakha(&df, Some(load)),
                    Some(After::OpenInSakha { df, .. }) =>
                        self.sakha.status = format!("Could not load {df}; the Console says why."),
                    Some(After::ViewData { df }) if !failed => self.open_data_tab(&df),
                    Some(After::ColumnChange) if failed =>
                        self.sakha.status = "R2 could not make that change; the Console says why.".into(),
                    Some(After::ColumnChange) => if let Some(d) = self.attr_dialog.as_mut() { d.applied(); },
                    Some(After::ChooseInSakha { load }) if !failed => {
                        self.sakha.pending_load = Some(load);
                        self.sakha.choosing = true;
                    }
                    Some(After::ChooseInSakha { .. }) =>
                        self.sakha.status = "Could not load that file; the Console says why.".into(),
                    _ => {}
                }
            }
            Req::Env => if ok { self.env_reply(body); },
            Req::Getwd => {
                if let Some(d) = body["value"]["values"][0].as_str() {
                    let d = PathBuf::from(d);
                    if self.files.dir.is_none() || self.session.cwd != d { self.files.dir = Some(d.clone()); }
                    self.session.cwd = d;
                }
            }
            Req::ViewPage { uid, page, gen } => self.view_reply(uid, page, gen, body),
            Req::SakhaCols { df, gen } => self.sakha_cols_reply(&df, gen, body),
            Req::SakhaCol { gen, col, slot } => self.sakha_col_reply(gen, col, slot, body),
            Req::Analysis { rid } => self.result_done(rid, body),
            Req::Statistic { rid } => self.result_statistic(rid, body),
            Req::Numbers { rid } => self.result_numbers(rid, body),
            Req::GviewRun { doc, gen } => self.gview_reply(doc, gen, body),
            Req::GviewPlot { doc, gen } => self.gview_plot(doc, gen),
            Req::PlotFile { rid } => self.whole_plot_reply(rid),
            Req::DatasetList => {}
            Req::DatasetSize { name } => self.dataset_size_reply(&name, body),
        }
    }

    /// R2 is gone. After the user's own `q(...)` the IDE closes too;
    /// otherwise the session starts again (its variables are lost).
    fn on_exit(&mut self) {
        let quit_by_user = self.session.pending.values().any(|r| match r {
            Req::Console { src, .. } => { let s = src.trim_start(); s.starts_with("q(") || s.starts_with("quit(") }
            _ => false,
        });
        self.session.engine = None;
        self.session.pending.clear();
        if quit_by_user {
            self.session.exit_now = true;
            return;
        }
        self.buffer.push_error("The R2 engine stopped. Starting a new session; earlier variables are gone.");
        self.sakha.engine_lost();
        self.start_engine();
    }

    /// After a command the user ran: variables and data may be different.
    pub(crate) fn data_may_have_changed(&mut self) {
        self.refresh_session_panes();
        for v in self.views.values_mut() { v.invalidate(); }
        self.sakha.stale = true;
    }
}
