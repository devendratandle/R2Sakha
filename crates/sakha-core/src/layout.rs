//! The four-pane studio layout, as RStudio arranges it:
//!
//! ```text
//!   ┌──────────────┬──────────────┐
//!   │ Source       │ Environment  │
//!   ├──────────────┼──────────────┤
//!   │ Console      │ Plots        │
//!   └──────────────┴──────────────┘
//! ```
//!
//! Three dividers, each a fraction of the workspace so the panes keep
//! their proportions when the window resizes: one vertical, and one
//! horizontal per column. Dragging a divider moves its fraction; every
//! pane keeps a minimum size so none can be dragged shut.

/// An axis-aligned rectangle in pixels (kept local so this crate needs no
/// GUI dependency; the app converts it to its own `Rect`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect { pub x: f32, pub y: f32, pub w: f32, pub h: f32 }

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

/// Which pane, in reading order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane { Source, Console, Environment, Plots }

/// A draggable divider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Divider { Vertical, Left, Right }

/// Gap between panes, px — wide enough to grab as a divider.
pub const GAP: f32 = 6.0;
/// No pane gets narrower or shorter than this, px.
pub const MIN_PANE: f32 = 120.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Studio {
    /// Left column's share of the width.
    pub split_x: f32,
    /// Source's share of the left column's height.
    pub split_left: f32,
    /// Environment's share of the right column's height.
    pub split_right: f32,
}

impl Default for Studio {
    fn default() -> Self { Studio { split_x: 0.55, split_left: 0.5, split_right: 0.45 } }
}

impl Studio {
    /// Each pane's rectangle inside `ws` (the MDI workspace).
    pub fn rect(&self, ws: Rect, pane: Pane) -> Rect {
        let g = GAP;
        let lw = ((ws.w - 3.0 * g) * self.split_x).round();
        let rw = ws.w - 3.0 * g - lw;
        let hh = ws.h - 3.0 * g;
        let (x0, x1) = (ws.x + g, ws.x + 2.0 * g + lw);
        let col = |x: f32, w: f32, frac: f32, top: bool| {
            let th = (hh * frac).round();
            if top { Rect { x, y: ws.y + g, w, h: th } }
            else { Rect { x, y: ws.y + 2.0 * g + th, w, h: hh - th } }
        };
        match pane {
            Pane::Source => col(x0, lw, self.split_left, true),
            Pane::Console => col(x0, lw, self.split_left, false),
            Pane::Environment => col(x1, rw, self.split_right, true),
            Pane::Plots => col(x1, rw, self.split_right, false),
        }
    }

    /// The divider under a point, if any (the gaps between panes).
    pub fn divider_at(&self, ws: Rect, x: f32, y: f32) -> Option<Divider> {
        let (s, c, e) = (self.rect(ws, Pane::Source), self.rect(ws, Pane::Console), self.rect(ws, Pane::Environment));
        let vgap = Rect { x: s.x + s.w, y: ws.y, w: e.x - (s.x + s.w), h: ws.h };
        let lgap = Rect { x: s.x, y: s.y + s.h, w: s.w, h: c.y - (s.y + s.h) };
        let p = self.rect(ws, Pane::Plots);
        let rgap = Rect { x: e.x, y: e.y + e.h, w: e.w, h: p.y - (e.y + e.h) };
        if vgap.contains(x, y) { Some(Divider::Vertical) }
        else if lgap.contains(x, y) { Some(Divider::Left) }
        else if rgap.contains(x, y) { Some(Divider::Right) }
        else { None }
    }

    /// Move `d` to the pointer, keeping every pane at least `MIN_PANE`.
    pub fn drag(&mut self, ws: Rect, d: Divider, x: f32, y: f32) {
        let clamp = |v: f32, total: f32| {
            let lo = (MIN_PANE / total).min(0.5);
            v.clamp(lo, 1.0 - lo)
        };
        let w = ws.w - 3.0 * GAP;
        let h = ws.h - 3.0 * GAP;
        if w <= 0.0 || h <= 0.0 { return; }
        match d {
            Divider::Vertical => self.split_x = clamp((x - ws.x - 1.5 * GAP) / w, w),
            Divider::Left => self.split_left = clamp((y - ws.y - 1.5 * GAP) / h, h),
            Divider::Right => self.split_right = clamp((y - ws.y - 1.5 * GAP) / h, h),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WS: Rect = Rect { x: 0.0, y: 30.0, w: 1206.0, h: 806.0 };

    #[test]
    fn panes_tile_the_workspace_without_overlap() {
        let s = Studio::default();
        let (a, b, c, d) = (s.rect(WS, Pane::Source), s.rect(WS, Pane::Console),
                            s.rect(WS, Pane::Environment), s.rect(WS, Pane::Plots));
        assert_eq!(a.x, b.x);
        assert_eq!(a.w, b.w);
        assert_eq!(c.x, d.x);
        assert!((a.x + a.w + GAP - c.x).abs() < 1e-3, "one gap between the columns");
        assert!((a.y + a.h + GAP - b.y).abs() < 1e-3, "one gap between source and console");
        assert!((d.x + d.w + GAP - (WS.x + WS.w)).abs() < 1e-3, "right margin is one gap");
        assert!((b.y + b.h + GAP - (WS.y + WS.h)).abs() < 1e-3, "bottom margin is one gap");
        assert!((a.w / (a.w + c.w) - 0.55).abs() < 0.01);
    }

    #[test]
    fn dividers_are_found_in_the_gaps_and_drag_with_limits() {
        let mut s = Studio::default();
        let src = s.rect(WS, Pane::Source);
        assert_eq!(s.divider_at(WS, src.x + src.w + GAP / 2.0, 200.0), Some(Divider::Vertical));
        assert_eq!(s.divider_at(WS, src.x + 10.0, src.y + src.h + GAP / 2.0), Some(Divider::Left));
        assert_eq!(s.divider_at(WS, src.x + 10.0, src.y + 10.0), None);
        s.drag(WS, Divider::Vertical, 300.0, 0.0);
        let src2 = s.rect(WS, Pane::Source);
        assert!((src2.x + src2.w + GAP / 2.0 - 300.0).abs() < 2.0, "divider follows the pointer");
        s.drag(WS, Divider::Vertical, -500.0, 0.0);
        assert!(s.rect(WS, Pane::Source).w >= MIN_PANE - 1.0, "a pane cannot be dragged shut");
        s.drag(WS, Divider::Right, 0.0, 10_000.0);
        assert!(s.rect(WS, Pane::Plots).h >= MIN_PANE - 1.0);
    }
}
