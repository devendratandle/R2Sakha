//! Project folders. A folder with a `.r2proj` file is a project: opening it
//! starts a fresh R2 session with the folder as its working folder, in both
//! faces, so `read.csv("trial.csv")` just works and the whole folder can be
//! moved or zipped. The button at the right of the menu bar holds the commands.
//! With no project, R2 works in the user's Documents.

use std::path::{Path, PathBuf};

use r2_ui::{ContextItem, ContextMenu, Rect};
use sakha_core::document::{project_file_in, resolved_path, stored_path, Project, Recent};

use crate::sakha_doc::config_dir;
use crate::source_view::Tab;
use crate::Gui;

/// The project open now.
pub(crate) struct OpenProject {
    pub(crate) file: PathBuf,
    pub(crate) folder: PathBuf,
    pub(crate) name: String,
}

fn recent_file() -> Option<PathBuf> { config_dir().map(|d| d.join("recent.json")) }

pub(crate) fn load_recent() -> Recent {
    recent_file().and_then(|f| std::fs::read_to_string(f).ok()).map(|t| Recent::from_text(&t)).unwrap_or_default()
}

impl Gui {
    pub(crate) fn save_recent(&self) {
        let Some(f) = recent_file() else { return };
        if let Some(d) = f.parent() { let _ = std::fs::create_dir_all(d); }
        let _ = std::fs::write(f, self.recent.to_text());
    }

    /// The button's label.
    pub(crate) fn project_label(&self) -> String {
        match &self.project {
            Some(p) => format!("{} ▾", p.name),
            None => "No project ▾".into(),
        }
    }

    /// Open the button's popup under it, with the commands and recent projects.
    pub(crate) fn open_project_menu(&mut self, button: Rect) {
        let mut items = vec![ContextItem::new("New project…", "project.new"),
                             ContextItem::new("Open project…", "project.open")];
        if self.project.is_some() { items.push(ContextItem::new("Close project", "project.close")); }
        let current = self.project.as_ref().map(|p| p.file.to_string_lossy().into_owned());
        let recents: Vec<ContextItem> = self.recent.projects.iter()
            .filter(|p| Some(*p) != current.as_ref() && Path::new(p).is_file())
            .map(|p| {
                let name = Path::new(p).file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                ContextItem::new(name, format!("project.open:{p}"))
            }).collect();
        if !recents.is_empty() { items.push(ContextItem::separator()); items.extend(recents); }
        let width = items.iter().map(|i| i.label.chars().count()).max().unwrap_or(0) as f32 * 9.0 + 24.0;
        self.project_menu = ContextMenu::new(items);
        self.project_menu.open_at((button.x + button.w - width.max(160.0)).max(0.0), button.y + button.h);
    }

    pub(crate) fn project_action(&mut self, action: &str) {
        if let Some(p) = action.strip_prefix("project.open:") { self.open_project(Path::new(p)); return; }
        if let Some(p) = action.strip_prefix("project.new:") { self.new_project(Path::new(p)); return; }
        if action == "project.close" {
            self.save_project();
            self.project = None;
            let home = crate::support::pick_user_home().unwrap_or_else(|| self.session.cwd.clone());
            self.fresh_session(&home);
            self.add_script(r2_editor::Editor::new());
        }
    }

    /// A folder becomes a project; one that already is simply opens.
    pub(crate) fn new_project(&mut self, folder: &Path) {
        let (file, exists) = project_file_in(folder);
        if !exists {
            if let Err(e) = std::fs::write(&file, Project::default().to_text()) {
                self.buffer.push_error(&format!("Could not create {}: {e}", file.display()));
                return;
            }
        }
        self.open_project(&file);
    }

    pub(crate) fn open_project(&mut self, file: &Path) {
        let parsed = std::fs::read_to_string(file).map_err(|e| e.to_string()).and_then(|t| Project::from_text(&t));
        let project = match parsed {
            Ok(p) => p,
            Err(e) => {
                self.buffer.push_error(&format!("Could not open {}: {e}", file.display()));
                return;
            }
        };
        self.save_project();
        let folder = file.parent().unwrap_or(Path::new(".")).to_path_buf();
        let name = file.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.project = Some(OpenProject { file: file.to_path_buf(), folder: folder.clone(), name });
        self.fresh_session(&folder);

        self.studio.split_x = project.split_x;
        self.studio.split_left = project.split_left;
        self.studio.split_right = project.split_right;
        for s in &project.scripts {
            let path = resolved_path(&folder, s);
            if path.is_file() { self.open_script_file(&path); }
            else { self.buffer.push_error(&format!("Script not found: {s}")); }
        }
        if self.source.tabs.is_empty() { self.add_script(r2_editor::Editor::new()); }
        self.source.active = project.active.min(self.source.tabs.len() - 1);
        if let Some(a) = &project.analysis {
            let path = resolved_path(&folder, a);
            if path.is_file() { self.open_document(&path); }
        }
        self.set_mode(project.sakha);
        self.recent.add_project(file);
        self.save_recent();
    }

    /// Everything from the last session goes; R2 starts again in `folder`.
    fn fresh_session(&mut self, folder: &Path) {
        self.session.engine = None;
        self.session.pending.clear();
        self.session.cwd = folder.to_path_buf();
        self.files.dir = Some(folder.to_path_buf());
        self.source = Default::default();
        self.views.clear();
        self.env = Default::default();
        self.plots = Default::default();
        self.sakha = Default::default();
        self.next_after = None;
        self.run_queue.clear();
        self.buffer = crate::console::ConsoleBuffer::new();
        if let Some(p) = &self.project {
            self.buffer.push_banner(&format!("Project {}: working folder {}", p.name, p.folder.display()));
        }
        self.start_engine();
    }

    /// Write the open project's file: its scripts, layout and analysis.
    pub(crate) fn save_project(&mut self) {
        let Some(p) = &self.project else { return };
        let (mut scripts, mut active) = (Vec::new(), 0);
        for (i, t) in self.source.tabs.iter().enumerate() {
            if let Tab::Script(s) = t {
                if let Some(path) = &s.ed.path {
                    if i == self.source.active { active = scripts.len(); }
                    scripts.push(stored_path(&p.folder, path));
                }
            }
        }
        let text = Project {
            scripts, active,
            split_x: self.studio.split_x, split_left: self.studio.split_left, split_right: self.studio.split_right,
            sakha: self.sakha_mode,
            analysis: self.sakha.document.as_ref().map(|d| stored_path(&p.folder, d)),
        }.to_text();
        if let Err(e) = std::fs::write(&p.file, text) {
            self.buffer.push_error(&format!("Could not save the project: {e}"));
        }
    }
}
