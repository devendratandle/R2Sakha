//! The two files R2Sakha saves, both small, readable JSON that works in Git.
//!
//! * [`Document`] is a `.sakha` file: one menu-driven analysis, like a
//!   jamovi file. It can sit anywhere, alone or in a project folder. It holds
//!   a link to the data, not a copy; the steps as R2 code; each result as R2
//!   printed and drew it, so it opens at once and its numbers stay as reported;
//!   and where the user was.
//! * [`Project`] is a `.r2proj` file: it marks its folder as a project, as
//!   `.Rproj` does. The folder is R2's working folder for the session. The
//!   file holds the Studio's open scripts and layout, relative to the folder.
//!
//! Neither holds R2's memory: reopening reruns the load and the steps that
//! change the data. [`Recent`] is the list of files lately opened.

use std::path::{Component, Path, PathBuf};

use sakha_engine::{json, Value};

use crate::script::SessionScript;

pub const SAKHA_EXT: &str = "sakha";
pub const PROJECT_EXT: &str = "r2proj";
pub const PARQUET_EXT: &str = "parquet";

const SAKHA_FORMAT: &str = "r2sakha.analysis";
const PROJECT_FORMAT: &str = "r2sakha.project";
const VERSION: u64 = 1;
/// Most entries kept in each recent list.
const RECENT_MAX: usize = 8;

fn string(v: &Value) -> String { v.as_str().unwrap_or("").to_string() }
fn strings(v: &Value) -> Vec<String> { v.as_array().map(|a| a.iter().map(string).collect()).unwrap_or_default() }
fn opt_string(v: &Value) -> Option<String> { v.as_str().map(str::to_string) }

fn parse(text: &str, format: &str, what: &str) -> Result<Value, String> {
    let v: Value = sakha_engine::parse_json(text).map_err(|e| format!("this is not a {what} file: {e}"))?;
    if v["format"].as_str() != Some(format) { return Err(format!("this is not a {what} file")); }
    if v["version"].as_u64().unwrap_or(0) > VERSION {
        return Err(format!("this {what} file was saved by a newer R2Sakha"));
    }
    Ok(v)
}

/// One analysis's card, as R2 left it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SavedResult {
    /// The analysis's id on the menu.
    pub analysis: String,
    pub title: String,
    pub code: Vec<String>,
    pub statistic: Option<String>,
    pub says: String,
    pub output: String,
    pub svg: Option<String>,
}

/// A `.sakha` file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Document {
    /// The data frame's name in the session.
    pub df: String,
    /// The line that loaded it, as first run.
    pub load: Option<String>,
    /// The data file, relative to the `.sakha` when inside its folder.
    pub data_file: Option<String>,
    /// Size and time of the data file when saved: tells if it has changed.
    pub data_stamp: Option<(u64, u64)>,
    pub header: Vec<String>,
    pub steps: Vec<(String, String, String)>,
    pub results: Vec<SavedResult>,
    /// "overview", "data" or "results".
    pub face: String,
    pub current: usize,
    pub selected: Option<String>,
}

impl Document {
    pub fn to_text(&self) -> String {
        let results: Vec<Value> = self.results.iter().map(|r| json!({
            "analysis": r.analysis, "title": r.title, "code": r.code, "statistic": r.statistic,
            "says": r.says, "output": r.output, "svg": r.svg,
        })).collect();
        let steps: Vec<Value> = self.steps.iter().map(|(k, c, code)| json!({ "key": k, "comment": c, "code": code })).collect();
        sakha_engine::pretty_json(&json!({
            "format": SAKHA_FORMAT, "version": VERSION,
            "dataset": { "name": self.df, "load": self.load, "file": self.data_file,
                         "size": self.data_stamp.map(|s| s.0), "modified": self.data_stamp.map(|s| s.1) },
            "script": { "header": self.header, "steps": steps },
            "results": results,
            "view": { "face": self.face, "current": self.current, "selected": self.selected },
        }))
    }

    pub fn from_text(text: &str) -> Result<Document, String> {
        let v = parse(text, SAKHA_FORMAT, "Sakha")?;
        let d = &v["dataset"];
        let stamp = match (d["size"].as_u64(), d["modified"].as_u64()) { (Some(a), Some(b)) => Some((a, b)), _ => None };
        Ok(Document {
            df: string(&d["name"]),
            load: opt_string(&d["load"]),
            data_file: opt_string(&d["file"]),
            data_stamp: stamp,
            header: strings(&v["script"]["header"]),
            steps: v["script"]["steps"].as_array().map(|a| a.iter()
                .map(|s| (string(&s["key"]), string(&s["comment"]), string(&s["code"]))).collect()).unwrap_or_default(),
            results: v["results"].as_array().map(|a| a.iter().map(|r| SavedResult {
                analysis: string(&r["analysis"]), title: string(&r["title"]), code: strings(&r["code"]),
                statistic: opt_string(&r["statistic"]), says: string(&r["says"]), output: string(&r["output"]),
                svg: opt_string(&r["svg"]),
            }).collect()).unwrap_or_default(),
            face: string(&v["view"]["face"]),
            current: v["view"]["current"].as_u64().unwrap_or(0) as usize,
            selected: opt_string(&v["view"]["selected"]),
        })
    }

    pub fn script(&self) -> SessionScript { SessionScript::from_parts(self.header.clone(), self.steps.clone()) }

    /// The lines that rebuild the data frame on reopening: the load, pointed
    /// at where the file is now, then the steps that changed the data.
    pub fn rebuild_lines(&self, data: Option<&Path>) -> Vec<String> {
        let load = match (&self.load, data) {
            (Some(l), Some(p)) => Some(with_quoted_path(l, p)),
            (Some(l), None) => Some(l.clone()),
            _ => None,
        };
        load.into_iter().chain(self.script().data_steps()).collect()
    }
}

/// The file a load line reads: its first quoted string.
pub fn quoted_path(line: &str) -> Option<String> {
    let start = line.find('"')? + 1;
    let end = line[start..].find('"')? + start;
    Some(line[start..end].to_string())
}

fn with_quoted_path(line: &str, path: &Path) -> String {
    let Some(old) = quoted_path(line) else { return line.to_string() };
    line.replacen(&format!("\"{old}\""), &format!("\"{}\"", r_path(path)), 1)
}

/// A path as R2 code wants it: forward slashes, quotes escaped.
pub fn r_path(p: &Path) -> String { p.to_string_lossy().replace('\\', "/").replace('"', "\\\"") }

/// The size and modified time that tell if a data file has changed.
pub fn data_stamp(p: &Path) -> Option<(u64, u64)> {
    let m = std::fs::metadata(p).ok()?;
    let t = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    Some((m.len(), t))
}

/// `target` relative to `base` when it is inside it, with forward slashes;
/// otherwise the whole path. So a moved folder keeps working.
pub fn stored_path(base: &Path, target: &Path) -> String {
    let inside = target.strip_prefix(base).ok()
        .filter(|r| r.components().all(|c| matches!(c, Component::Normal(_))));
    inside.unwrap_or(target).to_string_lossy().replace('\\', "/")
}

/// A stored path as a real one.
pub fn resolved_path(base: &Path, stored: &str) -> PathBuf {
    let p = Path::new(stored);
    if p.is_absolute() { p.to_path_buf() } else { base.join(p) }
}

/// A `.r2proj` file.
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    /// Open scripts, relative to the folder.
    pub scripts: Vec<String>,
    pub active: usize,
    pub split_x: f32,
    pub split_left: f32,
    pub split_right: f32,
    /// Sakha was in front.
    pub sakha: bool,
    /// The `.sakha` open in Sakha, relative to the folder.
    pub analysis: Option<String>,
}

impl Default for Project {
    fn default() -> Self {
        let s = crate::layout::Studio::default();
        Project { scripts: vec![], active: 0, split_x: s.split_x, split_left: s.split_left, split_right: s.split_right,
                  sakha: false, analysis: None }
    }
}

impl Project {
    pub fn to_text(&self) -> String {
        sakha_engine::pretty_json(&json!({
            "format": PROJECT_FORMAT, "version": VERSION,
            "scripts": self.scripts, "active": self.active,
            "layout": { "split_x": self.split_x, "split_left": self.split_left, "split_right": self.split_right },
            "face": if self.sakha { "sakha" } else { "studio" },
            "analysis": self.analysis,
        }))
    }

    pub fn from_text(text: &str) -> Result<Project, String> {
        let v = parse(text, PROJECT_FORMAT, "project")?;
        let d = Project::default();
        let split = |k: &str, or: f32| v["layout"][k].as_f64().map_or(or, |x| x as f32).clamp(0.1, 0.9);
        Ok(Project {
            scripts: strings(&v["scripts"]),
            active: v["active"].as_u64().unwrap_or(0) as usize,
            split_x: split("split_x", d.split_x),
            split_left: split("split_left", d.split_left),
            split_right: split("split_right", d.split_right),
            sakha: v["face"].as_str() == Some("sakha"),
            analysis: opt_string(&v["analysis"]),
        })
    }
}

/// The project file of a folder: the one already there, else `<folder>.r2proj`.
pub fn project_file_in(folder: &Path) -> (PathBuf, bool) {
    let existing = std::fs::read_dir(folder).ok().and_then(|d| d.flatten().map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == PROJECT_EXT)));
    match existing {
        Some(p) => (p, true),
        None => {
            let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "project".into());
            (folder.join(format!("{name}.{PROJECT_EXT}")), false)
        }
    }
}

/// The files lately opened, newest first.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Recent {
    pub projects: Vec<String>,
    pub analyses: Vec<String>,
}

fn push_front(list: &mut Vec<String>, item: &Path) {
    let item = item.to_string_lossy().into_owned();
    list.retain(|x| *x != item);
    list.insert(0, item);
    list.truncate(RECENT_MAX);
}

impl Recent {
    pub fn add_project(&mut self, p: &Path) { push_front(&mut self.projects, p); }
    pub fn add_analysis(&mut self, p: &Path) { push_front(&mut self.analyses, p); }

    pub fn from_text(text: &str) -> Recent {
        let v = sakha_engine::parse_json(text).unwrap_or(Value::Null);
        Recent { projects: strings(&v["projects"]), analyses: strings(&v["analyses"]) }
    }

    pub fn to_text(&self) -> String {
        sakha_engine::pretty_json(&json!({ "projects": self.projects, "analyses": self.analyses }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Document {
        Document {
            df: "trial".into(),
            load: Some("trial <- read.csv(\"C:/old/trial.csv\")".into()),
            data_file: Some("data/trial.csv".into()),
            data_stamp: Some((120, 1_700_000_000)),
            header: vec!["# Sakha session".into()],
            steps: vec![("cat:dose".into(), "dose as a category".into(), "trial$dose <- factor(trial$dose)".into()),
                        ("run:x".into(), "Histogram".into(), "hist(trial$score)".into())],
            results: vec![SavedResult { analysis: "hist".into(), title: "Histogram of score".into(),
                                        code: vec!["hist(trial$score)".into()], statistic: None, says: "ok \"quoted\"".into(),
                                        output: "line 1\nline 2\n".into(), svg: Some("<svg/>".into()) }],
            face: "results".into(),
            current: 0,
            selected: Some("score".into()),
        }
    }

    #[test]
    fn a_sakha_file_keeps_everything() {
        let d = sample();
        let text = d.to_text();
        assert!(text.contains("\n"), "readable, not one long line");
        assert_eq!(Document::from_text(&text).unwrap(), d);
    }

    #[test]
    fn reopening_rebuilds_the_data_from_where_the_file_is_now() {
        let lines = sample().rebuild_lines(Some(Path::new("E:\\new place\\trial.csv")));
        assert_eq!(lines, vec!["trial <- read.csv(\"E:/new place/trial.csv\")", "trial$dose <- factor(trial$dose)"],
                   "the load and the data-changing step, not the histogram");
        assert_eq!(quoted_path("x <- read.csv(\"a b.csv\")").as_deref(), Some("a b.csv"));
        let console_made = Document::default();
        assert!(console_made.rebuild_lines(None).is_empty());
    }

    #[test]
    fn paths_stay_relative_inside_the_folder() {
        let base = Path::new("E:/p/Trial");
        assert_eq!(stored_path(base, Path::new("E:/p/Trial/data/a.csv")), "data/a.csv");
        assert_eq!(stored_path(base, Path::new("E:/other/a.csv")), "E:/other/a.csv");
        assert_eq!(resolved_path(base, "data/a.csv"), Path::new("E:/p/Trial").join("data/a.csv"));
    }

    #[test]
    fn a_project_file_keeps_its_scripts_and_layout() {
        let p = Project { scripts: vec!["a.R".into(), "scripts/b.R".into()], active: 1, split_x: 0.4, split_left: 0.6,
                          split_right: 0.3, sakha: true, analysis: Some("base.sakha".into()) };
        assert_eq!(Project::from_text(&p.to_text()).unwrap(), p);
    }

    #[test]
    fn the_wrong_file_is_refused_with_a_reason() {
        assert!(Document::from_text("{not json").unwrap_err().contains("not a Sakha"));
        assert!(Document::from_text(&Project::default().to_text()).unwrap_err().contains("not a Sakha"));
        assert!(Project::from_text(&sample().to_text()).unwrap_err().contains("not a project"));
        let newer = sample().to_text().replace("\"version\": 1", "\"version\": 99");
        assert!(Document::from_text(&newer).unwrap_err().contains("newer"));
    }

    #[test]
    fn recent_lists_put_the_latest_first_without_repeats() {
        let mut r = Recent::default();
        for i in 0..12 { r.add_project(Path::new(&format!("p{i}.r2proj"))); }
        r.add_project(Path::new("p5.r2proj"));
        assert_eq!(r.projects.len(), RECENT_MAX);
        assert_eq!(r.projects[0], "p5.r2proj");
        assert_eq!(r.projects.iter().filter(|x| *x == "p5.r2proj").count(), 1);
        r.add_analysis(Path::new("a.sakha"));
        assert_eq!(Recent::from_text(&r.to_text()), r);
        assert_eq!(Recent::from_text("garbage"), Recent::default());
    }
}
