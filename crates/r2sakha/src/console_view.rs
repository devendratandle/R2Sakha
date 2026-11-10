//! The console window: keyboard input at the prompt, drag-selection, and
//! painting the transcript with its scrollbars and cursor.
//!
//! Selection, copy and paint must agree on ONE row structure — the
//! transcript plus the live prompt row, wrapped to the console width — or
//! a (row, col) picked by the mouse copies the wrong text. The helpers
//! here (`rows_with_prompt`, `transcript_rect`, `console_wrap`) are that
//! agreement.

use r2_ui::{auto_scroll_offset, Cell, Frame, FrameCtx, InputEvent, InputField,
            Rect, Renderer, Theme, SCROLLBAR_THICKNESS};

use crate::console::SubmitAction;
use crate::support::{console_wrap_width, rows_from_buffer, wrap_rows};
use crate::Gui;

/// "<prompt> <typed text>" in the console-input colour.
fn prompt_row(input: &InputField, theme: &Theme) -> Vec<Cell> {
    let full = format!("{} {}", input.prompt, input.current);
    full.chars().map(|c| Cell::plain(c, theme.console_input)).collect()
}

/// Where the transcript grid sits in the console's content rect: an 8 px
/// margin, minus the scrollbar strips on the right and bottom.
fn transcript_rect(content: Rect) -> Rect {
    Rect {
        x: content.x + 8.0,
        y: content.y + 8.0,
        w: content.w - 16.0 - SCROLLBAR_THICKNESS,
        h: content.h - 16.0 - SCROLLBAR_THICKNESS,
    }
}

impl Gui {
    /// The transcript plus the live prompt row, unwrapped.
    pub(crate) fn rows_with_prompt(&self, theme: &Theme) -> Vec<Vec<Cell>> {
        let mut rows = rows_from_buffer(&self.buffer, theme);
        rows.push(prompt_row(&self.input, theme));
        rows
    }

    /// The console's wrap width in cells (0 = don't wrap).
    pub(crate) fn console_wrap(&self, renderer: &mut Renderer, theme: &Theme) -> usize {
        let (cell_w, _) = renderer.cell_metrics(theme.fs());
        self.mdi.window(self.console_id)
            .map(|w| console_wrap_width(w.content_rect(theme).w, cell_w))
            .unwrap_or(0)
    }

    /// One line as if typed and Enter-pressed. Lines wait their turn, so each
    /// one's output lands under it in the transcript.
    pub(crate) fn submit(&mut self, line: String) { self.run_queue.push_back((line, None)); }

    /// The next waiting lines, as far as the next statement R2 must answer.
    pub(crate) fn pump_run_queue(&mut self) {
        while !self.session.console_running() {
            let Some((line, after)) = self.run_queue.pop_front() else { return };
            if let SubmitAction::Submit(src) = self.buffer.submit_line(line) {
                // a line that is only a comment needs no trip to R2
                if src.trim_start().starts_with('#') && !src.contains('\n') { continue; }
                self.next_after = after;
                self.run_source(src);
            }
            if self.quit_requested { self.run_queue.clear(); }
        }
    }

    /// Console keyboard input — always active, so the console stays
    /// typeable whichever window is in front (as in RGui).
    pub(crate) fn console_input(&mut self, ui_events: &[InputEvent], ctx: &mut FrameCtx,
                                renderer: &mut Renderer, theme: &Theme, ctx_was_open: bool) {
        let topmost = self.mdi.z_order().last();
        let resp = self.input.handle_events(ui_events, ctx.clipboard);

        for line in resp.auto_submit_lines {
            self.submit(line);
        }
        if let Some(line) = resp.submitted {
            self.submit(line);
        }
        // Esc at the prompt drops a pending `+` (while code runs, the frame
        // has already turned it into an interrupt).
        if resp.escaped && !self.session.busy() { self.buffer.cancel_continuation(); }
        if resp.history_up {
            if let Some(s) = self.buffer.history_up() { self.input.set_line(s); }
        }
        if resp.history_down {
            if let Some(s) = self.buffer.history_down() { self.input.set_line(s); }
        }
        self.input.set_prompt(self.buffer.current_prompt());

        // Drag-select / Ctrl+A / Ctrl+C only while the console is in front.
        if topmost != Some(self.console_id) { return; }
        let rows = self.rows_with_prompt(theme);
        let (cell_w, line_h) = renderer.cell_metrics(theme.fs());
        let Some(content) = self.mdi.window(self.console_id).map(|w| w.content_rect(theme))
            else { return };
        let rows = wrap_rows(rows, console_wrap_width(content.w, cell_w));
        if !ctx_was_open {
            // the wheel belongs to the pane under the pointer (console_wheel)
            let events: Vec<InputEvent> = ui_events.iter().filter(|e| !matches!(e, InputEvent::Scroll { .. })).copied().collect();
            let _copied = self.grid.handle_events(
                &events, &rows, transcript_rect(content), cell_w, line_h, ctx.clipboard);
        }
    }

    /// The wheel over the console scrolls it, whichever pane is in front.
    pub(crate) fn console_wheel(&mut self, events: &[InputEvent], renderer: &mut Renderer, theme: &Theme) {
        let Some(content) = self.mdi.window(self.console_id).filter(|w| w.visible).map(|w| w.content_rect(theme)) else { return };
        if !crate::ui::inside(content, self.mouse.0, self.mouse.1) { return; }
        let wheel: Vec<InputEvent> = events.iter().filter(|e| matches!(e, InputEvent::Scroll { .. })).copied().collect();
        if wheel.is_empty() { return; }
        let (cell_w, line_h) = renderer.cell_metrics(theme.fs());
        let rows = wrap_rows(self.rows_with_prompt(theme), console_wrap_width(content.w, cell_w));
        let mut clip = r2_ui::Clipboard::new();
        let _ = self.grid.handle_events(&wheel, &rows, transcript_rect(content), cell_w, line_h, &mut clip);
    }

    pub(crate) fn paint_console(&mut self, content: Rect, ui_events: &[InputEvent],
                                frame: &mut Frame, renderer: &mut Renderer, theme: &Theme) {
        let (cell_w, line_h) = renderer.cell_metrics(theme.fs());
        frame.paint_rect(content.x, content.y, content.w, content.h, theme.window_background);

        let grid_rect = transcript_rect(content);
        let sbt = SCROLLBAR_THICKNESS;
        let vtrack = Rect { x: grid_rect.x + grid_rect.w, y: grid_rect.y, w: sbt, h: grid_rect.h };
        let htrack = Rect { x: grid_rect.x, y: grid_rect.y + grid_rect.h, w: grid_rect.w, h: sbt };

        // Wrap the transcript, then the prompt separately, to map the
        // cursor into the wrapped grid.
        let cursor_col = self.input.prompt.chars().count() + 1
            + self.input.current[..self.input.cursor].chars().count();
        let wrap = (grid_rect.w / cell_w).floor() as usize;
        let mut rows = wrap_rows(rows_from_buffer(&self.buffer, theme), wrap);
        let prompt_base = rows.len();
        let (prompt_row_index, cursor_col_in_row) = if wrap > 0 {
            (prompt_base + cursor_col / wrap, cursor_col % wrap)
        } else {
            (prompt_base, cursor_col)
        };
        rows.extend(wrap_rows(vec![prompt_row(&self.input, theme)], wrap));

        let view = View {
            grid_rect, vtrack, htrack, line_h,
            total_rows: rows.len(),
            max_cols: rows.iter().map(|r| r.len()).max().unwrap_or(0).max(cursor_col_in_row + 1),
            visible_rows: (grid_rect.h / line_h).floor() as usize,
            visible_cols: (grid_rect.w / cell_w).floor() as usize,
        };
        self.drive_scrollbars(ui_events, &view, cursor_col_in_row);

        self.grid.paint(frame, renderer, &rows, grid_rect, cell_w, line_h, theme.fs(), theme);

        let scroll = self.grid.scroll_y_override
            .unwrap_or_else(|| auto_scroll_offset(rows.len(), grid_rect.h, line_h));
        let scroll_x = self.grid.scroll_x;
        let cursor_on = (self.frame_counter / 30).is_multiple_of(2);
        if cursor_on && prompt_row_index >= scroll && cursor_col_in_row >= scroll_x {
            let cx = grid_rect.x + (cursor_col_in_row - scroll_x) as f32 * cell_w;
            let cy = grid_rect.y + (prompt_row_index - scroll) as f32 * line_h;
            if cy + line_h <= grid_rect.y + grid_rect.h && cx + 2.0 <= grid_rect.x + grid_rect.w {
                frame.paint_rect(cx, cy + line_h * 0.1, 2.0, line_h * 0.8, theme.cursor);
            }
        }

        self.vscroll.paint(frame, vtrack, theme);
        self.hscroll.paint(frame, htrack, theme);
    }

    /// Scroll state from content vs viewport, scrollbar drags, and the
    /// typing cursor.
    fn drive_scrollbars(&mut self, ui_events: &[InputEvent], v: &View, cursor_col_in_row: usize) {
        // New output snaps back to the prompt, as the R console does.
        if self.last_total_rows != v.total_rows {
            self.last_total_rows = v.total_rows;
            self.grid.scroll_y_override = None;
            self.grid.scroll_x = 0;
        }
        if v.total_rows > 0 {
            self.vscroll.visible_fraction = (v.visible_rows as f32 / v.total_rows as f32).min(1.0);
        }
        if v.max_cols > 0 {
            self.hscroll.visible_fraction = (v.visible_cols as f32 / v.max_cols as f32).min(1.0);
        }
        if let Some(p) = self.vscroll.handle_events(ui_events, v.vtrack) {
            let off = r2_ui::scroll_pos_to_row(p, v.total_rows, v.visible_rows);
            self.grid.scroll_y_override =
                if off + v.visible_rows >= v.total_rows { None } else { Some(off) };
        }
        if let Some(p) = self.hscroll.handle_events(ui_events, v.htrack) {
            self.grid.scroll_x = r2_ui::scroll_pos_to_col(p, v.max_cols, v.visible_cols);
        }

        // Keep the typing cursor in view, but only when the user typed —
        // a manual scroll must not snap back every frame.
        let typed = self.last_input != self.input.current;
        if typed { self.last_input = self.input.current.clone(); }
        if typed && v.visible_cols > 0 {
            let gs = &mut self.grid;
            if cursor_col_in_row < v.visible_cols {
                gs.scroll_x = 0;
            } else if cursor_col_in_row >= gs.scroll_x + v.visible_cols {
                gs.scroll_x = cursor_col_in_row + 1 - v.visible_cols;
            } else if cursor_col_in_row < gs.scroll_x {
                gs.scroll_x = cursor_col_in_row;
            }
        }

        let eff_y = self.grid.scroll_y_override
            .unwrap_or_else(|| auto_scroll_offset(v.total_rows, v.grid_rect.h, v.line_h));
        self.vscroll.position = r2_ui::row_offset_to_scroll_pos(eff_y, v.total_rows, v.visible_rows);
        self.hscroll.position = r2_ui::col_offset_to_scroll_pos(self.grid.scroll_x, v.max_cols, v.visible_cols);
    }
}

/// The console transcript's geometry for one frame.
struct View {
    grid_rect: Rect,
    vtrack: Rect,
    htrack: Rect,
    line_h: f32,
    total_rows: usize,
    max_cols: usize,
    visible_rows: usize,
    visible_cols: usize,
}
