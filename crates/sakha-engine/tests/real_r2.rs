//! Drives a real `r2 --ide`. Set ARDON_STUDIO_R2 to an r2 executable built
//! with `--ide`; without it these tests say so and pass.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use sakha_engine::{json, Engine, Msg, R2_PATH_VAR};

fn r2() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var(R2_PATH_VAR).ok()?);
    if p.is_file() { Some(p) } else { None }
}

macro_rules! need_r2 {
    () => {
        match r2() {
            Some(p) => p,
            None => { eprintln!("skipped: set {R2_PATH_VAR} to an r2 with --ide"); return; }
        }
    };
}

/// Poll until the reply to `id` arrives; returns streamed text and reply.
fn run(e: &mut Engine, id: u64) -> (String, String, sakha_engine::Value) {
    let (mut out, mut err) = (String::new(), String::new());
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end {
        for m in e.poll() {
            match m {
                Msg::Stream { id: i, err: false, text } if i == id => out.push_str(&text),
                Msg::Stream { id: i, err: true, text } if i == id => err.push_str(&text),
                Msg::Reply { id: i, body } if i == id => return (out, err, body),
                Msg::Exited => panic!("r2 exited"),
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("no reply to {id}");
}

#[test]
fn console_session() {
    let exe = need_r2!();
    let (mut e, hello) = Engine::connect(&exe, &std::env::temp_dir()).unwrap();
    assert!(!hello.r2_version.is_empty());

    let id = e.eval("x <- c(81.5, 77, 92.25, NA); mean(x, na.rm = TRUE)");
    let (out, _, r) = run(&mut e, id);
    assert!(out.contains("83.58333"), "{out:?}");
    assert_eq!(r["error"], json!(false));

    let id = e.eval("f <- function(v) {");
    assert_eq!(run(&mut e, id).2["incomplete"], json!(true));

    let id = e.eval("stop(\"boom\")");
    let (_, err, r) = run(&mut e, id);
    assert!(err.contains("boom") && r["error"] == json!(true));

    let v = e.call("value", json!({"code": "summary(x)"}), Duration::from_secs(10)).unwrap();
    assert_eq!(v["value"]["values"][3], json!(83.58333333333333));
    assert!(e.call("value", json!({"code": "nope_nope"}), Duration::from_secs(10)).is_err());
    e.shutdown();
}

#[test]
fn plots_come_back_as_svg() {
    let exe = need_r2!();
    let (mut e, _) = Engine::connect(&exe, &std::env::temp_dir()).unwrap();
    let id = e.eval("hist(c(1, 2, 2, 3))");
    let r = run(&mut e, id).2;
    assert!(r["plot"].as_str().unwrap_or("").contains("<svg"));
    let id = e.eval("y <- 1");
    assert!(run(&mut e, id).2.get("plot").is_none(), "unchanged plot is not resent");
}

#[test]
fn interrupt_stops_a_loop() {
    let exe = need_r2!();
    let (mut e, _) = Engine::connect(&exe, &std::env::temp_dir()).unwrap();
    let id = e.eval("i <- 0; repeat { i <- i + 1 }");
    std::thread::sleep(Duration::from_millis(300));
    e.interrupt();
    assert_eq!(run(&mut e, id).2["interrupted"], json!(true));
    let v = e.call("value", json!({"code": "i > 0"}), Duration::from_secs(10)).unwrap();
    assert_eq!(v["value"]["values"][0], json!(true));
}
