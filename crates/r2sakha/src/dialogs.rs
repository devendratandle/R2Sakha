//! The two modal dialogs: quit ("Save workspace image?") and Settings
//! (UI font size). Handled and painted last in a frame, so they float
//! above everything and own the keyboard and mouse.

use std::time::Duration;

use r2_ui::{DialogButton, Frame, FrameCtx, Renderer, Theme};
use sakha_engine::json;

use crate::Gui;

impl Gui {
    /// q()/quit(), File ▸ Quit and the window's ✕ all ask first, as R does.
    pub(crate) fn open_quit_dialog_on_request(&mut self, close_requested: bool) {
        let quit_flag = std::mem::take(&mut self.quit_requested);
        if !(quit_flag || close_requested) || self.quit_dialog.is_open() || self.settings_dialog.is_open() {
            return;
        }
        let unsaved = self.source.unsaved();
        let sakha_dirty = self.sakha.dirty;
        let d = &mut self.quit_dialog;
        d.title = "Quit Ardon Studio".into();
        d.lines = vec!["Save workspace image?".into()];
        if unsaved > 0 {
            d.lines.push(format!("{unsaved} script{} not saved and will be lost.",
                                 if unsaved == 1 { " is" } else { "s are" }));
        }
        if sakha_dirty { d.lines.push("The Sakha analysis is not saved and will be lost.".into()); }
        d.buttons = vec![
            DialogButton::new("Yes",    "quit.yes"),
            DialogButton::new("No",     "quit.no"),
            DialogButton::new("Cancel", "quit.cancel"),
        ];
        d.default_action = "quit.yes".into();
        d.cancel_action  = "quit.cancel".into();
        d.open();
    }

    /// Unsaved work stands in the way of `action`: ask first.
    pub(crate) fn ask_discard(&mut self, action: &str) {
        let (scripts, sakha) = (self.source.unsaved(), self.sakha.dirty);
        self.pending_discard = Some(action.to_string());
        let d = &mut self.confirm_dialog;
        d.title = "Unsaved work".into();
        d.lines = vec![];
        if scripts > 0 { d.lines.push(format!("{scripts} script{} not saved.", if scripts == 1 { " is" } else { "s are" })); }
        if sakha { d.lines.push("The Sakha analysis is not saved.".into()); }
        d.lines.push("Continue and lose it?".into());
        d.buttons = vec![DialogButton::new("Discard", "confirm.discard"), DialogButton::new("Cancel", "confirm.cancel")];
        d.default_action = "confirm.cancel".into();
        d.cancel_action = "confirm.cancel".into();
        d.open();
    }

    /// The app is closing: keep the project's state; nothing unsaved is wanted now.
    fn leaving(&mut self) {
        self.save_project();
        self.remove_backup();
    }

    pub(crate) fn open_settings(&mut self) {
        let d = &mut self.settings_dialog;
        d.title = "Settings".into();
        d.buttons = vec![
            DialogButton::new("A \u{2013}", "settings.font_dec"),
            DialogButton::new("A +",        "settings.font_inc"),
            DialogButton::new("Reset",      "settings.font_reset"),
            DialogButton::new("Close",      "settings.close"),
        ];
        d.default_action = String::new();
        d.cancel_action  = "settings.close".into();
        d.open();
    }

    pub(crate) fn run_dialogs(&mut self, ctx: &mut FrameCtx, frame: &mut Frame,
                              renderer: &mut Renderer, theme: &Theme, win_w: f32, win_h: f32) {
        if self.settings_dialog.is_open() {
            self.settings_dialog.lines = vec![
                format!("UI font size:  {} pt", theme.font_size_base() as i32),
                String::new(),
                "Resizes the console + window text.".into(),
                "(DPI scaling applies on top automatically.)".into(),
            ];
            let action = self.settings_dialog.handle_events(ctx.events, renderer, theme, win_w, win_h);
            match action.as_deref() {
                Some("settings.font_dec")   => ctx.set_base_font_size(theme.font_size_base() - 1.0),
                Some("settings.font_inc")   => ctx.set_base_font_size(theme.font_size_base() + 1.0),
                Some("settings.font_reset") => ctx.set_base_font_size(14.0),
                Some("settings.close")      => self.settings_dialog.close(),
                _ => {}
            }
            self.settings_dialog.paint(frame, renderer, theme, win_w, win_h);
        }

        if self.quit_dialog.is_open() {
            let action = self.quit_dialog.handle_events(ctx.events, renderer, theme, win_w, win_h);
            match action.as_deref() {
                Some("quit.yes") => {
                    // Ardon-R2 saves a session (every variable) as .r2s,
                    // where R writes .RData.
                    if let Some(e) = self.session.engine.as_mut() {
                        let _ = e.call("eval", json!({ "code": "save(\".r2session.r2s\")" }),
                                       Duration::from_secs(30));
                    }
                    self.leaving();
                    ctx.request_exit();
                }
                Some("quit.no") => {
                    self.leaving();
                    ctx.request_exit();
                }
                Some("quit.cancel") => self.quit_dialog.close(),
                _ => {}
            }
            self.quit_dialog.paint(frame, renderer, theme, win_w, win_h);
        }

        if self.confirm_dialog.is_open() {
            let action = self.confirm_dialog.handle_events(ctx.events, renderer, theme, win_w, win_h);
            self.confirm_dialog.paint(frame, renderer, theme, win_w, win_h);
            match action.as_deref() {
                Some("confirm.discard") => {
                    self.confirm_dialog.close();
                    self.discard_ok = true;
                    if let Some(a) = self.pending_discard.take() { self.dispatch(&a, ctx, renderer, theme); }
                }
                Some("confirm.cancel") => {
                    self.confirm_dialog.close();
                    self.pending_discard = None;
                }
                _ => {}
            }
        }
    }
}
