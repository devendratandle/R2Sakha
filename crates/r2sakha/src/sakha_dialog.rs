//! Sakha's analysis dialog: the user picks variables for each role and R2
//! does the rest. It is the only thing an analysis asks of the user; no code
//! to type, and the variables offered are only those the role can take.

use r2_ui::{Frame, InputEvent, KeyCode, MouseButton, Rect, Renderer, Theme};
use sakha_core::analysis::{Accepts, Analysis};
use sakha_core::profile::ColumnProfile;

use crate::ui::{button, inside, mix, text, wrap};
use crate::Gui;

pub(crate) struct AnalysisDialog {
    pub(crate) a: &'static Analysis,
    /// Picked columns, one list per role.
    picks: Vec<Vec<usize>>,
    /// The role being filled.
    role: usize,
    /// A typed role's text.
    typed: String,
    message: String,
    scroll: usize,
    roles: Vec<Rect>,
    vars: Vec<(Rect, usize)>,
    run: Option<Rect>,
    cancel: Option<Rect>,
    list: Option<Rect>,
}

enum Outcome { Stay, Close, Run }

impl AnalysisDialog {
    /// A dialog for `a`; the selected variable fills the first role it fits.
    pub(crate) fn new(a: &'static Analysis, cols: &[ColumnProfile], selected: Option<usize>) -> AnalysisDialog {
        let mut picks = vec![vec![]; a.roles.len()];
        let mut role = 0;
        if let Some(i) = selected {
            if let Some(k) = a.roles.iter().position(|r| cols.get(i).is_some_and(|c| r.accepts.takes(c))) {
                picks[k].push(i);
                role = (k + 1).min(a.roles.len() - 1);
            }
        }
        let typed = a.roles.iter().find_map(|r| if let Accepts::Typed(d) = r.accepts { Some(d.to_string()) } else { None }).unwrap_or_default();
        AnalysisDialog { a, picks, role, typed, message: String::new(), scroll: 0, roles: vec![], vars: vec![],
                         run: None, cancel: None, list: None }
    }

    fn toggle(&mut self, col: usize) {
        let r = self.a.roles[self.role];
        let p = &mut self.picks[self.role];
        if r.many {
            if let Some(k) = p.iter().position(|&c| c == col) { p.remove(k); } else { p.push(col); }
        } else {
            *p = vec![col];
            // on to the next role that is still empty
            if let Some(n) = (0..self.a.roles.len()).find(|&k| self.picks[k].is_empty() && !matches!(self.a.roles[k].accepts, Accepts::Typed(_))) {
                self.role = n;
            }
        }
        self.message.clear();
    }

    fn input(&mut self, events: &[InputEvent], mouse: (f32, f32)) -> Outcome {
        let typed_role = matches!(self.a.roles[self.role].accepts, Accepts::Typed(_));
        for ev in events {
            match *ev {
                InputEvent::Key { code: KeyCode::Escape, pressed: true, .. } => return Outcome::Close,
                InputEvent::Key { code: KeyCode::Enter, pressed: true, .. } => return Outcome::Run,
                InputEvent::Key { code: KeyCode::Backspace, pressed: true, .. } if typed_role => { self.typed.pop(); }
                InputEvent::Char(ch) if typed_role && (ch.is_ascii_digit() || matches!(ch, '.' | '-' | 'e')) => self.typed.push(ch),
                InputEvent::Scroll { dy } if self.list.is_some_and(|r| inside(r, mouse.0, mouse.1)) => {
                    self.scroll = if dy > 0.0 { self.scroll.saturating_sub(2) } else { self.scroll + 2 };
                }
                InputEvent::MouseDown { button: MouseButton::Left, pos } => {
                    let hit = |r: &Option<Rect>| r.is_some_and(|r| inside(r, pos.x, pos.y));
                    if hit(&self.cancel) { return Outcome::Close; }
                    if hit(&self.run) { return Outcome::Run; }
                    if let Some(k) = self.roles.iter().position(|r| inside(*r, pos.x, pos.y)) { self.role = k; self.scroll = 0; }
                    else if let Some(&(_, c)) = self.vars.iter().find(|(r, _)| inside(*r, pos.x, pos.y)) { self.toggle(c); }
                }
                _ => {}
            }
        }
        Outcome::Stay
    }

    fn paint(&mut self, cols: &[ColumnProfile], area: Rect, frame: &mut Frame, renderer: &mut Renderer, theme: &Theme) {
        let (cw, lh) = renderer.cell_metrics(theme.fs());
        let (bg, fg, size) = (theme.window_background, theme.console_output, theme.fs());
        let dim = mix(fg, bg, 0.45);
        // the box, over a dimmed workspace
        frame.paint_rect(area.x, area.y, area.w, area.h, r2_ui::Color::rgba(0, 0, 0, 120));
        let (w, h) = ((72.0 * cw).min(area.w - 20.0), (24.0 * lh).min(area.h - 20.0));
        let b = Rect { x: area.x + (area.w - w) / 2.0, y: area.y + (area.h - h) / 2.0, w, h };
        frame.paint_rect(b.x - 1.0, b.y - 1.0, b.w + 2.0, b.h + 2.0, mix(bg, fg, 0.3));
        frame.paint_rect(b.x, b.y, b.w, b.h, bg);
        frame.paint_rect(b.x, b.y, b.w, lh + 10.0, mix(bg, fg, 0.1));
        text(frame, renderer, b.x + 10.0, b.y + 5.0 + lh * 0.8, self.a.title, size, theme.console_banner, cw, b.x + b.w);
        let mut y = b.y + lh + 16.0;
        for line in wrap(self.a.question, ((b.w - 20.0) / cw) as usize) {
            text(frame, renderer, b.x + 10.0, y + lh * 0.8, &line, size, fg, cw, b.x + b.w - 8.0);
            y += lh;
        }
        y += 8.0;

        // left: the roles, each with what it holds
        let foot = lh + 16.0;
        let left_w = b.w * 0.46;
        let body_h = b.y + b.h - foot - y;
        self.roles.clear();
        let mut ry = y;
        for (k, role) in self.a.roles.iter().enumerate() {
            let r = Rect { x: b.x + 10.0, y: ry, w: left_w - 16.0, h: 2.0 * lh + 8.0 };
            frame.paint_rect(r.x, r.y, r.w, r.h, if k == self.role { theme.console_selection_bg } else { mix(bg, fg, 0.06) });
            let ink = if k == self.role { theme.console_selection_fg } else { fg };
            text(frame, renderer, r.x + 6.0, r.y + 3.0 + lh * 0.8, role.label, size, if k == self.role { ink } else { dim }, cw, r.x + r.w);
            let held = match role.accepts {
                Accepts::Typed(_) => format!("{}_", self.typed),
                _ => {
                    let names: Vec<&str> = self.picks[k].iter().filter_map(|&i| cols.get(i)).map(|c| c.name.as_str()).collect();
                    if names.is_empty() { "choose →".to_string() } else { names.join(", ") }
                }
            };
            text(frame, renderer, r.x + 6.0, r.y + 3.0 + lh * 1.8, &held, size, ink, cw, r.x + r.w);
            self.roles.push(r);
            ry += r.h + 6.0;
        }

        // right: the variables this role can take
        let list = Rect { x: b.x + left_w, y, w: b.w - left_w - 10.0, h: body_h.max(0.0) };
        self.list = Some(list);
        frame.paint_rect(list.x, list.y, list.w, list.h, mix(bg, fg, 0.04));
        self.vars.clear();
        let accepts = self.a.roles[self.role].accepts;
        if let Accepts::Typed(_) = accepts {
            text(frame, renderer, list.x + 6.0, list.y + lh * 0.8, "Type a number.", size, dim, cw, list.x + list.w);
        } else {
            let offered: Vec<usize> = (0..cols.len()).filter(|&i| accepts.takes(&cols[i])).collect();
            if offered.is_empty() {
                let note = if cols.iter().any(|c| !c.ready) { "Sakha is still reading the variables…" } else { "No variable in this dataset fits." };
                text(frame, renderer, list.x + 6.0, list.y + lh * 0.8, note, size, dim, cw, list.x + list.w);
            }
            let fit = (list.h / lh).floor().max(1.0) as usize;
            self.scroll = self.scroll.min(offered.len().saturating_sub(fit));
            for (row, &i) in offered.iter().skip(self.scroll).take(fit).enumerate() {
                let r = Rect { x: list.x, y: list.y + row as f32 * lh, w: list.w, h: lh };
                let picked = self.picks[self.role].contains(&i);
                if picked { frame.paint_rect(r.x, r.y, r.w, r.h, theme.console_selection_bg); }
                let ink = if picked { theme.console_selection_fg } else { fg };
                text(frame, renderer, r.x + 6.0, r.y + lh * 0.8, &format!("{}  ({})", cols[i].name, cols[i].kind.label()), size, ink, cw, r.x + r.w);
                self.vars.push((r, i));
            }
        }

        // foot: the message, and the buttons
        let (fy, fh) = (b.y + b.h - foot + 6.0, lh + 6.0);
        let (cancel_w, run_w) = (10.0 * cw, 8.0 * cw);
        let cancel = Rect { x: b.x + b.w - cancel_w - 10.0, y: fy, w: cancel_w, h: fh };
        let run = Rect { x: cancel.x - run_w - 8.0, y: fy, w: run_w, h: fh };
        button(frame, renderer, theme, cancel, "Cancel", false);
        button(frame, renderer, theme, run, "Run", true);
        (self.cancel, self.run) = (Some(cancel), Some(run));
        text(frame, renderer, b.x + 10.0, fy + 3.0 + lh * 0.8, &self.message, size, theme.console_error, cw, run.x - 6.0);
    }
}

impl Gui {
    /// The dialog owns the input while it is open.
    pub(crate) fn sakha_dialog_input(&mut self, events: &[InputEvent]) {
        let mouse = self.mouse;
        let Some(d) = self.sakha.dialog.as_mut() else { return };
        match d.input(events, mouse) {
            Outcome::Stay => {}
            Outcome::Close => self.sakha.dialog = None,
            Outcome::Run => self.run_dialog(),
        }
    }

    fn run_dialog(&mut self) {
        let Some(df) = self.sakha.df.clone() else { return };
        let Some(d) = self.sakha.dialog.as_mut() else { return };
        match d.a.build(&df, &self.sakha.cols, &d.picks, &d.typed) {
            Ok(built) => {
                let a = d.a;
                self.sakha.dialog = None;
                self.run_analysis(a, built);
            }
            Err(m) => d.message = m,
        }
    }

    pub(crate) fn paint_sakha_dialog(&mut self, area: Rect, frame: &mut Frame, renderer: &mut Renderer, theme: &Theme) {
        let cols = &self.sakha.cols;
        if let Some(d) = self.sakha.dialog.as_mut() { d.paint(cols, area, frame, renderer, theme); }
    }
}
