//! The Environment pane: two tabs, as RStudio's top-right pane.
//!
//! * **Environment** — every variable in the session, grouped Data / Values /
//!   Functions, with the one-line summary R2 writes (`env` request). Clicking
//!   a data frame, matrix or vector opens it in the data viewer; `[Sakha]`
//!   opens a data frame in Sakha.
//! * **History** — the commands run so far. Clicking one puts it on the
//!   console's prompt line, to edit or rerun.

use r2_ui::{Frame, InputEvent, MouseButton, Rect, Renderer, Theme};
use sakha_engine::Value;

use crate::ui::{button_left_of, inside, mix, tabs, text};
use crate::Gui;

const TABS: [&str; 2] = ["Environment", "History"];

/// One variable, as R2 described it.
#[derive(Clone)]
pub(crate) struct EnvRow {
    pub(crate) name: String,
    pub(crate) group: String,
    pub(crate) summary: String,
    pub(crate) viewable: bool,
}

impl EnvRow {
    pub(crate) fn is_data_frame(&self) -> bool { self.summary.contains(" obs. of ") }
}

#[derive(Default)]
pub(crate) struct EnvPane {
    pub(crate) rows: Vec<EnvRow>,
    tab: usize,
    scroll: usize,
    tab_rects: Vec<Rect>,
    import: Option<Rect>,
}

/// What a click on a line does.
#[derive(Clone)]
enum Click { None, View(String), ToConsole(String) }

impl Gui {
    pub(crate) fn env_reply(&mut self, body: &Value) {
        let field = |r: &Value, k: &str| r[k].as_str().unwrap_or("").to_string();
        self.env.rows = body["rows"].as_array().map(|a| a.iter().map(|r| EnvRow {
            name: field(r, "name"), group: field(r, "group"), summary: field(r, "summary"),
            viewable: r["viewable"].as_bool().unwrap_or(false),
        }).collect()).unwrap_or_default();
    }

    /// One line per entry in the current tab: (text, a group heading?, click).
    fn env_lines(&self) -> Vec<(String, bool, Click)> {
        if self.env.tab == 1 {
            return self.buffer.history().iter()
                .map(|h| (h.replace('\n', " ⏎ "), false, Click::ToConsole(h.clone()))).collect();
        }
        let rows = &self.env.rows;
        if rows.is_empty() { return vec![("Environment is empty".into(), false, Click::None)]; }
        let name_w = rows.iter().map(|r| r.name.chars().count()).max().unwrap_or(0).clamp(4, 18);
        let mut out = Vec::new();
        let mut group: Option<&str> = None;
        for r in rows {
            if group != Some(r.group.as_str()) {
                group = Some(&r.group);
                out.push((r.group.clone(), true, Click::None));
            }
            let name: String = if r.name.chars().count() > name_w {
                r.name.chars().take(name_w - 1).chain(['…']).collect()
            } else { format!("{:<w$}", r.name, w = name_w) };
            let click = if r.viewable { Click::View(r.name.clone()) } else { Click::None };
            let sakha = if r.is_data_frame() { "  [Sakha]" } else { "" };
            out.push((format!("{name}  {}{sakha}", r.summary), false, click));
        }
        out
    }

    pub(crate) fn env_input(&mut self, events: &[InputEvent], renderer: &mut Renderer, theme: &Theme) {
        let Some(c) = self.mdi.window(self.env_id).filter(|w| w.visible).map(|w| w.content_rect(theme)) else { return };
        let (cell_w, line_h) = renderer.cell_metrics(theme.fs());
        let body_y = c.y + line_h + 10.0;
        for ev in events {
            match *ev {
                InputEvent::MouseDown { button: MouseButton::Left, pos } if inside(c, pos.x, pos.y) => {
                    if self.env.import.is_some_and(|r| inside(r, pos.x, pos.y)) {
                        self.queued = Some("data.import".into());
                        continue;
                    }
                    if pos.y < body_y {
                        if let Some(i) = self.env.tab_rects.iter().position(|r| inside(*r, pos.x, pos.y)) {
                            self.env.tab = i;
                            self.env.scroll = 0;
                        }
                        continue;
                    }
                    let row = ((pos.y - body_y) / line_h).floor() as usize + self.env.scroll;
                    let line = self.env_lines().into_iter().nth(row);
                    // a click on "[Sakha]" opens Sakha; elsewhere, the viewer
                    let col = ((pos.x - c.x - 8.0) / cell_w).floor().max(0.0) as usize;
                    if let Some((t, _, Click::View(name))) = &line {
                        if let Some(at) = t.find("[Sakha]").map(|b| t[..b].chars().count()) {
                            if col >= at && col < at + 7 {
                                self.queued = Some(format!("sakha.df:{name}"));
                                continue;
                            }
                        }
                    }
                    match line.map(|l| l.2) {
                        Some(Click::View(name)) => self.open_data_tab(&name),
                        Some(Click::ToConsole(cmd)) => self.input.set_line(cmd),
                        _ => {}
                    }
                }
                InputEvent::Scroll { dy } if inside(c, self.mouse.0, self.mouse.1) => {
                    let n = self.env_lines().len();
                    self.env.scroll = if dy > 0.0 { self.env.scroll.saturating_sub(3) }
                                      else { (self.env.scroll + 3).min(n.saturating_sub(1)) };
                }
                _ => {}
            }
        }
    }

    pub(crate) fn paint_env(&mut self, c: Rect, frame: &mut Frame, renderer: &mut Renderer, theme: &Theme) {
        let (cell_w, line_h) = renderer.cell_metrics(theme.fs());
        let (bg, fg, size) = (theme.window_background, theme.console_output, theme.fs());
        frame.paint_rect(c.x, c.y, c.w, c.h, bg);
        let strip = line_h + 8.0;
        frame.paint_rect(c.x, c.y, c.w, strip, mix(bg, fg, 0.06));
        let labels: Vec<String> = TABS.iter().map(|t| t.to_string()).collect();
        self.env.tab_rects = tabs(frame, renderer, theme, c.x + 6.0, c.y, strip, c.x + c.w, &labels, self.env.tab);
        self.env.import = Some(button_left_of(frame, renderer, theme, c.x + c.w - 4.0, c.y + 3.0, strip - 6.0, "Import CSV", false));
        let lines = self.env_lines();
        // History opens at its newest entries, like a console transcript.
        let rows = ((c.h - line_h - 14.0) / line_h).floor().max(0.0) as usize;
        if self.env.tab == 1 && self.env.scroll == 0 && lines.len() > rows {
            self.env.scroll = lines.len() - rows;
        }
        let body_y = c.y + line_h + 10.0;
        let max_x = c.x + c.w - 6.0;
        for (i, (t, heading, click)) in lines.iter().skip(self.env.scroll).take(rows).enumerate() {
            let y = body_y + i as f32 * line_h;
            if *heading { frame.paint_rect(c.x, y, c.w, line_h, mix(bg, fg, 0.08)); }
            let col = if *heading { theme.console_banner }
                      else if matches!(click, Click::View(_)) { theme.console_input } else { fg };
            text(frame, renderer, c.x + 8.0, y + line_h * 0.8, t, size, col, cell_w, max_x);
        }
    }
}
