//! One frame, in order: take in R2's output → menus and commands → input for
//! the face in front → paint → popups → modal dialogs. Anything painted later
//! floats above what came before, and a modal owns the input.

use r2_ui::{menu_bar_height, Frame, FrameCtx, InputEvent, KeyCode, MenuBarState, MouseButton, Rect, Renderer, Theme, WindowId};

use crate::Gui;

/// Which menu bar is showing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Bar { Console, Script, Graphics, Sakha }

impl Gui {
    pub(crate) fn frame(&mut self, ctx: &mut FrameCtx, renderer: &mut Renderer,
                        frame: &mut Frame, theme: &Theme) {
        self.frame_counter += 1;
        self.upload_logo(renderer);
        for ev in ctx.events {
            if let InputEvent::MouseMoved(p) = ev { self.mouse = (p.x, p.y); }
        }
        let win_w = renderer.size.width as f32;
        let win_h = renderer.size.height as f32;
        let menu_h = menu_bar_height(theme);
        let menu_rect = Rect { x: 0.0, y: 0.0, w: win_w, h: menu_h };
        let workspace = Rect { x: 0.0, y: menu_h, w: win_w, h: win_h - menu_h };
        self.mdi.set_workspace(workspace);
        self.workspace = workspace;

        self.pump_engine();
        self.pump_run_queue();
        if self.session.exit_now {
            ctx.request_exit();
            return;
        }
        // Esc stops running code, whichever pane has the keyboard.
        if (self.session.busy() || !self.run_queue.is_empty()) && ctx.events.iter().any(|e| matches!(e, InputEvent::Key { code: KeyCode::Escape, pressed: true, .. })) {
            self.interrupt();
        }

        self.open_quit_dialog_on_request(ctx.close_requested);
        let app_modal = self.quit_dialog.is_open() || self.settings_dialog.is_open() || self.confirm_dialog.is_open();
        let data_dialog = self.data_dialog_open();
        let dialog_open = data_dialog || (self.sakha_mode && self.sakha.dialog.is_some());
        let ui_events: &[InputEvent] = if app_modal || dialog_open { &[] } else { ctx.events };

        // ── The menu bar belongs to the face and pane in front.
        let top = self.mdi.z_order().last();
        let bar = if self.sakha_mode { Bar::Sakha }
                  else if top == Some(self.source_id) { Bar::Script }
                  else if top == Some(self.plots_id) { Bar::Graphics }
                  else { Bar::Console };
        for (b, m) in [(Bar::Console, &mut self.menu_console), (Bar::Script, &mut self.menu_script),
                       (Bar::Graphics, &mut self.menu_graphics), (Bar::Sakha, &mut self.menu_sakha)] {
            if b != bar { m.open = None; }
        }

        // A click that lands on a menu must not also reach what is under it.
        let press = ui_events.iter().any(|e| matches!(e, InputEvent::MouseDown { button: MouseButton::Left, .. }));
        let on_bar = ui_events.iter().any(|e| matches!(e, InputEvent::MouseDown { pos, .. } if pos.y < menu_h));
        let ctx_was_open = self.ctx_console.is_open() || self.ctx_graphics.is_open() || self.project_menu.is_open() || self.dataset_menu.is_open();
        let menu_was_open = self.bar_menu(bar).open.is_some();
        let picked = self.project_menu.handle_events(ui_events, Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 }, renderer, theme);
        if let Some(action) = picked { self.dispatch(&action, ctx, renderer, theme); }
        let picked = self.dataset_menu.handle_events(ui_events, Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 }, renderer, theme);
        if let Some(action) = picked { self.dispatch(&action, ctx, renderer, theme); }
        // a click that only closed the popup does nothing more
        if !ctx_was_open {
            if let Some(action) = self.top_bar_input(ui_events, menu_rect, renderer, theme) {
                self.dispatch(action, ctx, renderer, theme);
            }
        }
        if let Some(action) = self.menu_events(ui_events, menu_rect, bar, renderer, theme) {
            self.dispatch(&action, ctx, renderer, theme);
        }
        let below: &[InputEvent] = if press && (on_bar || menu_was_open || ctx_was_open) { &[] } else { ui_events };

        if self.sakha_mode {
            self.sakha_input(below);
            if dialog_open && !data_dialog { self.sakha_dialog_input(ctx.events); }
        } else {
            let divider_owned = self.studio_dividers(below, ctx);
            self.mdi.handle_events(if divider_owned { &[] } else { below }, theme);
            if self.divider_hover.is_none() && self.divider_drag.is_none() {
                ctx.set_cursor(self.mdi.hover_cursor(theme));
            }
            self.apply_studio();
            // the keyboard goes to the Source pane while it is in front,
            // and to the console otherwise (as in RStudio)
            let source_top = self.mdi.z_order().last() == Some(self.source_id);
            self.console_input(if source_top { &[] } else { below }, ctx, renderer, theme, ctx_was_open);
            self.console_wheel(below, renderer, theme);
            self.source_input(below, ctx, renderer, theme);
            self.env_input(below, renderer, theme);
            if let Some(action) = self.plots_input(below, renderer, theme) {
                self.dispatch(action, ctx, renderer, theme);
            }
        }

        if data_dialog && !app_modal { self.data_dialog_input(ctx.events); }

        // what the panes asked for, and the test driver's commands
        if let Some(action) = self.queued.take() { self.dispatch(&action, ctx, renderer, theme); }
        #[cfg(feature = "drive")]
        if let Some(action) = crate::drive::take_action() { self.dispatch(&action, ctx, renderer, theme); }
        self.backup_tick();

        // ── Paint
        frame.paint_rect(workspace.x, workspace.y, workspace.w, workspace.h, theme.mdi_background);
        self.bar_menu(bar).paint_strip(frame, renderer, menu_rect, theme);
        self.paint_top_bar(frame, menu_rect, renderer, theme);
        if self.sakha_mode {
            self.paint_sakha(workspace, frame, renderer, theme);
            self.paint_sakha_dialog(workspace, frame, renderer, theme);
        } else {
            self.paint_windows(below, frame, renderer, theme);
            self.handle_close_buttons();
        }

        self.paint_data_dialogs(workspace, frame, renderer, theme);

        // Popups and context menus last: no pane can cover a menu.
        self.bar_menu(bar).paint_popup(frame, renderer, menu_rect, theme);
        self.ctx_console.paint(frame, renderer, theme);
        self.ctx_graphics.paint(frame, renderer, theme);
        self.project_menu.paint(frame, renderer, theme);
        self.dataset_menu.paint(frame, renderer, theme);

        self.run_dialogs(ctx, frame, renderer, theme, win_w, win_h);
    }

    fn bar_menu(&mut self, bar: Bar) -> &mut MenuBarState {
        match bar {
            Bar::Console => &mut self.menu_console,
            Bar::Script => &mut self.menu_script,
            Bar::Graphics => &mut self.menu_graphics,
            Bar::Sakha => &mut self.menu_sakha,
        }
    }

    fn upload_logo(&mut self, renderer: &mut Renderer) {
        if self.logo.uploaded { return; }
        if let Some(handle) = renderer.upload_image(self.logo.w, self.logo.h, &self.logo.rgba) {
            self.logo.handle = Some(handle);
            for id in [self.console_id, self.source_id, self.env_id, self.plots_id] {
                if let Some(w) = self.mdi.window_mut(id) { w.icon = Some(handle); }
            }
        }
        self.logo.uploaded = true;
    }

    /// Menu bar and right-click menus feed the same dispatch.
    fn menu_events(&mut self, ui_events: &[InputEvent], menu_rect: Rect, bar: Bar, renderer: &mut Renderer,
                   theme: &Theme) -> Option<String> {
        let topmost = self.mdi.z_order().last();
        let mb_action = self.bar_menu(bar).handle_events(ui_events, menu_rect, renderer, theme);
        if self.sakha_mode { return mb_action; }
        let cm_action = match topmost {
            Some(id) if id == self.console_id => {
                let content = self.mdi.window(id).map(|w| w.content_rect(theme));
                content.and_then(|c| self.ctx_console.handle_events(ui_events, c, renderer, theme))
            }
            Some(id) if id == self.plots_id => {
                let content = self.mdi.window(id).map(|w| w.content_rect(theme));
                content.and_then(|c| self.ctx_graphics.handle_events(ui_events, c, renderer, theme))
            }
            _ => None,
        };
        mb_action.or(cm_action)
    }

    /// Bottom to top, each pane's body, content and title bar as one unit.
    fn paint_windows(&mut self, ui_events: &[InputEvent], frame: &mut Frame,
                     renderer: &mut Renderer, theme: &Theme) {
        let order: Vec<WindowId> = self.mdi.z_order().collect();
        for id in order {
            if !self.mdi.should_paint_content(id) { continue; }
            self.mdi.paint_body(id, frame, theme);
            let Some(content) = self.mdi.window(id).filter(|w| w.visible).map(|w| w.content_rect(theme))
                else { continue };
            if id == self.console_id {
                self.paint_console(content, ui_events, frame, renderer, theme);
            } else if id == self.source_id {
                self.paint_source(content, frame, renderer, theme);
            } else if id == self.env_id {
                self.paint_env(content, frame, renderer, theme);
            } else if id == self.plots_id {
                self.paint_plots(content, frame, renderer, theme);
            }
            self.mdi.paint_titlebar(id, frame, renderer, theme);
        }
    }

    /// Close buttons hide a pane; Windows ▸ brings it back.
    fn handle_close_buttons(&mut self) {
        for id in [self.console_id, self.source_id, self.env_id, self.plots_id] {
            if self.mdi.take_close_requested(id) {
                if let Some(w) = self.mdi.window_mut(id) { w.visible = false; }
            }
        }
    }
}
