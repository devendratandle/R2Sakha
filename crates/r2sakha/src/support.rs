//! Free helpers: console lines → painted cells, row wrapping, and the
//! default working folder.

use r2_ui::{Cell, Color, Theme};

use crate::console::{ConsoleBuffer, LineKind};

pub(crate) fn line_color(theme: &Theme, kind: LineKind) -> Color {
    match kind {
        LineKind::Input | LineKind::Continuation => theme.console_input,
        LineKind::Output                          => theme.console_output,
        LineKind::Error                           => theme.console_error,
        LineKind::Banner                          => theme.console_banner,
    }
}

pub(crate) fn rows_from_buffer(buf: &ConsoleBuffer, theme: &Theme) -> Vec<Vec<Cell>> {
    buf.transcript().iter()
        .map(|cl| {
            let col = line_color(theme, cl.kind);
            cl.text.chars().map(|c| Cell::plain(c, col)).collect()
        })
        .collect()
}

/// Fold rows longer than `width` cells onto following rows. Char-level,
/// as the console is monospace and much of R output is aligned tables.
/// `width == 0` (not measurable yet) leaves the rows alone.
pub(crate) fn wrap_rows(rows: Vec<Vec<Cell>>, width: usize) -> Vec<Vec<Cell>> {
    if width == 0 { return rows; }
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        if row.len() <= width {
            out.push(row);
        } else {
            out.extend(row.chunks(width).map(|c| c.to_vec()));
        }
    }
    out
}

/// The console's usable width in cells: the content width less the 8 px
/// margins and the scrollbar, as paint and selection lay it out.
pub(crate) fn console_wrap_width(content_w: f32, cell_w: f32) -> usize {
    if cell_w <= 0.0 { return 0; }
    let grid_w = content_w - 16.0 - r2_ui::SCROLLBAR_THICKNESS;
    if grid_w <= 0.0 { return 0; }
    (grid_w / cell_w).floor() as usize
}

/// A writable default working folder, as R and Ardon-R2 pick it:
/// R2_HOME, else Documents (OneDrive's first), else HOME.
pub(crate) fn pick_user_home() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    if let Ok(custom) = std::env::var("R2_HOME") {
        let p = PathBuf::from(custom);
        if p.is_dir() { return Some(p); }
    }
    if let Ok(od) = std::env::var("OneDrive") {
        let p = PathBuf::from(&od).join("Documents");
        if p.is_dir() { return Some(p); }
    }
    if let Ok(user) = std::env::var("USERPROFILE") {
        let od = PathBuf::from(&user).join("OneDrive").join("Documents");
        if od.is_dir() { return Some(od); }
        let docs = PathBuf::from(user).join("Documents");
        if docs.is_dir() { return Some(docs); }
    }
    if let Ok(home) = std::env::var("HOME") {
        let docs = PathBuf::from(&home).join("Documents");
        if docs.is_dir() { return Some(docs); }
        let h = PathBuf::from(home);
        if h.is_dir() { return Some(h); }
    }
    None
}
