//! R2Sakha's engine: the installed Ardon-R2, run as `r2 --ide`.
//!
//! R2Sakha links no Ardon-R2 code. Like RStudio with R, it starts the
//! `r2` executable as a child process and talks to it over stdin/stdout,
//! one JSON object per line (docs/IDE_PROTOCOL.md). R2 does every
//! statistic, every printed value and every plot; R2Sakha displays them.
//!
//! The GUI never blocks on R2: [`Engine::send`] returns the request's id
//! at once, and each frame [`Engine::poll`] hands over whatever R2 wrote
//! since — output as it is printed ([`Msg::Stream`]) and replies
//! ([`Msg::Reply`]). [`Engine::call`] is the blocking form, for startup,
//! quitting and tests.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

pub use serde_json::{json, Value};

/// Read JSON text (the app's own files use the same library as the protocol).
pub fn parse_json(text: &str) -> Result<Value, String> { serde_json::from_str(text).map_err(|e| e.to_string()) }

/// JSON as readable, line-by-line text.
pub fn pretty_json(v: &Value) -> String { serde_json::to_string_pretty(v).unwrap_or_default() }

/// The protocol version this R2Sakha speaks (`hello` must report it).
pub const PROTOCOL: u64 = 1;

/// Environment variable naming the `r2` executable to use.
pub const R2_PATH_VAR: &str = "R2SAKHA_R2";

/// Something R2 wrote.
#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    /// Printed output of the `eval` with this id, as it happens.
    Stream { id: u64, err: bool, text: String },
    /// The reply to request `id`: the whole reply object.
    Reply { id: u64, body: Value },
    /// R2 closed its output: it quit or crashed.
    Exited,
}

/// What `hello` reported.
#[derive(Debug, Clone)]
pub struct Hello {
    pub r2_version: String,
    pub protocol: u64,
}

pub struct Engine {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Msg>,
    next_id: u64,
    exe: PathBuf,
}

impl Engine {
    /// Start `exe --ide` in `cwd` (the session's working folder).
    pub fn start(exe: &Path, cwd: &Path) -> Result<Engine, String> {
        let mut cmd = Command::new(exe);
        cmd.arg("--ide").current_dir(cwd)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        #[cfg(windows)]
        {
            // no console window flashing up behind the IDE
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn().map_err(|e| format!("cannot start {}: {e}", exe.display()))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let (tx, rx) = channel();
        std::thread::Builder::new().name("r2-reader".into()).spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Some(m) = parse_line(&line) {
                    if tx.send(m).is_err() { return; }
                }
            }
            let _ = tx.send(Msg::Exited);
        }).map_err(|e| e.to_string())?;
        Ok(Engine { child, stdin, rx, next_id: 1, exe: exe.to_path_buf() })
    }

    /// Start R2 and check that it speaks this protocol.
    pub fn connect(exe: &Path, cwd: &Path) -> Result<(Engine, Hello), String> {
        let mut e = Engine::start(exe, cwd)?;
        let r = e.call("hello", json!({}), Duration::from_secs(20))
            .map_err(|m| format!("{} did not answer as an R2 engine ({m}); it needs Ardon-R2 with `r2 --ide`", exe.display()))?;
        let hello = Hello {
            r2_version: r["r2"].as_str().unwrap_or("?").to_string(),
            protocol: r["protocol"].as_u64().unwrap_or(0),
        };
        if hello.protocol != PROTOCOL {
            return Err(format!("{} speaks IDE protocol {}, R2Sakha needs {PROTOCOL}", exe.display(), hello.protocol));
        }
        Ok((e, hello))
    }

    pub fn exe(&self) -> &Path { &self.exe }

    /// Send a request; returns its id. `fields` (an object) is merged in.
    pub fn send(&mut self, op: &str, fields: Value) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let mut req = json!({ "id": id, "op": op });
        if let (Some(r), Value::Object(f)) = (req.as_object_mut(), fields) { r.extend(f); }
        self.write(&req);
        id
    }

    /// Console input; output streams back, then the reply.
    pub fn eval(&mut self, code: &str) -> u64 { self.send("eval", json!({ "code": code })) }

    /// Stop the running `eval` (no reply of its own).
    pub fn interrupt(&mut self) { self.write(&json!({ "op": "interrupt" })); }

    fn write(&mut self, req: &Value) {
        // a dead R2 shows up as Msg::Exited on the reader side
        let _ = writeln!(self.stdin, "{req}");
        let _ = self.stdin.flush();
    }

    /// Everything R2 wrote since the last call; never waits.
    pub fn poll(&self) -> Vec<Msg> { self.rx.try_iter().collect() }

    /// Send and wait for the reply. Stream lines met on the way are
    /// dropped; `Err` holds R2's error message, or says R2 is gone.
    pub fn call(&mut self, op: &str, fields: Value, timeout: Duration) -> Result<Value, String> {
        let id = self.send(op, fields);
        let end = Instant::now() + timeout;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(Msg::Reply { id: rid, body }) if rid == id => {
                    return if body["ok"] == Value::Bool(true) { Ok(body) }
                           else { Err(body["error"].as_str().unwrap_or("error").to_string()) };
                }
                Ok(Msg::Exited) => return Err("R2 exited".into()),
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => return Err("R2 did not answer in time".into()),
                Err(RecvTimeoutError::Disconnected) => return Err("R2 exited".into()),
            }
        }
    }

    /// Ask R2 to quit and wait briefly for it; kill it if it lingers.
    pub fn shutdown(mut self) { self.stop(); }

    fn stop(&mut self) {
        let _ = writeln!(self.stdin, "{}", json!({ "id": 0, "op": "quit" }));
        let _ = self.stdin.flush();
        let end = Instant::now() + Duration::from_secs(2);
        while Instant::now() < end {
            if let Ok(Some(_)) = self.child.try_wait() { return; }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() { self.stop(); }
    }
}

/// One line from R2. Lines that are not protocol JSON are ignored; anything
/// before the object is dropped, since some R2 builtins (`clear()`) write
/// raw terminal bytes to stdout with no newline, onto the next reply's line.
fn parse_line(line: &str) -> Option<Msg> {
    let v: Value = serde_json::from_str(line.get(line.find("{\"id\"")?..)?).ok()?;
    let id = v["id"].as_u64()?;
    if let Some(s) = v["stream"].as_str() {
        return Some(Msg::Stream { id, err: s == "err", text: v["text"].as_str().unwrap_or("").to_string() });
    }
    Some(Msg::Reply { id, body: v })
}

/// The `r2` executable to run, first found: `$R2SAKHA_R2`; `r2` beside
/// R2Sakha's own executable; `r2` on PATH; the Ardon-R2 installer's
/// folder.
pub fn find_r2() -> Option<PathBuf> {
    let exe_name = if cfg!(windows) { "r2.exe" } else { "r2" };
    if let Ok(p) = std::env::var(R2_PATH_VAR) {
        let p = PathBuf::from(p);
        if p.is_file() { return Some(p); }
    }
    let beside = std::env::current_exe().ok()
        .and_then(|e| e.parent().map(|d| d.join(exe_name)));
    let on_path = std::env::var_os("PATH").into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .map(|d| d.join(exe_name));
    let installed = ["ProgramFiles", "ProgramW6432"].iter()
        .filter_map(std::env::var_os)
        .map(|d| PathBuf::from(d).join("Ardon-R2").join(exe_name));
    beside.into_iter().chain(on_path).chain(installed).find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_become_messages() {
        assert_eq!(parse_line(r#"{"id":3,"stream":"err","text":"Error: boom\n"}"#),
                   Some(Msg::Stream { id: 3, err: true, text: "Error: boom\n".into() }));
        match parse_line(r#"{"id":4,"ok":true,"incomplete":true}"#) {
            Some(Msg::Reply { id: 4, body }) => assert_eq!(body["incomplete"], Value::Bool(true)),
            other => panic!("{other:?}"),
        }
        assert_eq!(parse_line("not json"), None);
        assert!(matches!(parse_line("\u{1b}[2J\u{1b}[H{\"id\":5,\"ok\":true}"), Some(Msg::Reply { id: 5, .. })));
    }
}
