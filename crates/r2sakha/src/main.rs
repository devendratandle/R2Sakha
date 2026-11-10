// No console window behind the IDE in release builds.
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

//! Ardon Studio — one program with two faces on one Ardon-R2 session, so nobody
//! has to install two complementary applications:
//!
//! * the **Pro**, a script environment in RStudio's arrangement;
//! * **Sakha**, menu-driven statistics where the user chooses, never codes.
//!
//! A switch in the menu bar changes face. Ardon Studio links no Ardon-R2 code: it
//! starts the installed `r2 --ide` and talks to it in JSON lines
//! (`sakha-engine`, docs/IDE_PROTOCOL.md), as RStudio drives R. R2 computes,
//! prints and plots; Ardon Studio shows.
//!
//! Files: this one sets the process up and owns the state (`Gui`); `session.rs`
//! is the link to R2 and routes its replies; `frame.rs` runs one frame in
//! order; `actions.rs` carries out menu commands; `menus.rs` builds the menus.
//! Pro: `studio_view.rs` (layout and the switch), `source_view.rs`
//! (script and data tabs), `console_view.rs` and `console.rs` (the console),
//! `env_view.rs` (Environment, History), `plots_view.rs` (Plots, Files),
//! `data_view.rs` (the data grid, Sakha's too). Sakha: `sakha_view.rs` and
//! `sakha_dialog.rs` and `sakha_doc.rs` (its `.sakha` file), on `sakha-core`.
//! `project_view.rs` is project folders (`.r2proj`). `dialogs.rs` has the modals.

use std::collections::HashMap;

use r2_ui::{
    CellGridState, ContextMenu, Dialog, ImageHandle, InputField, MdiHost, MenuBarState, R2Ui,
    Rect, ScrollOrientation, Scrollbar, Theme, WindowId,
};
use sakha_core::layout::{Divider, Studio};

mod actions;
mod console;
mod console_view;
mod data_dialogs;
mod data_view;
mod dialogs;
#[cfg(feature = "drive")]
mod drive;
mod env_view;
mod frame;
mod menus;
mod plots_view;
mod project_view;
mod sakha_doc;
mod sakha_dialog;
mod sakha_view;
mod session;
mod source_view;
mod studio_view;
mod support;
mod ui;

use console::ConsoleBuffer;
use session::{After, Session};

const LOGO_PNG: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/logo.png"));

fn main() -> Result<(), String> {
    let cwd = support::pick_user_home()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| ".".into());
    // R2 renders plots to SVG; load the system fonts for drawing them
    // off the startup path, so the first plot shows at once.
    std::thread::spawn(r2_ui::graph::warm_fonts);

    let mut gui = Gui::new(cwd)?;
    R2Ui::app("Ardon Studio")
        .theme(Theme::default())
        .initial_size(1280, 800)
        .icon_png(LOGO_PNG)
        .on_frame(move |ctx, renderer, frame, theme| {
            #[cfg(feature = "drive")]
            drive::inject(ctx);
            gui.frame(ctx, renderer, frame, theme)
        })
        .run()
}

/// Everything the app keeps between frames.
struct Gui {
    buffer: ConsoleBuffer,
    session: Session,
    /// What to do after the next console command (loading a dataset).
    next_after: Option<After>,
    /// Lines waiting to reach the console, each with what follows it.
    run_queue: std::collections::VecDeque<(String, Option<After>)>,
    mdi: MdiHost,
    console_id: WindowId,
    source_id: WindowId,
    env_id: WindowId,
    plots_id: WindowId,
    workspace: Rect,
    /// Sakha is in front; otherwise Pro.
    sakha_mode: bool,
    studio: Studio,
    divider_drag: Option<Divider>,
    divider_hover: Option<Divider>,
    source: source_view::SourcePane,
    env: env_view::EnvPane,
    plots: plots_view::PlotsPane,
    files: plots_view::FilesTab,
    /// Open data grids, by id (Source tabs and Sakha's Data tab).
    views: HashMap<u64, data_view::DataView>,
    next_view: u64,
    sakha: sakha_view::Sakha,
    /// Last pointer position: the wheel scrolls the pane under it.
    mouse: (f32, f32),
    grid: CellGridState,
    vscroll: Scrollbar,
    hscroll: Scrollbar,
    /// Previous frame's input text: typing (not scrolling) moves the
    /// horizontal view to the cursor.
    last_input: String,
    /// Transcript rows last frame: new output snaps back to the prompt.
    last_total_rows: usize,
    input: InputField,
    /// Set by q()/quit() and File ▸ Quit; the next frame opens the quit dialog.
    quit_requested: bool,
    quit_dialog: Dialog,
    settings_dialog: Dialog,
    menu_console: MenuBarState,
    menu_script: MenuBarState,
    menu_graphics: MenuBarState,
    menu_sakha: MenuBarState,
    ctx_console: ContextMenu,
    ctx_graphics: ContextMenu,
    /// The Data menu's dialogs.
    list_dialog: Option<data_dialogs::ListDialog>,
    import_dialog: Option<data_dialogs::ImportDialog>,
    attr_dialog: Option<data_dialogs::AttributesDialog>,
    /// Sakha's dataset button's popup.
    dataset_menu: ContextMenu,
    /// The project button's popup.
    project_menu: ContextMenu,
    project: Option<project_view::OpenProject>,
    recent: sakha_core::document::Recent,
    /// An action a pane asked for, dispatched later in the frame.
    queued: Option<String>,
    /// `Unsaved work: continue?` and the action waiting on its answer.
    confirm_dialog: Dialog,
    pending_discard: Option<String>,
    discard_ok: bool,
    /// An analysis left unsaved by the last run, to offer back.
    recover: Option<std::path::PathBuf>,
    last_backup: Option<std::time::Instant>,
    logo: TitleLogo,
    frame_counter: u64,
}

impl Gui {
    fn new(cwd: std::path::PathBuf) -> Result<Gui, String> {
        let mut buffer = ConsoleBuffer::new();
        buffer.push_banner(&format!("Ardon Studio {}: Pro for scripts, Sakha for menus (switch at top right).", env!("CARGO_PKG_VERSION")));
        buffer.push_banner("Ctrl+Enter runs the script line. Esc stops a running computation. q() quits.");

        // The panes' bounds are set every frame (`apply_studio`).
        let mut mdi = MdiHost::new();
        let start = Rect { x: 0.0, y: 0.0, w: 640.0, h: 440.0 };
        let env_id = mdi.add_window("Environment", start);
        let plots_id = mdi.add_window("Plots", start);
        let source_id = mdi.add_window("Source", start);
        let console_id = mdi.add_window("Console", start);

        let mut gui = Gui {
            buffer,
            session: Session::new(cwd),
            next_after: None,
            run_queue: Default::default(),
            mdi,
            console_id,
            source_id,
            env_id,
            plots_id,
            workspace: start,
            sakha_mode: false,
            studio: Studio::default(),
            divider_drag: None,
            divider_hover: None,
            source: Default::default(),
            env: Default::default(),
            plots: Default::default(),
            files: Default::default(),
            views: HashMap::new(),
            next_view: 0,
            sakha: Default::default(),
            mouse: (0.0, 0.0),
            grid: CellGridState::new(),
            vscroll: Scrollbar::new(ScrollOrientation::Vertical),
            hscroll: Scrollbar::new(ScrollOrientation::Horizontal),
            last_input: String::new(),
            last_total_rows: 0,
            input: InputField::new(),
            quit_requested: false,
            quit_dialog: Dialog::new(),
            settings_dialog: Dialog::new(),
            menu_console: menus::console_menu(),
            menu_script: menus::script_menu(),
            menu_graphics: menus::graphics_menu(),
            menu_sakha: menus::sakha_menu(),
            ctx_console: menus::console_context(),
            ctx_graphics: menus::graphics_context(),
            list_dialog: None,
            import_dialog: None,
            attr_dialog: None,
            dataset_menu: ContextMenu::new(vec![]),
            project_menu: ContextMenu::new(vec![]),
            project: None,
            recent: project_view::load_recent(),
            queued: None,
            confirm_dialog: Dialog::new(),
            pending_discard: None,
            discard_ok: false,
            recover: sakha_doc::backup_file().filter(|f| f.is_file()),
            last_backup: None,
            logo: TitleLogo::load()?,
            frame_counter: 0,
        };
        gui.add_script(r2_editor::Editor::new());
        gui.start_engine();
        // a file named on the command line: how a double-click arrives
        if let Some(arg) = std::env::args().nth(1) {
            let path = std::path::PathBuf::from(arg);
            match path.extension().and_then(|e| e.to_str()) {
                Some(sakha_core::document::PROJECT_EXT) => gui.open_project(&path),
                Some(sakha_core::document::SAKHA_EXT) => gui.open_document(&path),
                _ => gui.open_script_file(&path),
            }
        } else {
            // a fresh start types at the console, as R does
            gui.mdi.bring_to_front(gui.console_id);
        }
        Ok(gui)
    }
}

/// Title-bar logo: decoded at startup, uploaded on the first frame
/// (which is when a Renderer exists).
struct TitleLogo {
    rgba: Vec<u8>,
    w: u32,
    h: u32,
    handle: Option<ImageHandle>,
    uploaded: bool,
}

impl TitleLogo {
    fn load() -> Result<TitleLogo, String> {
        let full = image::load_from_memory(LOGO_PNG)
            .map_err(|e| format!("logo decode: {}", e))?
            .into_rgba8();
        // The logo is a wide composite; the title bar shows only its "R2"
        // mark (top ~60 %, central ~78 %), at its natural aspect.
        let (fw, fh) = (full.width(), full.height());
        let img = image::imageops::crop_imm(
            &full, (fw as f32 * 0.11) as u32, (fh as f32 * 0.03) as u32,
            (fw as f32 * 0.78) as u32, (fh as f32 * 0.60) as u32).to_image();
        let scale = 128.0 / img.width().max(img.height()) as f32;
        let nw = ((img.width() as f32 * scale).round() as u32).max(1);
        let nh = ((img.height() as f32 * scale).round() as u32).max(1);
        let resized = image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle);
        Ok(TitleLogo { rgba: resized.into_raw(), w: nw, h: nh, handle: None, uploaded: false })
    }
}
