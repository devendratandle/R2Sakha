//! The Studio's bottom-right pane: **Plots**, with the session's plot history
//! to step through, and **Files**, the working folder to browse. R2 sends each
//! changed plot as SVG; this pane keeps them to show, save and copy.

use std::path::PathBuf;

use r2_ui::{Color, Frame, GraphPanel, InputEvent, MouseButton, Rect, Renderer, Theme};

use crate::session::Req;
use crate::ui::{button_left_of, inside, mix, tabs, text};
use crate::Gui;

/// Most plots kept in the history.
const KEEP: usize = 50;

pub(crate) struct PlotsPane {
    pub(crate) history: Vec<String>,
    /// Which plot shows.
    pub(crate) at: usize,
    panel: GraphPanel,
    tab: usize,
    tab_rects: Vec<Rect>,
    prev: Option<Rect>,
    next: Option<Rect>,
    save: Option<Rect>,
}

impl Default for PlotsPane {
    fn default() -> Self {
        PlotsPane { history: vec![], at: 0, panel: GraphPanel::new(), tab: 0, tab_rects: vec![], prev: None, next: None, save: None }
    }
}

impl PlotsPane {
    pub(crate) fn current(&self) -> Option<&str> { self.history.get(self.at).map(String::as_str) }

    fn step(&mut self, by: isize) {
        let to = self.at as isize + by;
        if to < 0 || to as usize >= self.history.len() { return; }
        self.at = to as usize;
        let svg = self.history[self.at].clone().into_bytes();
        self.panel.set_svg(svg);
    }
}

/// The Files tab: the folder shown, and its entries.
#[derive(Default)]
pub(crate) struct FilesTab {
    pub(crate) dir: Option<PathBuf>,
    listed: Option<PathBuf>,
    entries: Vec<(String, bool)>,
    scroll: usize,
}

impl FilesTab {
    /// Read the folder when it changed since the last listing.
    fn refresh(&mut self) {
        if self.dir == self.listed { return; }
        self.listed = self.dir.clone();
        self.scroll = 0;
        self.entries.clear();
        let Some(d) = &self.dir else { return };
        let Ok(read) = std::fs::read_dir(d) else { return };
        let mut items: Vec<(String, bool)> = read.flatten()
            .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path().is_dir()))
            .filter(|(n, _)| !n.starts_with('.')).collect();
        items.sort_by_key(|(n, dir)| (!*dir, n.to_lowercase()));
        if d.parent().is_some() { self.entries.push(("..".into(), true)); }
        self.entries.extend(items);
    }
}

/// Where R2 writes the whole figure; private to this run of the app.
fn whole_plot_file() -> PathBuf {
    std::env::temp_dir().join(format!("r2sakha-{}-plot.svg", std::process::id()))
}

impl Gui {
    /// A plot from R2: into the history, and on screen.
    pub(crate) fn add_plot(&mut self, svg: String) {
        let p = &mut self.plots;
        if p.history.len() == KEEP { p.history.remove(0); }
        p.history.push(svg.clone());
        p.at = p.history.len() - 1;
        p.panel.set_svg(svg.into_bytes());
        p.tab = 0;
        if !self.sakha_mode { self.show_window(self.plots_id); }
    }

    /// `r2 --ide` sends a plot as `plot()` first drew it: `lines()`, `abline()`,
    /// `points()` and `legend()` never reach its replies. `save.plot()` writes the whole
    /// figure, so after each run R2 is asked for it. Remove this when `--ide` sends it.
    pub(crate) fn ask_whole_plot(&mut self, rid: Option<usize>) {
        if self.plots.history.is_empty() { return; }
        let code = format!("save.plot(\"{}\")", sakha_core::document::r_path(&whole_plot_file()));
        self.session.value(&code, Req::PlotFile { rid });
    }

    /// Additions only ever lengthen a figure; a shorter file is not one (after
    /// `dev.off()`, say).
    pub(crate) fn whole_plot_reply(&mut self, rid: Option<usize>) {
        let file = whole_plot_file();
        let read = std::fs::read_to_string(&file);
        let _ = std::fs::remove_file(&file);
        let (Ok(svg), Some(last)) = (read, self.plots.history.last_mut()) else { return };
        if svg.len() <= last.len() {
            if let Some(rid) = rid { let same = last.clone(); self.result_plot(rid, same); }
            return;
        }
        *last = svg.clone();
        self.plots.at = self.plots.history.len() - 1;
        self.plots.panel.set_svg(svg.clone().into_bytes());
        if let Some(rid) = rid { self.result_plot(rid, svg); }
    }

    pub(crate) fn plots_input(&mut self, events: &[InputEvent], renderer: &mut Renderer, theme: &Theme) -> Option<&'static str> {
        let c = self.mdi.window(self.plots_id).filter(|w| w.visible).map(|w| w.content_rect(theme))?;
        let (_, line_h) = renderer.cell_metrics(theme.fs());
        let body_y = c.y + line_h + 10.0;
        let mut action = None;
        for ev in events {
            match *ev {
                InputEvent::MouseDown { button: MouseButton::Left, pos } if inside(c, pos.x, pos.y) => {
                    let hit = |r: &Option<Rect>| r.is_some_and(|r| inside(r, pos.x, pos.y));
                    if let Some(i) = self.plots.tab_rects.iter().position(|r| inside(*r, pos.x, pos.y)) {
                        self.plots.tab = i;
                    } else if hit(&self.plots.prev) {
                        self.plots.step(-1);
                    } else if hit(&self.plots.next) {
                        self.plots.step(1);
                    } else if hit(&self.plots.save) {
                        action = Some("file.save_plot");
                    } else if self.plots.tab == 1 && pos.y >= body_y {
                        let row = ((pos.y - body_y) / line_h).floor() as usize + self.files.scroll;
                        self.file_clicked(row);
                    }
                }
                InputEvent::Scroll { dy } if self.plots.tab == 1 && inside(c, self.mouse.0, self.mouse.1) => {
                    self.files.scroll = if dy > 0.0 { self.files.scroll.saturating_sub(3) }
                                        else { (self.files.scroll + 3).min(self.files.entries.len().saturating_sub(1)) };
                }
                _ => {}
            }
        }
        action
    }

    /// A folder opens; a script goes to the Source pane; a CSV is loaded.
    fn file_clicked(&mut self, row: usize) {
        let Some((name, is_dir)) = self.files.entries.get(row).cloned() else { return };
        let Some(dir) = self.files.dir.clone() else { return };
        if is_dir {
            self.files.dir = if name == ".." { dir.parent().map(PathBuf::from) } else { Some(dir.join(name)) };
            return;
        }
        let path = dir.join(&name);
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        match ext.as_str() {
            "csv" => self.queued = Some(format!("data.open:{}", path.display())),
            "r" | "r2" | "txt" | "md" => self.open_script_file(&path),
            _ => self.buffer.push_output(&format!("{name}: R2Sakha opens .R, .R2, .txt, .md and .csv files.")),
        }
    }

    pub(crate) fn paint_plots(&mut self, c: Rect, frame: &mut Frame, renderer: &mut Renderer, theme: &Theme) {
        let (cell_w, line_h) = renderer.cell_metrics(theme.fs());
        let (bg, fg, size) = (theme.window_background, theme.console_output, theme.fs());
        frame.paint_rect(c.x, c.y, c.w, c.h, bg);
        let strip = line_h + 8.0;
        frame.paint_rect(c.x, c.y, c.w, strip, mix(bg, fg, 0.06));
        let labels = vec!["Plots".to_string(), "Files".to_string()];
        self.plots.tab_rects = tabs(frame, renderer, theme, c.x + 6.0, c.y, strip, c.x + c.w, &labels, self.plots.tab);
        (self.plots.prev, self.plots.next, self.plots.save) = (None, None, None);
        let body = Rect { x: c.x, y: c.y + strip, w: c.w, h: (c.h - strip).max(0.0) };

        if self.plots.tab == 1 {
            self.files.refresh();
            let shown = self.files.dir.as_ref().map(|d| d.display().to_string()).unwrap_or_default();
            let tx = c.x + 8.0 + 16.0 * cell_w;
            text(frame, renderer, tx, c.y + 4.0 + line_h * 0.8, &shown, size, mix(fg, bg, 0.4), cell_w, c.x + c.w - 6.0);
            let body_y = c.y + line_h + 10.0;
            let rows = ((c.h - line_h - 14.0) / line_h).floor().max(0.0) as usize;
            for (i, (name, is_dir)) in self.files.entries.iter().skip(self.files.scroll).take(rows).enumerate() {
                let y = body_y + i as f32 * line_h;
                let (mark, col) = if *is_dir { ("▸ ", theme.console_banner) } else { ("  ", theme.console_input) };
                text(frame, renderer, c.x + 8.0, y + line_h * 0.8, &format!("{mark}{name}"), size, col, cell_w, c.x + c.w - 6.0);
            }
            return;
        }

        let n = self.plots.history.len();
        if n > 0 {
            let (y, h) = (c.y + 3.0, strip - 6.0);
            let mut right = c.x + c.w - 4.0;
            let r = button_left_of(frame, renderer, theme, right, y, h, "Save", false);
            self.plots.save = Some(r); right = r.x - 8.0;
            let r = button_left_of(frame, renderer, theme, right, y, h, "›", false);
            self.plots.next = Some(r); right = r.x - 2.0;
            let label = format!("{}/{}", self.plots.at + 1, n);
            let w = (label.chars().count() + 1) as f32 * cell_w;
            text(frame, renderer, right - w, y + (h - line_h) / 2.0 + line_h * 0.8, &label, size, mix(fg, bg, 0.3), cell_w, right);
            right -= w + 2.0;
            let r = button_left_of(frame, renderer, theme, right, y, h, "‹", false);
            self.plots.prev = Some(r);
        }
        if n == 0 {
            let note = "Plots appear here.";
            let x = body.x + (body.w - note.len() as f32 * cell_w) / 2.0;
            text(frame, renderer, x, body.y + body.h / 2.0, note, size, mix(fg, bg, 0.45), cell_w, body.x + body.w);
            return;
        }
        // A plot is a document: white in every theme, as in R.
        frame.paint_rect(body.x, body.y, body.w, body.h, Color::WHITE);
        let inner = Rect { x: body.x + 8.0, y: body.y + 8.0, w: (body.w - 16.0).max(0.0), h: (body.h - 16.0).max(0.0) };
        self.plots.panel.paint(frame, renderer, inner, theme);
    }
}
