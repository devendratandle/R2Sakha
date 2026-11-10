//! Saving and resuming a Sakha analysis: the `.sakha` file (`sakha_core::document`).
//! File ▸ Save, Ctrl+S and the header's Save button write it; opening it puts
//! the user back where they were, with the results as R2 last gave them. R2's
//! memory is rebuilt by running the load and the data-changing steps again.
//! A backup is written while there are unsaved changes, so a crash loses little.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sakha_core::analysis;
use sakha_core::document::{data_stamp, quoted_path, r_path, resolved_path, stored_path, Document, SavedResult, SAKHA_EXT};
use sakha_engine::json;

use crate::sakha_view::{Face, SakhaResult};
use crate::session::Req;
use crate::Gui;

/// How often unsaved changes are written to the backup.
const BACKUP_EVERY: Duration = Duration::from_secs(30);

/// Where Ardon Studio keeps its own files: `ARDON_STUDIO_CONFIG`, else the user's
/// application-data folder.
pub(crate) fn config_dir() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("ARDON_STUDIO_CONFIG") { return Some(PathBuf::from(d)); }
    std::env::var_os("APPDATA").or_else(|| std::env::var_os("HOME")).map(|d| PathBuf::from(d).join("Ardon Studio"))
}

pub(crate) fn backup_file() -> Option<PathBuf> { config_dir().map(|d| d.join("unsaved.sakha")) }

impl Gui {
    /// The analysis as it stands, for a file in `dir`.
    fn to_document(&self, dir: &Path) -> Option<Document> {
        let s = &self.sakha;
        let df = s.df.clone()?;
        let data = s.load.as_deref().and_then(quoted_path).map(|p| resolved_path(&self.session.cwd, &p));
        let (header, steps) = s.script.parts();
        Some(Document {
            df,
            load: s.load.clone(),
            data_file: data.as_deref().map(|p| stored_path(dir, p)),
            data_stamp: data.as_deref().and_then(data_stamp),
            header: header.to_vec(),
            steps: steps.to_vec(),
            results: s.results.iter().map(|r| SavedResult {
                analysis: r.a.id.to_string(), title: r.title.clone(), code: r.code.clone(), statistic: r.statistic.clone(),
                says: r.says.clone(), output: r.output.clone(), svg: r.svg.clone(),
            }).collect(),
            face: match s.face { Face::Overview => "overview", Face::Data => "data", Face::Results => "results" }.into(),
            current: s.current,
            selected: s.selected.and_then(|i| s.cols.get(i)).map(|c| c.name.clone()),
        })
    }

    /// File ▸ Save (or Save as…, or a first save): ask where when needed.
    pub(crate) fn save_document(&mut self, ask: bool) {
        if self.sakha.df.is_none() {
            self.sakha.status = "Open a dataset first: there is nothing to save yet.".into();
            return;
        }
        let path = match self.sakha.document.clone().filter(|_| !ask) {
            Some(p) => p,
            None => {
                let name = self.sakha.document.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| format!("{}.{SAKHA_EXT}", self.sakha.df.clone().unwrap_or_default()));
                let Some(mut p) = rfd::FileDialog::new().set_title("Save Sakha analysis").set_directory(&self.session.cwd)
                    .set_file_name(name).add_filter("Sakha analysis", &[SAKHA_EXT]).save_file() else { return };
                if p.extension().is_none() { p.set_extension(SAKHA_EXT); }
                p
            }
        };
        self.save_document_to(&path);
    }

    pub(crate) fn save_document_to(&mut self, path: &Path) {
        let dir = path.parent().unwrap_or(Path::new("."));
        let Some(doc) = self.to_document(dir) else { return };
        let shown = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match std::fs::write(path, doc.to_text()) {
            Ok(()) => {
                self.sakha.document = Some(path.to_path_buf());
                self.sakha.dirty = false;
                self.sakha.status = format!("Saved {shown}");
                self.recent.add_analysis(path);
                self.save_recent();
                self.remove_backup();
            }
            Err(e) => self.sakha.status = format!("Could not save {shown}: {e}"),
        }
    }

    pub(crate) fn open_document(&mut self, path: &Path) {
        let shown = path.display().to_string();
        let doc = std::fs::read_to_string(path).map_err(|e| e.to_string()).and_then(|t| Document::from_text(&t));
        match doc {
            Ok(doc) => {
                // with no project, R2 works beside the file, so its script's
                // relative paths work when edited in Pro
                if let (None, Some(dir)) = (&self.project, path.parent()) {
                    self.session.value(&format!("setwd(\"{}\")", r_path(dir)), Req::Getwd);
                    self.session.cwd = dir.to_path_buf();
                    self.session.value("getwd()", Req::Getwd);
                }
                self.resume(doc, path);
                self.sakha.document = Some(path.to_path_buf());
                self.sakha.dirty = false;
                self.recent.add_analysis(path);
                self.save_recent();
            }
            Err(e) => {
                self.set_mode(true);
                self.sakha.status = format!("Could not open {shown}: {e}");
            }
        }
    }

    /// The analysis left unsaved when the app last stopped.
    pub(crate) fn recover_document(&mut self) {
        let Some(path) = self.recover.take() else { return };
        if let Ok(doc) = std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| Document::from_text(&t)) {
            self.resume(doc, &path);
            self.sakha.document = None;
            self.sakha.dirty = true;
            self.sakha.status = "Recovered: use Save to keep it.".into();
        }
    }

    /// Put Sakha back as the file left it, and rebuild the data in R2.
    fn resume(&mut self, doc: Document, path: &Path) {
        self.set_mode(true);
        let dir = path.parent().unwrap_or(Path::new("."));
        let data = doc.data_file.as_deref().map(|f| resolved_path(dir, f));
        // no file at all: R2's own dataset, whose load line needs none
        let found = match &data { Some(p) => p.is_file(), None => doc.load.is_some() };
        let changed = found && doc.data_stamp.is_some() && data.as_deref().and_then(data_stamp) != doc.data_stamp;
        let in_session = self.env.rows.iter().any(|r| r.name == doc.df && r.is_data_frame());
        let lines = if found { doc.rebuild_lines(data.as_deref()) } else { vec![] };

        if let Some(old) = self.sakha.data_uid.take() { self.views.remove(&old); }
        let uid = self.new_view(&doc.df);
        let s = &mut self.sakha;
        s.script = doc.script();
        s.load = lines.first().filter(|_| doc.load.is_some()).cloned().or_else(|| doc.load.clone());
        s.df = Some(doc.df.clone());
        s.cols.clear();
        s.selected = None;
        s.reselect = doc.selected.clone();
        s.scroll = 0;
        s.results = doc.results.iter().filter_map(|r| analysis::find(&r.analysis).map(|a| SakhaResult {
            title: r.title.clone(), a, code: r.code.clone(), statistic: r.statistic.clone(), says: r.says.clone(),
            output: r.output.clone(), svg: r.svg.clone(), running: false,
        })).collect();
        s.current = doc.current.min(s.results.len().saturating_sub(1));
        s.panel_for = None;
        s.result_scroll = 0;
        s.face = match doc.face.as_str() { "data" => Face::Data, "results" => Face::Results, _ => Face::Overview };
        s.data_uid = Some(uid);
        s.choosing = false;
        s.data_changed = changed;
        s.dialog = None;
        s.status = if changed { "The data file has changed since this was saved. Results shown are from the saved run.".into() }
                   else if !found && !in_session {
                       match &doc.data_file {
                           Some(f) => format!("Data file not found: {f}. The saved results are still here."),
                           None => format!("'{}' was made in the console, so it is not in this session. The saved results are still here.", doc.df),
                       }
                   } else { String::new() };
        s.stale = false;

        if lines.is_empty() {
            if in_session { self.sakha.stale = true; }
            return;
        }
        // through the console, like any load; the first reply re-reads the columns
        self.buffer.cancel_continuation();
        for line in lines { self.submit(line); }
    }

    /// Run every result again, on the data as it is now.
    pub(crate) fn rerun_results(&mut self) {
        for rid in 0..self.sakha.results.len() {
            let r = &mut self.sakha.results[rid];
            r.output.clear();
            r.svg = None;
            r.running = true;
            r.says = "R2 is working…".into();
            let code = r.code.join("\n");
            self.session.send("eval", json!({ "code": code }), Req::Analysis { rid });
        }
        self.sakha.data_changed = false;
        self.sakha.status.clear();
        self.sakha.dirty = true;
        self.sakha.panel_for = None;
    }

    /// Keep a backup of unsaved work, every so often.
    pub(crate) fn backup_tick(&mut self) {
        if !self.sakha.dirty { return; }
        if self.last_backup.is_some_and(|t| t.elapsed() < BACKUP_EVERY) { return; }
        self.last_backup = Some(Instant::now());
        let Some(file) = backup_file() else { return };
        // relative to its own folder, as every `.sakha` is: so the data stays found
        let Some(parent) = file.parent().map(Path::to_path_buf) else { return };
        if let Some(doc) = self.to_document(&parent) {
            let parent = &parent;
            let _ = std::fs::create_dir_all(parent);
            let _ = std::fs::write(&file, doc.to_text());
        }
    }

    pub(crate) fn remove_backup(&mut self) {
        if let Some(f) = backup_file() { let _ = std::fs::remove_file(f); }
    }
}
