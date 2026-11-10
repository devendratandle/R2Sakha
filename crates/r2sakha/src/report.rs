//! Sakha's Results: one scrolling report, each result below the last.
//!
//! ```text
//!   ┌ 3. t test: score by grp ─────────── [Code] [↑] [↓] [Copy] [Save plot] [×] ┐  Outline
//!   │ Sakha says: the groups differ (p = 0.012).                                │  1. Summary
//!   │ Welch Two Sample t-test …                                                 │  2. Histogram
//!   │ ┌ plot ┐                                                                  │ ▸3. t test
//! ```
//!
//! A block shows the title, the line in plain words, R2's own output, its
//! code (on request) and its plot at a readable size; clicking the plot
//! enlarges it. Plots share a few small atlas slots: only those on screen
//! are drawn, so a long report costs no more than a short one.

use std::collections::HashSet;

use r2_ui::{Color, Frame, FrameCtx, GraphPanel, InputEvent, KeyCode, MouseButton, Rect, Renderer, Theme};

use crate::sakha_view::SakhaResult;
use crate::ui::{button_left_of, inside, mix, text, wrap};
use crate::Gui;

/// Plot slots for the report: as many plots as fit on a screen at once.
const POOL: usize = 4;
const SLOT_W: u32 = 1020;
const SLOT_H: u32 = 760;
/// A plot's width in the report, in logical pixels, at most.
const PLOT_W: f32 = 600.0;
/// The outline's width in cells.
const OUTLINE_W: usize = 28;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Btn { Code, Up, Down, Copy, Save, Gview, Remove }

struct Slot {
    panel: GraphPanel,
    /// (result uid, plot generation) it holds.
    holds: Option<(u64, u64)>,
}

#[derive(Default)]
struct Hits {
    area: Option<Rect>,
    buttons: Vec<(Rect, u64, Btn)>,
    plots: Vec<(Rect, u64)>,
    outline: Vec<(Rect, u64)>,
    close: Option<Rect>,
    big_copy: Option<Rect>,
    big_save: Option<Rect>,
}

pub(crate) struct Report {
    /// Pixels scrolled from the report's top.
    scroll: f32,
    pool: Vec<Slot>,
    /// Bring this result to the top; kept while it runs, so its output
    /// stays in view as it arrives.
    jump: Option<u64>,
    code_shown: HashSet<u64>,
    /// Remove was clicked once on this result: a second click removes it.
    confirm: Option<u64>,
    /// The result whose plot fills the report.
    pub(crate) enlarged: Option<u64>,
    /// (uid, gen) the big panel shows.
    big_holds: Option<(u64, u64)>,
    /// The result at the top of the view.
    pub(crate) focus: Option<u64>,
    next_uid: u64,
    line_h: f32,
    hits: Hits,
}

impl Default for Report {
    fn default() -> Self {
        Report {
            scroll: 0.0,
            pool: (0..POOL).map(|_| Slot { panel: GraphPanel::with_slot(SLOT_W, SLOT_H), holds: None }).collect(),
            jump: None, code_shown: HashSet::new(), confirm: None, enlarged: None, big_holds: None,
            focus: None, next_uid: 0, line_h: 16.0, hits: Hits::default(),
        }
    }
}

impl Report {
    pub(crate) fn new_uid(&mut self) -> u64 {
        self.next_uid += 1;
        self.next_uid
    }

    /// A new analysis: nothing of the old report's view carries over.
    pub(crate) fn reset(&mut self) {
        self.scroll = 0.0;
        self.jump = None;
        self.code_shown.clear();
        self.confirm = None;
        self.enlarged = None;
        self.big_holds = None;
        self.focus = None;
        for s in &mut self.pool { s.holds = None; }
    }

    pub(crate) fn jump_to(&mut self, uid: u64) { self.jump = Some(uid); }

    /// The focused result's place, for saving the view.
    pub(crate) fn focus_index(&self, results: &[SakhaResult]) -> usize {
        self.focus.and_then(|u| results.iter().position(|r| r.uid == u)).unwrap_or(0)
    }
}

/// An SVG's height over its width, from its opening tag.
pub(crate) fn svg_aspect(svg: &str) -> Option<f32> {
    let start = svg.find("<svg")?;
    let head = &svg[start..start + svg[start..].find('>')?];
    let num = |attr: &str| -> Option<f32> {
        let i = head.find(&format!(" {attr}=\""))? + attr.len() + 3;
        let v: String = head[i..].chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
        v.parse().ok().filter(|v: &f32| *v > 0.0)
    };
    Some(num("height")? / num("width")?)
}

/// What a block takes, top to bottom.
struct Block {
    says: Vec<String>,
    code_h: f32,
    out_h: f32,
    plot: Option<(f32, f32)>,
    total: f32,
}

#[allow(clippy::too_many_arguments)]
fn measure(r: &SakhaResult, code: bool, width: f32, cw: f32, lh: f32, strip: f32, plot_w: f32, max_h: f32) -> Block {
    let chars = ((width - 16.0) / cw).floor().max(10.0) as usize;
    let says = wrap(&r.says, chars);
    let code_h = if code { r.code.len() as f32 * lh + 8.0 } else { 0.0 };
    let lines = r.output.lines().count();
    let out_h = if lines > 0 { lines as f32 * lh + 6.0 } else { 0.0 };
    let plot = r.svg.as_ref().map(|_| {
        let mut w = plot_w.min(width - 16.0).max(80.0);
        let mut h = w * r.aspect;
        let cap = max_h.min(SLOT_H as f32);
        if h > cap { h = cap; w = h / r.aspect; }
        (w, h)
    });
    let total = strip + says.len() as f32 * lh + 6.0 + code_h + out_h + plot.map_or(0.0, |(_, h)| h + 16.0) + 14.0;
    Block { says, code_h, out_h, plot, total }
}

/// `r` cut to the band `clip`, painted when anything of it is left.
pub(crate) fn clip_rect(frame: &mut Frame, r: Rect, clip: (f32, f32), color: Color) {
    let (t, b) = (r.y.max(clip.0), (r.y + r.h).min(clip.1));
    if b > t { frame.paint_rect(r.x, t, r.w, b - t, color); }
}

impl Gui {
    pub(crate) fn paint_results(&mut self, area: Rect, frame: &mut Frame, renderer: &mut Renderer, theme: &Theme) {
        let (cw, lh) = renderer.cell_metrics(theme.fs());
        let (bg, fg, size) = (theme.window_background, theme.console_output, theme.fs());
        let dim = mix(fg, bg, 0.45);
        self.sakha.report.hits = Hits::default();
        self.sakha.report.line_h = lh;
        if self.sakha.results.is_empty() {
            text(frame, renderer, area.x, area.y + lh * 0.8, "No results yet. Choose an analysis from the Summarize, Compare, Relate or Plot menus.", size, dim, cw, area.x + area.w);
            return;
        }
        if let Some(uid) = self.sakha.report.enlarged {
            if self.sakha.result(uid).is_some_and(|r| r.svg.is_some()) {
                self.paint_enlarged(uid, area, frame, renderer, theme);
                return;
            }
            self.close_enlarged();
        }
        let strip = lh + 8.0;
        let outline = self.sakha.results.len() > 1 && area.w > (OUTLINE_W + 60) as f32 * cw;
        let ow = if outline { OUTLINE_W as f32 * cw + 10.0 } else { 0.0 };
        let body = Rect { x: area.x, y: area.y, w: area.w - ow, h: area.h };
        let clip = (body.y, body.y + body.h);
        let plot_w = PLOT_W * theme.dpi;

        let s = &mut self.sakha;
        let (results, rep) = (&s.results, &mut s.report);
        rep.hits.area = Some(body);
        let blocks: Vec<Block> = results.iter()
            .map(|r| measure(r, rep.code_shown.contains(&r.uid), body.w, cw, lh, strip, plot_w, body.h * 0.85))
            .collect();
        let mut tops = Vec::with_capacity(blocks.len());
        let mut total = 0.0;
        for b in &blocks { tops.push(total); total += b.total; }

        // where the view is
        if let Some(uid) = rep.jump {
            match results.iter().position(|r| r.uid == uid) {
                Some(i) => {
                    rep.scroll = tops[i];
                    if !results[i].running { rep.jump = None; }
                }
                None => rep.jump = None,
            }
        }
        rep.scroll = rep.scroll.clamp(0.0, (total - body.h).max(0.0));
        // the block a third of the way down the view is the one being read
        let at = tops.iter().rposition(|t| *t <= rep.scroll + body.h / 3.0).unwrap_or(0);
        rep.focus = Some(results[at].uid);

        // which plots are on screen, and the slots they draw in
        let plot_rect = |i: usize| -> Option<Rect> {
            let b = &blocks[i];
            let (w, h) = b.plot?;
            let y = body.y + tops[i] - rep.scroll + strip + b.says.len() as f32 * lh + 6.0 + b.code_h + b.out_h + 8.0;
            Some(Rect { x: body.x + 8.0, y, w, h })
        };
        let wanted: Vec<(usize, (u64, u64))> = (0..results.len())
            .filter(|&i| plot_rect(i).is_some_and(|p| p.y < clip.1 && p.y + p.h > clip.0))
            .map(|i| (i, (results[i].uid, results[i].gen)))
            .collect();
        let mut slot_of: Vec<Option<usize>> = vec![None; results.len()];
        for &(i, key) in &wanted {
            slot_of[i] = rep.pool.iter().position(|s| s.holds == Some(key));
        }
        for &(i, key) in &wanted {
            if slot_of[i].is_some() { continue; }
            let free = (0..rep.pool.len()).find(|k| !slot_of.contains(&Some(*k)));
            if let Some(k) = free {
                if let Some(svg) = &results[i].svg { rep.pool[k].panel.set_svg(svg.clone().into_bytes()); }
                rep.pool[k].holds = Some(key);
                slot_of[i] = Some(k);
            }
        }

        // the blocks
        for (i, r) in results.iter().enumerate() {
            let b = &blocks[i];
            let y0 = body.y + tops[i] - rep.scroll;
            if y0 > clip.1 { break; }
            if y0 + b.total < clip.0 { continue; }
            let head = Rect { x: body.x, y: y0, w: body.w, h: strip };
            clip_rect(frame, head, clip, mix(bg, fg, 0.07));
            let line_in = |base: f32| base - lh * 0.8 >= clip.0 && base + lh * 0.2 <= clip.1;
            if head.y >= clip.0 && head.y + head.h <= clip.1 {
                let (by, bh) = (head.y + 2.0, strip - 4.0);
                let mut right = body.x + body.w - 4.0;
                let sure = rep.confirm == Some(r.uid);
                let mut btn = |label: &str, on: bool, what: Btn, right: &mut f32| {
                    let rr = button_left_of(frame, renderer, theme, *right, by, bh, label, on);
                    rep.hits.buttons.push((rr, r.uid, what));
                    *right = rr.x - 4.0;
                };
                btn(if sure { "Remove?" } else { "×" }, sure, Btn::Remove, &mut right);
                if r.svg.is_some() { btn("Save plot", false, Btn::Save, &mut right); }
                if r.svg.is_some() { btn("Gview", false, Btn::Gview, &mut right); }
                btn("Copy", false, Btn::Copy, &mut right);
                if i + 1 < results.len() { btn("↓", false, Btn::Down, &mut right); }
                if i > 0 { btn("↑", false, Btn::Up, &mut right); }
                btn("Code", rep.code_shown.contains(&r.uid), Btn::Code, &mut right);
                let title = format!("{}. {}{}", i + 1, r.title, if r.running { "  (running…)" } else { "" });
                text(frame, renderer, head.x + 8.0, head.y + (strip - lh) / 2.0 + lh * 0.8, &title, size, theme.console_banner, cw, right - cw);
            }
            let mut y = y0 + strip + 2.0;
            for l in &b.says {
                let base = y + lh * 0.8;
                if line_in(base) { text(frame, renderer, body.x + 8.0, base, l, size, fg, cw, body.x + body.w - 8.0); }
                y += lh;
            }
            y += 4.0;
            if b.code_h > 0.0 {
                clip_rect(frame, Rect { x: body.x + 4.0, y, w: body.w - 8.0, h: b.code_h - 4.0 }, clip, mix(bg, fg, 0.10));
                for (k, l) in r.code.iter().enumerate() {
                    let base = y + 2.0 + lh * (0.8 + k as f32);
                    if line_in(base) { text(frame, renderer, body.x + 10.0, base, l, size, theme.console_input, cw, body.x + body.w - 8.0); }
                }
                y += b.code_h;
            }
            if b.out_h > 0.0 {
                for (k, l) in r.output.lines().enumerate() {
                    let base = y + lh * (0.8 + k as f32);
                    if base > clip.1 + lh { break; }
                    if !line_in(base) { continue; }
                    let color = if l.starts_with("Error") { theme.console_error } else { fg };
                    text(frame, renderer, body.x + 8.0, base, l, size, color, cw, body.x + body.w - 8.0);
                }
            }
            if let Some(p) = plot_rect(i) {
                let pad = Rect { x: p.x - 8.0, y: p.y - 8.0, w: p.w + 16.0, h: p.h + 16.0 };
                clip_rect(frame, pad, clip, Color::WHITE);
                match slot_of[i] {
                    Some(k) => rep.pool[k].panel.paint_clipped(frame, renderer, p, clip, theme),
                    None => {
                        let base = p.y + p.h / 2.0;
                        if line_in(base) { text(frame, renderer, p.x + 8.0, base, "(plot: scroll to see it)", size, dim, cw, p.x + p.w); }
                    }
                }
                let (t, bt) = (p.y.max(clip.0), (p.y + p.h).min(clip.1));
                if bt > t { rep.hits.plots.push((Rect { x: p.x, y: t, w: p.w, h: bt - t }, r.uid)); }
            }
        }

        // the outline: every result, the one in view marked
        if outline {
            let o = Rect { x: body.x + body.w + 10.0, y: area.y, w: ow - 10.0, h: area.h };
            frame.paint_rect(o.x, o.y, o.w, o.h, mix(bg, fg, 0.03));
            text(frame, renderer, o.x + 8.0, o.y + lh * 0.8, "Outline", size, dim, cw, o.x + o.w);
            let fit = ((o.h - lh * 1.4) / lh).floor().max(1.0) as usize;
            let first = at.saturating_sub(fit / 2).min(results.len().saturating_sub(fit));
            for (k, r) in results.iter().enumerate().skip(first).take(fit) {
                let y = o.y + lh * 1.4 + (k - first) as f32 * lh;
                let row = Rect { x: o.x, y, w: o.w, h: lh };
                let on = k == at;
                if on { frame.paint_rect(row.x, row.y, row.w, row.h, theme.console_selection_bg); }
                let label = format!("{}. {}", k + 1, r.title);
                text(frame, renderer, o.x + 8.0, y + lh * 0.8, &label, size,
                     if on { theme.console_selection_fg } else { fg }, cw, o.x + o.w - 4.0);
                rep.hits.outline.push((row, r.uid));
            }
        }
    }

    /// One plot filling the report, with what can be done with it.
    fn paint_enlarged(&mut self, uid: u64, area: Rect, frame: &mut Frame, renderer: &mut Renderer, theme: &Theme) {
        let (cw, lh) = renderer.cell_metrics(theme.fs());
        let (bg, fg, size) = (theme.window_background, theme.console_output, theme.fs());
        let strip = lh + 8.0;
        let Some(r) = self.sakha.result(uid) else { return };
        let (title, aspect, key) = (r.title.clone(), r.aspect, (r.uid, r.gen));
        if self.sakha.report.big_holds != Some(key) {
            if let Some(svg) = &r.svg { self.plots.panel.set_svg(svg.clone().into_bytes()); }
            self.sakha.report.big_holds = Some(key);
        }
        frame.paint_rect(area.x, area.y, area.w, strip, mix(bg, fg, 0.07));
        let (by, bh) = (area.y + 2.0, strip - 4.0);
        let rep = &mut self.sakha.report;
        let mut right = area.x + area.w - 4.0;
        let close = button_left_of(frame, renderer, theme, right, by, bh, "Close ×", false);
        right = close.x - 4.0;
        let save = button_left_of(frame, renderer, theme, right, by, bh, "Save plot", false);
        right = save.x - 4.0;
        let copy = button_left_of(frame, renderer, theme, right, by, bh, "Copy image", false);
        right = copy.x - 4.0;
        rep.hits.close = Some(close);
        rep.hits.big_save = Some(save);
        rep.hits.big_copy = Some(copy);
        text(frame, renderer, area.x + 8.0, area.y + (strip - lh) / 2.0 + lh * 0.8, &title, size, theme.console_banner, cw, right - cw);
        let room = Rect { x: area.x, y: area.y + strip + 4.0, w: area.w, h: (area.h - strip - 4.0).max(0.0) };
        frame.paint_rect(room.x, room.y, room.w, room.h, Color::WHITE);
        let (mut w, mut h) = (room.w - 24.0, (room.w - 24.0) * aspect);
        if h > room.h - 24.0 { h = room.h - 24.0; w = h / aspect; }
        let p = Rect { x: room.x + (room.w - w) / 2.0, y: room.y + (room.h - h) / 2.0, w: w.max(0.0), h: h.max(0.0) };
        self.plots.panel.paint(frame, renderer, p, theme);
    }

    /// Back to the report; the Plots pane shows its own plot again.
    pub(crate) fn close_enlarged(&mut self) {
        let rep = &mut self.sakha.report;
        if rep.enlarged.take().is_some() || rep.big_holds.take().is_some() {
            self.plots.show_current();
        }
    }

    /// Clicks, the wheel and Esc on the report.
    pub(crate) fn report_input(&mut self, events: &[InputEvent]) {
        let mouse = self.mouse;
        let mut queued = None;
        let mut close = false;
        {
            let s = &mut self.sakha;
            let rep = &mut s.report;
            for ev in events {
                match *ev {
                    InputEvent::MouseDown { button: MouseButton::Left, pos } => {
                        let (x, y) = (pos.x, pos.y);
                        let hit = |r: &Option<Rect>| r.is_some_and(|r| inside(r, x, y));
                        if rep.enlarged.is_some() {
                            let uid = rep.enlarged.unwrap_or(0);
                            if hit(&rep.hits.close) { close = true; }
                            else if hit(&rep.hits.big_save) { queued = Some(format!("result.save:{uid}")); }
                            else if hit(&rep.hits.big_copy) { queued = Some(format!("result.copy_image:{uid}")); }
                            continue;
                        }
                        let was = rep.confirm.take();
                        if let Some(&(_, uid, what)) = rep.hits.buttons.iter().find(|(r, _, _)| inside(*r, x, y)) {
                            let i = s.results.iter().position(|r| r.uid == uid);
                            match (what, i) {
                                (Btn::Code, _) => { if !rep.code_shown.remove(&uid) { rep.code_shown.insert(uid); } }
                                (Btn::Up, Some(i)) if i > 0 => { s.results.swap(i, i - 1); s.dirty = true; rep.jump = Some(uid); }
                                (Btn::Down, Some(i)) if i + 1 < s.results.len() => { s.results.swap(i, i + 1); s.dirty = true; rep.jump = Some(uid); }
                                (Btn::Copy, _) => queued = Some(format!("result.copy:{uid}")),
                                (Btn::Save, _) => queued = Some(format!("result.save:{uid}")),
                                (Btn::Gview, _) => queued = Some(format!("result.gview:{uid}")),
                                (Btn::Remove, Some(i)) => {
                                    if was == Some(uid) {
                                        s.results.remove(i);
                                        rep.code_shown.remove(&uid);
                                        s.dirty = true;
                                        s.status = "Result removed from the report (the script keeps its step).".into();
                                    } else {
                                        rep.confirm = Some(uid);
                                    }
                                }
                                _ => {}
                            }
                        } else if let Some(&(_, uid)) = rep.hits.plots.iter().find(|(r, _)| inside(*r, x, y)) {
                            rep.enlarged = Some(uid);
                        } else if let Some(&(_, uid)) = rep.hits.outline.iter().find(|(r, _)| inside(*r, x, y)) {
                            rep.jump = Some(uid);
                        }
                    }
                    InputEvent::Scroll { dy } if rep.enlarged.is_none() && rep.hits.area.is_some_and(|r| inside(r, mouse.0, mouse.1)) => {
                        rep.jump = None;
                        let step = rep.line_h * 3.0;
                        rep.scroll += if dy > 0.0 { -step } else { step };
                    }
                    InputEvent::Key { code: KeyCode::Escape, pressed: true, .. } if rep.enlarged.is_some() => close = true,
                    _ => {}
                }
            }
        }
        if close { self.close_enlarged(); }
        if queued.is_some() { self.queued = queued; }
    }

    /// `result.copy:<uid>`, `result.copy_image:<uid>`, `result.save:<uid>`.
    pub(crate) fn result_action(&mut self, action: &str, ctx: &mut FrameCtx) {
        let Some((what, uid)) = action.strip_prefix("result.").and_then(|a| a.split_once(':')) else { return };
        let Some(r) = uid.parse().ok().and_then(|u| self.sakha.result(u)) else { return };
        let svg = r.svg.clone();
        match what {
            "copy" => {
                let mut t = format!("{}\n{}\n", r.title, r.says);
                if !r.output.trim().is_empty() { t.push('\n'); t.push_str(r.output.trim_end()); t.push('\n'); }
                ctx.clipboard.set_text(&t);
                self.sakha.status = "Result copied.".into();
            }
            "copy_image" => {
                let Some(svg) = svg else { return };
                let (w, h) = plot_pixels(&svg);
                self.sakha.status = match r2_ui::graph::rasterize_plot(svg.as_bytes(), w, h) {
                    Some(rgba) if ctx.clipboard.set_image(w, h, &rgba) => format!("Plot copied as a {w}×{h} image."),
                    _ => "Could not copy the plot.".into(),
                };
            }
            "gview" => {
                let (title, code) = (r.title.clone(), r.code.clone());
                self.open_in_gview(&title, &code);
            }
            "save" => {
                let Some(svg) = svg else { return };
                let name = format!("{}.svg", file_stem(&r.title));
                let (w, h) = plot_pixels(&svg);
                if let Some(msg) = self.save_svg(&svg, &name, (w, h), false) { self.sakha.status = msg; }
            }
            _ => {}
        }
    }
}

/// Pixels for a plot saved or copied as a picture: twice its own size.
fn plot_pixels(svg: &str) -> (u32, u32) {
    let aspect = svg_aspect(svg).unwrap_or(2.0 / 3.0);
    (1400, (1400.0 * aspect).round() as u32)
}

/// A title as a file name.
fn file_stem(title: &str) -> String {
    let s: String = title.chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' }).collect();
    let s = s.trim_matches('_').to_string();
    if s.is_empty() { "plot".into() } else { s.chars().take(60).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aspect_from_the_opening_tag() {
        assert_eq!(svg_aspect(r#"<svg xmlns="x" width="600" height="400" viewBox="0 0 600 400">"#), Some(400.0 / 600.0));
        assert_eq!(svg_aspect(r#"<svg width="700px" height="350px">"#), Some(0.5));
        assert_eq!(svg_aspect("<svg>"), None);
    }

    #[test]
    fn titles_become_file_names() {
        assert_eq!(file_stem("Histogram of score"), "Histogram_of_score");
        assert_eq!(file_stem("t test: a ~ b"), "t_test__a___b");
        assert_eq!(file_stem("***"), "plot");
    }
}
