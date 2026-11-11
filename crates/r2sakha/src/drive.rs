//! Test driver, only in builds with `--features drive`: scripted input for
//! checking the real window end to end, with no real keyboard or mouse.
//!
//! When `ARDON_STUDIO_DRIVE` names a file, its lines are taken as commands, one
//! per frame, and the file is removed: `text ...`, `enter`, `ctrlenter`,
//! `ctrlshiftenter`, `esc`, `backspace`, `click X Y`, `wheel DY`, `ctrls`,
//! `move X Y`, `wait N` (frames), and `do ACTION` for a menu action (with a
//! path, in place of a file dialog). `do sakha.test:ID:A,B+C=TYPED` runs
//! Sakha's analysis ID with A in its first role, B and C in its second, as
//! its dialog would, so scripts need not know where the dialog's rows are.

use std::collections::VecDeque;
use std::sync::Mutex;

use r2_ui::{FrameCtx, InputEvent, KeyCode, Mods, MouseButton, MousePos};

static QUEUE: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
static ACTION: Mutex<Option<String>> = Mutex::new(None);

/// A `do` command for the frame to dispatch.
pub(crate) fn take_action() -> Option<String> { ACTION.lock().unwrap().take() }

fn key(code: KeyCode, mods: Mods) -> Vec<InputEvent> {
    vec![InputEvent::Key { code, mods, pressed: true }, InputEvent::Key { code, mods, pressed: false }]
}

/// Add this frame's scripted events to the frame's input.
pub(crate) fn inject(ctx: &mut FrameCtx) {
    let Ok(path) = std::env::var("ARDON_STUDIO_DRIVE") else { return };
    let mut q = QUEUE.lock().unwrap();
    if let Ok(text) = std::fs::read_to_string(&path) {
        if std::fs::remove_file(&path).is_ok() { q.extend(text.lines().map(String::from)); }
    }
    let Some(line) = q.pop_front() else { return };
    if let Some(n) = line.strip_prefix("wait ").and_then(|n| n.trim().parse::<u32>().ok()) {
        if n > 1 { q.push_front(format!("wait {}", n - 1)); }
        return;
    }
    let (cmd, rest) = line.split_once(' ').unwrap_or((line.as_str(), ""));
    let ctrl = Mods { ctrl: true, ..Mods::default() };
    let mut ev: Vec<InputEvent> = ctx.events.to_vec();
    match cmd {
        "text" => ev.extend(rest.chars().map(InputEvent::Char)),
        "enter" => ev.extend(key(KeyCode::Enter, Mods::default())),
        "ctrlenter" => ev.extend(key(KeyCode::Enter, ctrl)),
        "ctrlshiftenter" => ev.extend(key(KeyCode::Enter, Mods { shift: true, ..ctrl })),
        "do" => *ACTION.lock().unwrap() = Some(rest.to_string()),
        "ctrls" => ev.extend(key(KeyCode::KeyS, ctrl)),
        "esc" => ev.extend(key(KeyCode::Escape, Mods::default())),
        "backspace" => ev.extend(key(KeyCode::Backspace, Mods::default())),
        "tab" => ev.extend(key(KeyCode::Tab, Mods::default())),
        "delete" => ev.extend(key(KeyCode::Delete, Mods::default())),
        "down" => ev.extend(key(KeyCode::Down, Mods::default())),
        "right" => ev.extend(key(KeyCode::Right, Mods::default())),
        "wheel" => ev.push(InputEvent::Scroll { dy: rest.trim().parse().unwrap_or(0.0) }),
        "move" => {
            let mut n = rest.split_whitespace().filter_map(|s| s.parse::<f32>().ok());
            ev.push(InputEvent::MouseMoved(MousePos { x: n.next().unwrap_or(0.0), y: n.next().unwrap_or(0.0) }));
        }
        "click" => {
            let mut n = rest.split_whitespace().filter_map(|s| s.parse::<f32>().ok());
            let pos = MousePos { x: n.next().unwrap_or(0.0), y: n.next().unwrap_or(0.0) };
            ev.push(InputEvent::MouseMoved(pos));
            ev.push(InputEvent::MouseDown { button: MouseButton::Left, pos });
            ev.push(InputEvent::MouseUp { button: MouseButton::Left, pos });
        }
        _ => {}
    }
    ctx.events = Box::leak(ev.into_boxed_slice());
}

impl crate::Gui {
    /// `sakha.test:ID:roles`: run an analysis on variables named in the script.
    pub(crate) fn drive_run(&mut self, spec: &str) {
        let Some((id, rest)) = spec.split_once(':') else { return };
        let (roles, typed) = rest.split_once('=').unwrap_or((rest, ""));
        let (Some(a), Some(df)) = (sakha_core::analysis::find(id), self.sakha.df.clone()) else { return };
        let cols = &self.sakha.cols;
        let picks: Vec<Vec<usize>> = roles.split(',')
            .map(|r| r.split('+').filter_map(|n| cols.iter().position(|c| c.name == n.trim())).collect()).collect();
        match a.build(&df, cols, &picks, typed) {
            Ok(b) => self.run_analysis(a, b),
            Err(e) => self.sakha.status = e,
        }
    }
}
