//! Sakha's Tables: the results gathered into publishable tables.
//!
//! ```text
//!   In the tables          │ Table 1                               [Copy] [Save CSV]
//!   [x] 2. t-test: mpg…    │ Two-group t-tests by am
//!   [x] 3. t-test: wt…     │ ─────────────────────────────────────────────
//!   [ ] 5. t-test: qsec…   │ Variable   0: M (SD)     1: M (SD)     t   …
//!                          │ ─────────────────────────────────────────────
//!                          │ mpg        17.15 (3.83)  24.39 (6.17)  −3.77
//! ```
//!
//! Every result whose analysis makes a table is listed on the left; a click
//! leaves it out or puts it back. The tables (`sakha_core::tables`) are built
//! again each frame from the numbers R2 returned, so they follow the report:
//! its order, its removals, a rerun. They are copied (HTML for Word, tab-
//! separated text for a spreadsheet) or saved as HTML or CSV.

use r2_ui::{Frame, FrameCtx, InputEvent, MouseButton, Rect, Renderer, Theme};
use sakha_core::tables::{self, build_all, Entry, Table};

use crate::report::clip_rect;
use crate::ui::{button, button_left_of, inside, mix, text, wrap};
use crate::Gui;

/// The list of results on the left, in cells.
const LIST_W: usize = 34;

#[derive(Default)]
struct Hits {
    area: Option<Rect>,
    /// A result's row in the list: a click leaves it out or puts it back.
    rows: Vec<(Rect, u64)>,
    /// (button, action).
    buttons: Vec<(Rect, String)>,
}

#[derive(Default)]
pub(crate) struct TablesView {
    /// Pixels scrolled from the top of the tables.
    scroll: f32,
    line_h: f32,
    hits: Hits,
}

impl Gui {
    /// The tables, from the results the user keeps in them, in report order.
    pub(crate) fn sakha_tables(&self) -> Vec<Table> {
        let entries: Vec<Entry> = self.sakha.results.iter().filter(|r| !r.untabled)
            .filter_map(|r| r.numbers.as_ref().map(|n| Entry { analysis: r.a.id, labels: &r.labels, numbers: n }))
            .collect();
        build_all(&entries)
    }

    pub(crate) fn paint_tables(&mut self, area: Rect, frame: &mut Frame, renderer: &mut Renderer, theme: &Theme) {
        let (cw, lh) = renderer.cell_metrics(theme.fs());
        let (bg, fg, size) = (theme.window_background, theme.console_output, theme.fs());
        let dim = mix(fg, bg, 0.45);
        let made = self.sakha_tables();
        let tv = &mut self.sakha.tables;
        tv.hits = Hits::default();
        tv.line_h = lh;
        let tabled: Vec<_> = self.sakha.results.iter().enumerate().filter(|(_, r)| tables::tabulates(r.a.id)).collect();
        if tabled.is_empty() {
            let msg = "No tables yet. Tests from the Compare and Relate menus, and the summaries, are gathered here into \
                       publishable tables: run the same test on several variables and they share one table.";
            for (k, l) in wrap(msg, ((area.w / cw) as usize).max(20)).iter().enumerate() {
                text(frame, renderer, area.x, area.y + lh * (0.8 + k as f32), l, size, dim, cw, area.x + area.w);
            }
            return;
        }

        // ── the results that make tables, each in or out; saving them all below
        let strip = lh + 8.0;
        let lw = (LIST_W as f32 * cw).min(area.w * 0.4);
        let all = Rect { x: area.x + 6.0, y: area.y + area.h - strip, w: lw - 12.0, h: strip - 4.0 };
        frame.paint_rect(area.x, area.y, lw, area.h, mix(bg, fg, 0.03));
        button(frame, renderer, theme, all, "Save all as HTML…", !made.is_empty());
        tv.hits.buttons.push((all, "tables.html".into()));
        text(frame, renderer, area.x + 8.0, area.y + lh * 0.8, "In the tables (click to leave out)", size, dim, cw, area.x + lw);
        for (k, (i, r)) in tabled.iter().enumerate() {
            let y = area.y + lh * (1.4 + k as f32);
            if y + lh > all.y - 4.0 { break; }
            let row = Rect { x: area.x, y, w: lw, h: lh };
            let state = if r.running { "…" } else if r.numbers.is_none() { "!" } else if r.untabled { " " } else { "x" };
            let color = if r.untabled || r.numbers.is_none() { dim } else { fg };
            text(frame, renderer, area.x + 8.0, y + lh * 0.8, &format!("[{state}] {}. {}", i + 1, r.title), size, color, cw, area.x + lw - 4.0);
            tv.hits.rows.push((row, r.uid));
        }

        // ── the tables, one below another
        let body = Rect { x: area.x + lw + 12.0, y: area.y, w: (area.w - lw - 12.0).max(0.0), h: area.h };
        tv.hits.area = Some(body);
        let clip = (body.y, body.y + body.h);
        if made.is_empty() {
            let msg = if self.sakha.results.iter().any(|r| r.running) { "R2 is working…" }
                      else { "Every result is left out, or R2 could not compute its numbers (marked !)." };
            text(frame, renderer, body.x, body.y + lh * 0.8, msg, size, dim, cw, body.x + body.w);
            return;
        }
        let note_w = ((body.w / cw) as usize).saturating_sub(2).max(20);
        let heights: Vec<f32> = made.iter().map(|t| strip + lh * (1.2 + 1.4 + t.rows.len() as f32 + 0.4)
                                                     + lh * wrap(&format!("Note. {}", t.note), note_w).len() as f32 + lh * 1.5).collect();
        let total: f32 = heights.iter().sum();
        tv.scroll = tv.scroll.clamp(0.0, (total - body.h).max(0.0));
        let line_in = |base: f32| base - lh * 0.8 >= clip.0 && base + lh * 0.2 <= clip.1;
        let mut y0 = body.y - tv.scroll;
        for (n, t) in made.iter().enumerate() {
            let h = heights[n];
            if y0 > clip.1 { break; }
            if y0 + h < clip.0 { y0 += h; continue; }
            // "Table N" and what can be done with it
            let head = Rect { x: body.x, y: y0, w: body.w, h: strip };
            clip_rect(frame, head, clip, mix(bg, fg, 0.07));
            let mut right = body.x + body.w - 4.0;
            if head.y >= clip.0 && head.y + head.h <= clip.1 {
                for (label, action) in [("Save CSV", format!("tables.csv:{n}")), ("Copy", format!("tables.copy:{n}"))] {
                    let r = button_left_of(frame, renderer, theme, right, head.y + 2.0, strip - 4.0, label, false);
                    tv.hits.buttons.push((r, action));
                    right = r.x - 4.0;
                }
                text(frame, renderer, body.x + 8.0, head.y + (strip - lh) / 2.0 + lh * 0.8, &format!("Table {}", n + 1),
                     size, theme.console_banner, cw, right - cw);
            }
            let mut y = y0 + strip + 2.0;
            if line_in(y + lh * 0.8) { text(frame, renderer, body.x + 8.0, y + lh * 0.8, &t.title, size, fg, cw, body.x + body.w); }
            y += lh * 1.2;
            // columns as wide as their widest cell
            let widths: Vec<usize> = (0..t.columns.len()).map(|c| {
                t.rows.iter().filter_map(|r| r.get(c)).chain(std::iter::once(&t.columns[c]))
                    .map(|s| s.chars().count()).max().unwrap_or(0) + 2
            }).collect();
            let table_w = (widths.iter().sum::<usize>() as f32 * cw).min(body.w - 16.0);
            let rule = |frame: &mut Frame, y: f32| clip_rect(frame, Rect { x: body.x + 8.0, y, w: table_w, h: 1.0 }, clip, fg);
            let line = |frame: &mut Frame, renderer: &mut Renderer, y: f32, cells: &[String], color| {
                let base = y + lh * 0.8;
                if !line_in(base) { return; }
                let mut x = body.x + 8.0;
                for (c, cell) in cells.iter().enumerate() {
                    let w = widths.get(c).copied().unwrap_or(4) as f32 * cw;
                    // the label column is left-aligned, numbers centred, as APA sets them
                    let pad = if c == 0 { 0.0 } else { (w - cell.chars().count() as f32 * cw) / 2.0 };
                    text(frame, renderer, x + pad, base, cell, size, color, cw, (body.x + 8.0 + table_w).min(x + w));
                    x += w;
                }
            };
            rule(frame, y);
            line(frame, renderer, y + lh * 0.2, &t.columns, theme.console_banner);
            y += lh * 1.4;
            rule(frame, y - lh * 0.1);
            for r in &t.rows {
                line(frame, renderer, y, r, fg);
                y += lh;
            }
            rule(frame, y + lh * 0.2);
            y += lh * 0.4;
            for l in wrap(&format!("Note. {}", t.note), note_w) {
                if line_in(y + lh * 0.8) { text(frame, renderer, body.x + 8.0, y + lh * 0.8, &l, size, dim, cw, body.x + body.w); }
                y += lh;
            }
            y0 += h;
        }
    }

    pub(crate) fn tables_input(&mut self, events: &[InputEvent]) {
        let mouse = self.mouse;
        let mut queued = None;
        let s = &mut self.sakha;
        for ev in events {
            match *ev {
                InputEvent::MouseDown { button: MouseButton::Left, pos } => {
                    let (x, y) = (pos.x, pos.y);
                    if let Some((_, a)) = s.tables.hits.buttons.iter().find(|(r, _)| inside(*r, x, y)) {
                        queued = Some(a.clone());
                    } else if let Some(&(_, uid)) = s.tables.hits.rows.iter().find(|(r, _)| inside(*r, x, y)) {
                        if let Some(r) = s.results.iter_mut().find(|r| r.uid == uid) {
                            r.untabled = !r.untabled;
                            s.dirty = true;
                        }
                    }
                }
                InputEvent::Scroll { dy } if s.tables.hits.area.is_some_and(|r| inside(r, mouse.0, mouse.1)) => {
                    let step = s.tables.line_h * 3.0;
                    s.tables.scroll += if dy > 0.0 { -step } else { step };
                }
                _ => {}
            }
        }
        if queued.is_some() { self.queued = queued; }
    }

    /// `tables.copy:<n>`, `tables.csv:<n>`, `tables.html`.
    pub(crate) fn tables_action(&mut self, action: &str, ctx: &mut FrameCtx) {
        let made = self.sakha_tables();
        let (what, n) = action.strip_prefix("tables.").map(|a| a.split_once(':').unwrap_or((a, ""))).unwrap_or(("", ""));
        let one = n.parse::<usize>().ok().and_then(|k| made.get(k).map(|t| (k, t)));
        let df = self.sakha.df.clone().unwrap_or_else(|| "sakha".into());
        self.sakha.status = match (what, one) {
            ("copy", Some((k, t))) => {
                // Word takes the HTML as a real table; a spreadsheet the text
                let html = tables::to_html(std::slice::from_ref(t), &t.title).replacen("Table 1", &format!("Table {}", k + 1), 1);
                if ctx.clipboard.set_html(&html, &tables::to_tsv(t, k + 1)) { format!("Table {} copied: paste it into Word or a spreadsheet.", k + 1) }
                else { "Could not copy the table.".into() }
            }
            ("csv", Some((k, t))) => match self.save_text_as("Save table as CSV", &format!("{df}-table-{}.csv", k + 1), "CSV", "csv", &tables::to_csv(t)) {
                Some(msg) => msg,
                None => return,
            },
            ("html", _) if !made.is_empty() => {
                let html = tables::to_html(&made, &format!("Tables: {df}"));
                match self.save_text_as("Save tables as HTML", &format!("{df}-tables.html"), "Web page", "html", &html) {
                    Some(msg) => msg,
                    None => return,
                }
            }
            ("html", _) => "No tables to save yet.".into(),
            _ => return,
        };
    }

    /// Ask where, then write `body`; the status to show, or `None` if cancelled.
    fn save_text_as(&mut self, title: &str, name: &str, kind: &str, ext: &str, body: &str) -> Option<String> {
        let mut path = rfd::FileDialog::new().set_title(title).set_directory(&self.session.cwd).set_file_name(name)
            .add_filter(kind, &[ext]).save_file()?;
        if path.extension().is_none() { path.set_extension(ext); }
        let shown = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Some(match std::fs::write(&path, body) {
            Ok(()) => format!("Saved {shown}"),
            Err(e) => format!("Could not save {shown}: {e}"),
        })
    }
}
