//! Small painting helpers the panes share.

use r2_ui::{Color, Frame, Rect, Renderer, Theme};

/// `c` moved a fraction toward `toward` — for tints of the theme's colours.
pub(crate) fn mix(c: Color, toward: Color, t: f32) -> Color {
    let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    Color::rgb(m(c.0, toward.0), m(c.1, toward.1), m(c.2, toward.2))
}

pub(crate) fn inside(r: Rect, x: f32, y: f32) -> bool {
    x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h
}

/// A string of monospace glyphs from `x`, clipped at `max_x`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn text(frame: &mut Frame, renderer: &mut Renderer, x: f32, baseline: f32, s: &str,
                   size: f32, color: Color, cell_w: f32, max_x: f32) {
    for (k, ch) in s.chars().enumerate() {
        let gx = x + k as f32 * cell_w;
        if gx >= max_x { break; }
        if ch != ' ' { frame.paint_glyph_clipped(renderer, gx, baseline, ch, size, color, max_x); }
    }
}

/// A button whose right edge is at `right`, on a strip at `y`; returns its rect.
#[allow(clippy::too_many_arguments)]
pub(crate) fn button_left_of(frame: &mut Frame, renderer: &mut Renderer, theme: &Theme, right: f32, y: f32,
                             h: f32, label: &str, on: bool) -> Rect {
    let (cw, _) = renderer.cell_metrics(theme.fs());
    let w = (label.chars().count() + 2) as f32 * cw;
    let r = Rect { x: right - w, y, w, h };
    button(frame, renderer, theme, r, label, on);
    r
}

/// A button filling `r`, its label centred.
pub(crate) fn button(frame: &mut Frame, renderer: &mut Renderer, theme: &Theme, r: Rect, label: &str, on: bool) {
    let (cw, lh) = renderer.cell_metrics(theme.fs());
    let (bg, fg) = (theme.window_background, theme.console_output);
    let (fill, ink) = if on { (theme.console_selection_bg, theme.console_selection_fg) }
                      else { (mix(bg, fg, 0.14), theme.console_input) };
    frame.paint_rect(r.x, r.y, r.w, r.h, fill);
    let tx = r.x + (r.w - label.chars().count() as f32 * cw) / 2.0;
    let base = r.y + (r.h - lh) / 2.0 + lh * 0.8;
    text(frame, renderer, tx, base, label, theme.fs(), ink, cw, r.x + r.w);
}

/// A row of tabs from `x` on a strip of height `h`: returns each tab's rect.
#[allow(clippy::too_many_arguments)]
pub(crate) fn tabs(frame: &mut Frame, renderer: &mut Renderer, theme: &Theme, x: f32, y: f32, h: f32,
                   max_x: f32, labels: &[String], active: usize) -> Vec<Rect> {
    let (cw, lh) = renderer.cell_metrics(theme.fs());
    let (bg, fg) = (theme.window_background, theme.console_output);
    let mut out = Vec::with_capacity(labels.len());
    let mut at = x;
    for (i, label) in labels.iter().enumerate() {
        let w = ((label.chars().count() + 2) as f32 * cw).min((max_x - at).max(0.0));
        let r = Rect { x: at, y, w, h };
        if i == active {
            frame.paint_rect(r.x, r.y, r.w, r.h, bg);
            frame.paint_rect(r.x, r.y + r.h - 2.0, r.w, 2.0, theme.cursor);
        }
        let color = if i == active { fg } else { mix(fg, bg, 0.4) };
        text(frame, renderer, at + cw, y + (h - lh) / 2.0 + lh * 0.8, label, theme.fs(), color, cw, at + w);
        out.push(r);
        at += w + 4.0;
    }
    out
}

/// Word-wrap to `width` characters.
pub(crate) fn wrap(s: &str, width: usize) -> Vec<String> {
    let mut out = vec![String::new()];
    for word in s.split(' ') {
        let cur = out.last_mut().unwrap();
        if !cur.is_empty() && cur.chars().count() + 1 + word.chars().count() > width { out.push(word.to_string()); }
        else { if !cur.is_empty() { cur.push(' '); } cur.push_str(word); }
    }
    out
}

/// Today's date as YYYY-MM-DD (UTC), for the session script's header.
pub(crate) fn today() -> String {
    let days = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400).unwrap_or(0) as i64;
    // civil-from-days (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}
