//! Test driver, only in builds with `--features drive`: scripted input for
//! checking the real window end to end, with no real keyboard or mouse.
//!
//! When `R2SAKHA_DRIVE` names a file, its lines are taken as commands, one
//! per frame, and the file is removed: `text ...`, `enter`, `ctrlenter`,
//! `ctrlshiftenter`, `esc`, `backspace`, `click X Y`, `wheel DY`, `ctrls`,
//! and `do ACTION` for a menu action (with a path, in place of a file dialog).

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
    let Ok(path) = std::env::var("R2SAKHA_DRIVE") else { return };
    let mut q = QUEUE.lock().unwrap();
    if let Ok(text) = std::fs::read_to_string(&path) {
        if std::fs::remove_file(&path).is_ok() { q.extend(text.lines().map(String::from)); }
    }
    let Some(line) = q.pop_front() else { return };
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
        "wheel" => ev.push(InputEvent::Scroll { dy: rest.trim().parse().unwrap_or(0.0) }),
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
