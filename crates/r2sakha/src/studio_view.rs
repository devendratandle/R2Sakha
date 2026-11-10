//! Pro layout and the switch between the app's two faces.
//!
//! Source top-left, Console bottom-left, Environment top-right, Plots
//! bottom-right — RStudio's arrangement — with the gaps between them as
//! draggable dividers (`sakha_core::layout` has the math). Panes stay on their
//! tiles; a pane's maximize button is the one way to give it the workspace.
//!
//! Ardon Studio ships one program with two complementary faces on one R2 session:
//! the **Pro** for people who write scripts, **Sakha** for people who work
//! from menus. The switch in the menu bar changes face in one click, and
//! each keeps its state while the other is in front.

use r2_ui::{CursorShape, FrameCtx, InputEvent, MouseButton, Rect, Renderer, Theme, WindowId};
use sakha_core::layout::{Pane, Rect as SRect};

use crate::ui::{button, inside};
use crate::Gui;

/// Where the switch's middle sits along the menu bar.
const GOLDEN: f32 = 0.618;

fn to_s(r: Rect) -> SRect { SRect { x: r.x, y: r.y, w: r.w, h: r.h } }
fn from_s(r: SRect) -> Rect { Rect { x: r.x, y: r.y, w: r.w, h: r.h } }

impl Gui {
    fn panes(&self) -> [(WindowId, Pane); 4] {
        [(self.source_id, Pane::Source), (self.console_id, Pane::Console),
         (self.env_id, Pane::Environment), (self.plots_id, Pane::Plots)]
    }

    /// Put each pane on its tile, every frame.
    pub(crate) fn apply_studio(&mut self) {
        let ws = self.workspace;
        for (id, pane) in self.panes() {
            let tile = from_s(self.studio.rect(to_s(ws), pane));
            if let Some(w) = self.mdi.window_mut(id) { w.bounds = if w.maximized { ws } else { tile }; }
        }
    }

    pub(crate) fn show_window(&mut self, id: WindowId) {
        if let Some(w) = self.mdi.window_mut(id) { w.visible = true; }
    }

    pub(crate) fn show_all_panes(&mut self) {
        for (id, _) in self.panes() { self.show_window(id); }
    }

    /// Divider hover and drag. Returns true while one is dragged, so the
    /// press stays away from the panes.
    pub(crate) fn studio_dividers(&mut self, events: &[InputEvent], ctx: &mut FrameCtx) -> bool {
        let ws = to_s(self.workspace);
        for ev in events {
            match *ev {
                InputEvent::MouseDown { button: MouseButton::Left, pos } => {
                    self.divider_drag = self.studio.divider_at(ws, pos.x, pos.y);
                }
                InputEvent::MouseMoved(pos) => {
                    if let Some(d) = self.divider_drag { self.studio.drag(ws, d, pos.x, pos.y); }
                    self.divider_hover = self.studio.divider_at(ws, pos.x, pos.y);
                }
                InputEvent::MouseUp { button: MouseButton::Left, .. } => self.divider_drag = None,
                _ => {}
            }
        }
        if let Some(d) = self.divider_drag.or(self.divider_hover) {
            use sakha_core::layout::Divider;
            ctx.set_cursor(if d == Divider::Vertical { CursorShape::ResizeH } else { CursorShape::ResizeV });
        }
        self.divider_drag.is_some()
    }

    pub(crate) fn set_mode(&mut self, sakha: bool) {
        // the enlarged plot borrows the Plots pane's panel
        if !sakha { self.close_enlarged(); }
        self.sakha_mode = sakha;
        self.divider_drag = None;
        self.divider_hover = None;
    }

    /// The menu bar's right-hand furniture. The switch's middle sits at the
    /// golden ratio of the width, clear of the menus; the project button takes
    /// the right end.
    fn bar_rects(&mut self, bar: Rect, renderer: &mut Renderer, theme: &Theme) -> ([(Rect, bool); 2], Rect) {
        let (cw, _) = renderer.cell_metrics(theme.fs());
        let (y, h) = (bar.y + 1.0, bar.h - 2.0);
        let pw = (self.project_label().chars().count() + 2) as f32 * cw;
        let project = Rect { x: bar.x + bar.w - pw - cw, y, w: pw, h };
        let menus: usize = if self.sakha_mode { self.menu_sakha.bar.items.iter() } else { self.menu_console.bar.items.iter() }
            .map(|t| t.label.chars().count() + 2).sum();
        let sw = 9.0 * cw;
        let x = (bar.x + bar.w * GOLDEN - sw).max(bar.x + (menus + 3) as f32 * cw).min(project.x - 2.0 * sw - cw);
        ([(Rect { x, y, w: sw, h }, false), (Rect { x: x + sw, y, w: sw, h }, true)], project)
    }

    /// A press on the switch or the project button: returns the action to dispatch.
    pub(crate) fn top_bar_input(&mut self, events: &[InputEvent], bar: Rect, renderer: &mut Renderer, theme: &Theme) -> Option<&'static str> {
        let (rects, project) = self.bar_rects(bar, renderer, theme);
        for ev in events {
            if let InputEvent::MouseDown { button: MouseButton::Left, pos } = *ev {
                if inside(project, pos.x, pos.y) {
                    if self.project_menu.is_open() { self.project_menu.close(); } else { self.open_project_menu(project); }
                    return None;
                }
                if let Some((_, sakha)) = rects.iter().find(|(r, _)| inside(*r, pos.x, pos.y)) {
                    return Some(if *sakha { "mode.sakha" } else { "mode.studio" });
                }
            }
        }
        None
    }

    /// The switch, the project button, and between them what R2 is doing.
    pub(crate) fn paint_top_bar(&mut self, frame: &mut r2_ui::Frame, bar: Rect, renderer: &mut Renderer, theme: &Theme) {
        let (cw, lh) = renderer.cell_metrics(theme.fs());
        let (rects, project) = self.bar_rects(bar, renderer, theme);
        for (r, sakha) in rects {
            button(frame, renderer, theme, r, if sakha { "Sakha" } else { "Pro" }, sakha == self.sakha_mode);
        }
        let label = self.project_label();
        button(frame, renderer, theme, project, &label, self.project_menu.is_open());
        let status = match (&self.session.engine, self.session.busy()) {
            (None, _) => "no R2 engine".to_string(),
            (_, true) => "R2 is running… (Esc stops it)".to_string(),
            _ => format!("R2 {}", self.session.version),
        };
        let w = (status.chars().count() + 2) as f32 * cw;
        let x = project.x - w;
        // only when it fits between the switch and the button
        if x >= rects[1].0.x + rects[1].0.w {
            crate::ui::text(frame, renderer, x, bar.y + (bar.h - lh) / 2.0 + lh * 0.8, &status, theme.fs(), theme.menu_text, cw, project.x);
        }
    }
}
