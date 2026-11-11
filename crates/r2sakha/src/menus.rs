//! Menu-bar and right-click menu construction. Action strings share one
//! namespace with the dispatch in `actions.rs`. Pro's menus follow the
//! pane in front; Sakha has its own, built from its list of analyses.

use r2_ui::{ContextItem, ContextMenu, MenuBarState, MenuBuilder};
use sakha_core::analysis::ANALYSES;

/// Where data comes from, in both faces, on the lines of Rcmdr's Data menu. Any
/// number of R2's datasets sit behind one item; Sakha adds the attributes.
fn data_menu(mb: &mut MenuBuilder, sakha: bool) {
    let m = mb.top("Data");
    m.item("Import text file (CSV, tab, …)…", "", "data.import")
        .item("Import Parquet file…", "", "data.import_parquet")
        .item("Load R2 data file (.r2s)…", "", "data.load_r2s")
        .item("R2 datasets…", "", "data.r2_list")
        .item("Select dataset…", "", "data.select");
    if sakha {
        m.item("Variable names and types…", "", "data.attributes")
            .item("Compute variable…", "", "dataedit.compute")
            .item("Recode variable…", "", "dataedit.recode")
            .item("Filter rows…", "", "dataedit.filter")
            .item("Undo last data change", "", "dataedit.undo");
    }
}

fn windows_and_help(mb: &mut MenuBuilder) {
    mb.top("Session")
        .item("Interrupt R2",  "Esc", "session.interrupt");
    mb.top("Windows")
        .item("Show all panes", "", "win.all")
        .item("Sakha",          "", "mode.sakha");
    mb.top("Help")
        .item("About Ardon Studio",  "", "help.about");
}

/// Pro, with the console or the Environment pane in front.
pub(crate) fn console_menu() -> MenuBarState {
    let mut mb = MenuBuilder::new();
    mb.top("File")
        .item("New script",    "", "script.new")
        .item("Open script…",  "", "script.open")
        .item("Clear console", "", "file.clear")
        .item("Quit",          "Ctrl+Q", "file.quit");
    data_menu(&mut mb, false);
    mb.top("Edit")
        .item("Copy",          "Ctrl+C", "edit.copy")
        .item("Paste",         "Ctrl+V", "edit.paste")
        .item("Select all",    "Ctrl+A", "edit.select_all")
        .item("Settings…",     "",       "edit.settings");
    windows_and_help(&mut mb);
    MenuBarState::new(mb.bar)
}

/// Pro, with the Source pane in front: the editor's commands; the
/// shortcuts are handled in `source_view.rs`.
pub(crate) fn script_menu() -> MenuBarState {
    let mut mb = MenuBuilder::new();
    mb.top("File")
        .item("New script",       "Ctrl+N", "script.new")
        .item("Open script…",     "Ctrl+O", "script.open")
        .item("Save script",      "Ctrl+S", "script.save")
        .item("Save script as…",  "",       "script.save_as")
        .item("Quit",             "Ctrl+Q", "file.quit");
    data_menu(&mut mb, false);
    mb.top("Edit")
        .item("Undo",             "Ctrl+Z", "script.undo")
        .item("Redo",             "Ctrl+Y", "script.redo")
        .item("Cut",              "Ctrl+X", "script.cut")
        .item("Copy",             "Ctrl+C", "script.copy")
        .item("Paste",            "Ctrl+V", "script.paste")
        .item("Select all",       "Ctrl+A", "script.select_all");
    mb.top("Run")
        .item("Run line or selection", "Ctrl+Enter",       "script.run")
        .item("Run all",               "Ctrl+Shift+Enter", "script.run_all");
    windows_and_help(&mut mb);
    MenuBarState::new(mb.bar)
}

/// Pro, with the Plots pane in front — a plot is output: no Paste.
pub(crate) fn graphics_menu() -> MenuBarState {
    let mut mb = MenuBuilder::new();
    mb.top("File")
        .item("Save plot…",         "", "file.save_plot")
        .item("Save plot as PNG…",  "", "file.save_plot_png")
        .item("Copy plot as image", "", "file.copy_plot_image")
        .item("Copy plot SVG",      "", "file.copy_plot")
        .item("Quit",               "Ctrl+Q", "file.quit");
    windows_and_help(&mut mb);
    MenuBarState::new(mb.bar)
}

/// Sakha: the data, then what to do with it.
pub(crate) fn sakha_menu() -> MenuBarState {
    let mut mb = MenuBuilder::new();
    mb.top("File")
        .item("Load new dataset…",              "", "data.load")
        .item("Open saved analysis…",           "", "doc.open")
        .item("Save analysis",                  "Ctrl+S", "doc.save")
        .item("Save analysis as…",              "", "doc.save_as")
        .item("Export session script…",         "", "sakha.export")
        .item("Edit session script in Pro",  "", "sakha.to_studio")
        .item("Quit",                           "Ctrl+Q", "file.quit");
    data_menu(&mut mb, true);
    for top in ["Summarize", "Compare", "Relate", "Plot"] {
        let m = mb.top(top);
        if top == "Summarize" { m.item("All variables", "", "sakha.overview"); }
        for a in ANALYSES.iter().filter(|a| a.menu == top) {
            m.item(format!("{}…", a.title), "", format!("sakha.run.{}", a.id));
        }
    }
    mb.top("View")
        .item("All variables",  "", "sakha.overview")
        .item("Data",           "", "sakha.data")
        .item("Results",        "", "sakha.results")
        .item("Tables",         "", "sakha.tables")
        .item("Session script", "", "sakha.script");
    mb.top("Help")
        .item("About Ardon Studio",  "", "help.about");
    MenuBarState::new(mb.bar)
}

pub(crate) fn console_context() -> ContextMenu {
    ContextMenu::new(vec![
        ContextItem::new("Copy",       "edit.copy"),
        ContextItem::new("Paste",      "edit.paste"),
        ContextItem::new("Select all", "edit.select_all"),
        ContextItem::separator(),
        ContextItem::new("Clear console", "file.clear"),
    ])
}

pub(crate) fn graphics_context() -> ContextMenu {
    ContextMenu::new(vec![
        ContextItem::new("Save plot…",         "file.save_plot"),
        ContextItem::new("Save plot as PNG…",  "file.save_plot_png"),
        ContextItem::separator(),
        ContextItem::new("Copy plot as image", "file.copy_plot_image"),
        ContextItem::new("Copy plot SVG",      "file.copy_plot"),
    ])
}
