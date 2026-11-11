//! Menu and context-menu commands. Action strings come from `menus.rs`; the
//! menu bars, right-click menus and the mode switch share this one dispatch.

use r2_ui::{FrameCtx, GridPos, Renderer, Selection, Theme};

use crate::support::{console_wrap_width, rows_from_buffer, wrap_rows};
use crate::Gui;

impl Gui {
    /// Would this action throw away unsaved work?
    fn would_discard(&self, action: &str) -> bool {
        let sakha = self.sakha.dirty;
        let scripts = self.source.unsaved() > 0;
        match action {
            "project.close" => sakha || scripts,
            a if a.starts_with("project.open:") || a.starts_with("project.new:") => sakha || scripts,
            "sakha.open_csv" | "doc.recover" => sakha,
            a if a.starts_with("doc.open:") => sakha,
            // the dataset already open costs nothing to choose again
            a if a.starts_with("sakha.df:") => sakha && self.sakha.df.as_deref() != Some(&a[9..]),
            a if a.starts_with("data.open:") || a.starts_with("data.builtin:") || a.starts_with("data.r2s:")
                || a.starts_with("data.importfile:") || a.starts_with("data.parquet:") => sakha && self.sakha_mode,
            _ => false,
        }
    }

    pub(crate) fn dispatch(&mut self, action: &str, ctx: &mut FrameCtx, renderer: &mut Renderer, theme: &Theme) {
        if !std::mem::take(&mut self.discard_ok) && self.would_discard(action) {
            self.ask_discard(action);
            return;
        }
        match action {
            "file.quit"            => self.quit_requested = true,
            "edit.settings"        => self.open_settings(),
            "file.clear"           => self.buffer.clear(),
            "file.save_plot"       => self.save_plot(theme, false),
            "file.save_plot_png"   => self.save_plot(theme, true),
            "file.copy_plot"       => self.copy_plot_svg(ctx),
            "file.copy_plot_image" => self.copy_plot_image(ctx, theme),
            "edit.copy"            => self.copy_selection(ctx, renderer, theme),
            "edit.paste"           => self.paste(ctx),
            "edit.select_all"      => self.select_all(renderer, theme),
            "session.interrupt"    => self.interrupt(),
            "win.all"              => self.show_all_panes(),
            "mode.sakha" => {
                self.set_mode(true);
                // data loaded in Pro: with only one choice, it is the one
                if self.sakha.df.is_none() {
                    let frames: Vec<String> = self.env.rows.iter().filter(|r| r.is_data_frame()).map(|r| r.name.clone()).collect();
                    if let [only] = frames.as_slice() { let only = only.clone(); self.open_sakha(&only, None); }
                }
            }
            "mode.studio"          => self.set_mode(false),
            "data.load" => self.open_load_chooser(),
            "data.select" => self.open_select_dialog(),
            "data.r2_list" => self.open_r2_datasets(),
            "data.attributes" => self.open_attributes(),
            a if a.starts_with("dataedit.") => self.data_edit_action(&a[9..]),
            "data.import" => {
                let picked = rfd::FileDialog::new().set_title("Import text file").set_directory(&self.session.cwd)
                    .add_filter("Text data", &["csv", "txt", "tsv", "dat"]).add_filter("All files", &["*"]).pick_file();
                if let Some(p) = picked { self.queued = Some(format!("data.importfile:{}", p.display())); }
            }
            "data.import_parquet" => {
                let picked = rfd::FileDialog::new().set_title("Import Parquet file").set_directory(&self.session.cwd)
                    .add_filter("Parquet", &[sakha_core::document::PARQUET_EXT]).pick_file();
                if let Some(p) = picked { self.queued = Some(format!("data.parquet:{}", p.display())); }
            }
            a if a.starts_with("data.importfile:") => self.open_import_dialog(std::path::Path::new(&a[16..])),
            a if a.starts_with("data.parquet:") => self.import_parquet(std::path::Path::new(&a[13..])),
            a if a.starts_with("data.view:") => self.open_data_tab(&a[10..]),
            a if a.starts_with("data.open:") => self.open_import_dialog(std::path::Path::new(&a[10..])),
            a if a.starts_with("data.builtin:") => self.load_builtin(&a[13..]),
            a if a.starts_with("data.r2s:") => self.load_r2_file(std::path::Path::new(&a[9..])),
            "data.load_r2s" => {
                let picked = rfd::FileDialog::new().set_title("Load R2 data file").set_directory(&self.session.cwd)
                    .add_filter("R2 data file", &["r2s"]).pick_file();
                if let Some(p) = picked { self.queued = Some(format!("data.r2s:{}", p.display())); }
            }
            "doc.save"             => self.save_document(false),
            "doc.save_as"          => self.save_document(true),
            "doc.recover"          => self.recover_document(),
            "doc.open" => {
                let picked = rfd::FileDialog::new().set_title("Open Sakha analysis").set_directory(&self.session.cwd)
                    .add_filter("Sakha analysis", &[sakha_core::document::SAKHA_EXT]).pick_file();
                if let Some(p) = picked { self.queued = Some(format!("doc.open:{}", p.display())); }
            }
            a if a.starts_with("doc.open:") => self.open_document(std::path::Path::new(&a[9..])),
            a if a.starts_with("doc.saveto:") => self.save_document_to(std::path::Path::new(&a[11..])),
            "project.new" | "project.open" => {
                // choose first, then ask about unsaved work for that choice
                let picked = if action == "project.new" {
                    rfd::FileDialog::new().set_title("Choose or create the project's folder").set_directory(&self.session.cwd).pick_folder()
                        .map(|f| format!("project.new:{}", f.display()))
                } else {
                    rfd::FileDialog::new().set_title("Open project").set_directory(&self.session.cwd)
                        .add_filter("Ardon Studio project", &[sakha_core::document::PROJECT_EXT]).pick_file()
                        .map(|f| format!("project.open:{}", f.display()))
                };
                if picked.is_some() { self.queued = picked; }
            }
            a if a.starts_with("project.") => self.project_action(a),
            a if a.starts_with("script.") => self.script_action(a, ctx),
            a if a.starts_with("sakha.") => self.sakha_action(a),
            a if a.starts_with("gview.") => self.sakha_action(a),
            a if a.starts_with("result.") => self.result_action(a, ctx),
            a if a.starts_with("tables.") => self.tables_action(a, ctx),
            "help.about" => {
                self.buffer.push_banner(&format!(
                    "Ardon Studio {}: one program with two faces on one Ardon-R2 session. AGPL-3.0.",
                    env!("CARGO_PKG_VERSION")));
                self.buffer.push_banner("Pro is for writing scripts; Sakha is for working from menus. R2 (r2 --ide) computes for both.");
            }
            _ => {}
        }
    }

    /// Pixel size of the plot as the Plots pane shows it (× DPI).
    fn plot_pixels(&self, theme: &Theme) -> (u32, u32) {
        self.mdi.window(self.plots_id).map(|w| {
            let r = w.content_rect(theme);
            (((r.w * theme.dpi).round() as u32).max(640), ((r.h * theme.dpi).round() as u32).max(480))
        }).unwrap_or((1024, 768))
    }

    /// Save the current plot: SVG or PNG by the chosen extension, or PNG only.
    fn save_plot(&mut self, theme: &Theme, png_only: bool) {
        let Some(svg) = self.plots.current().map(str::to_string) else {
            self.buffer.push_output("No plot to save.");
            return;
        };
        let px = self.plot_pixels(theme);
        let name = if png_only { "plot.png" } else { "plot.svg" };
        if let Some(msg) = self.save_svg(&svg, name, px, png_only) {
            if msg.starts_with("Save failed") { self.buffer.push_error(&msg) } else { self.buffer.push_output(&msg) }
        }
    }

    /// Ask where, then write `svg` as SVG or as a `px`-sized PNG, by the
    /// name's extension. Says what happened; nothing when cancelled.
    pub(crate) fn save_svg(&mut self, svg: &str, name: &str, px: (u32, u32), png_only: bool) -> Option<String> {
        let dialog = rfd::FileDialog::new().set_directory(&self.session.cwd);
        let dialog = if png_only {
            dialog.set_title("Save plot as PNG").set_file_name(name).add_filter("PNG image", &["png"])
        } else {
            dialog.set_title("Save plot").set_file_name(name)
                .add_filter("SVG vector", &["svg"]).add_filter("PNG image", &["png"])
        };
        let path = dialog.save_file()?;
        let png = png_only || path.extension().is_some_and(|e| e.eq_ignore_ascii_case("png"));
        let (w, h) = px;
        let result = if png {
            r2_ui::graph::rasterize_plot(svg.as_bytes(), w, h)
                .ok_or_else(|| "could not draw the plot".to_string())
                .and_then(|rgba| image::save_buffer(&path, &rgba, w, h, image::ExtendedColorType::Rgba8)
                    .map_err(|e| e.to_string()))
        } else {
            std::fs::write(&path, svg).map_err(|e| e.to_string())
        };
        Some(match result {
            Ok(()) if png => format!("Saved plot to {} ({w}×{h})", path.display()),
            Ok(()) => format!("Saved plot to {}", path.display()),
            Err(e) => format!("Save failed: {e}"),
        })
    }

    fn copy_plot_svg(&mut self, ctx: &mut FrameCtx) {
        match self.plots.current() {
            Some(svg) => {
                ctx.clipboard.set_text(svg);
                self.buffer.push_output("Plot SVG copied to clipboard.");
            }
            None => self.buffer.push_output("No plot to copy."),
        }
    }

    /// The plot as a bitmap on the clipboard, for Word, Excel and the like.
    fn copy_plot_image(&mut self, ctx: &mut FrameCtx, theme: &Theme) {
        let (w, h) = self.plot_pixels(theme);
        let Some(svg) = self.plots.current() else {
            self.buffer.push_output("No plot to copy.");
            return;
        };
        match r2_ui::graph::rasterize_plot(svg.as_bytes(), w, h) {
            Some(rgba) if ctx.clipboard.set_image(w, h, &rgba) =>
                self.buffer.push_output(&format!("Plot copied to clipboard as {w}×{h} image.")),
            Some(_) => self.buffer.push_error("Clipboard image copy failed (OS rejected)."),
            None => self.buffer.push_error("Could not draw the plot."),
        }
    }

    /// Copy the selection; rows include the live prompt row, as paint
    /// and selection index them.
    fn copy_selection(&self, ctx: &mut FrameCtx, renderer: &mut Renderer, theme: &Theme) {
        let rows = wrap_rows(self.rows_with_prompt(theme), self.console_wrap(renderer, theme));
        if let Some(sel) = self.grid.selection {
            let text = r2_ui::grid::selection_to_text(&rows, sel);
            if !text.is_empty() { ctx.clipboard.set_text(&text); }
        }
    }

    /// Paste as Ctrl+V does: the first chunk completes the line being
    /// typed, each further full line submits, the last stays at the prompt.
    fn paste(&mut self, ctx: &mut FrameCtx) {
        let Some(s) = ctx.clipboard.get_text() else { return };
        let s = s.replace('\r', "");
        if !s.contains('\n') {
            let pos = self.input.cursor;
            self.input.current.insert_str(pos, &s);
            self.input.cursor = pos + s.len();
            return;
        }
        let mut parts: Vec<String> = s.split('\n').map(String::from).collect();
        let head = parts.remove(0);
        let tail = parts.pop().unwrap_or_default();
        let pos = self.input.cursor;
        self.input.current.insert_str(pos, &head);
        let first = std::mem::take(&mut self.input.current);
        for line in std::iter::once(first).chain(parts) {
            self.submit(line);
        }
        self.input.current = tail;
        self.input.cursor = self.input.current.len();
    }

    fn select_all(&mut self, renderer: &mut Renderer, theme: &Theme) {
        let (cell_w, _) = renderer.cell_metrics(theme.fs());
        let ww = self.mdi.window(self.console_id)
            .map(|w| console_wrap_width(w.content_rect(theme).w, cell_w))
            .unwrap_or(0);
        let rows = wrap_rows(rows_from_buffer(&self.buffer, theme), ww);
        if !rows.is_empty() {
            let last = rows.len() - 1;
            self.grid.selection = Some(Selection {
                start: GridPos { row: 0, col: 0 },
                end:   GridPos { row: last, col: rows[last].len().saturating_sub(1) },
            });
        }
    }
}
