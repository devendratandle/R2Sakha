//! Every analysis on Sakha's menu, run through a real `r2 --ide` and checked
//! against the numbers GNU R 4.5.3 gives on the same data. Set ARDON_STUDIO_R2 to
//! an r2 executable built with `--ide`; without it these tests say so and pass.

use std::path::PathBuf;
use std::time::Duration;

use sakha_core::analysis::{find, ANALYSES};
use sakha_core::profile::{fill, queries, reals, ColumnProfile, Kind};
use sakha_engine::{json, Engine, Msg, R2_PATH_VAR};

const DATA: &str = r#"d <- data.frame(score = c(81.5, 77, 92.25, 70, 68, 88, 95.5, 73, 66, 84),
    before = c(5, 6, 7, 8, 5, 6, 7, 8, 6, 7),
    grp = factor(c("a","b","a","a","b","a","b","b","a","b")),
    three = c("x","y","z","x","y","z","x","y","z","x"),
    flag = c(0,1,0,1,1,0,1,0,1,0))"#;

fn session() -> Option<Engine> {
    let Some(p) = std::env::var(R2_PATH_VAR).ok().map(PathBuf::from).filter(|p| p.is_file()) else {
        eprintln!("skipped: set {R2_PATH_VAR} to an r2 with --ide");
        return None;
    };
    let (mut e, _) = Engine::connect(&p, &std::env::temp_dir()).unwrap();
    assert_eq!(e.call("eval", json!({"code": DATA}), Duration::from_secs(20)).unwrap()["error"], json!(false));
    Some(e)
}

fn ask(e: &mut Engine, code: &str) -> Vec<f64> {
    reals(&e.call("value", json!({"code": code}), Duration::from_secs(20)).unwrap_or_else(|m| panic!("{code}: {m}"))["value"])
}

/// The profiled columns of `d`, as the Sakha view fills them.
fn profiled(e: &mut Engine) -> Vec<ColumnProfile> {
    let v = e.call("view", json!({"code": "d", "offset": 0, "limit": 0}), Duration::from_secs(20)).unwrap();
    let types: Vec<String> = v["types"].as_array().unwrap().iter().map(|t| t.as_str().unwrap().to_string()).collect();
    let mut cols: Vec<ColumnProfile> = v["headers"].as_array().unwrap().iter().zip(&types)
        .map(|(h, t)| ColumnProfile::new(h.as_str().unwrap(), Kind::from_view_type(t))).collect();
    for c in &mut cols {
        let answers: Vec<_> = queries("d", c).iter()
            .map(|q| e.call("value", json!({"code": q}), Duration::from_secs(20)).ok().map(|r| r["value"].clone()))
            .collect();
        fill(c, &answers);
    }
    cols
}

fn close(got: f64, want: f64) {
    assert!((got - want).abs() <= 1e-9 * want.abs().max(1e-9), "got {got}, GNU R gives {want}");
}

#[test]
fn profiles_match_gnu_r() {
    let Some(mut e) = session() else { return };
    let cols = profiled(&mut e);
    let s = cols[0].num.as_ref().expect("score has statistics");
    close(s.mean, 79.525);
    close(s.sd, 10.3725398689681);
    assert_eq!((s.min, s.q1, s.median, s.q3, s.max), (66.0, 70.75, 79.25, 87.0, 95.5));
    close(s.ci.0, 72.1049319722776);
    close(s.ci.1, 86.9450680277224);
    assert_eq!(s.breaks, vec![65.0, 70.0, 75.0, 80.0, 85.0, 90.0, 95.0, 100.0]);
    assert_eq!(s.bins, vec![3, 1, 1, 2, 1, 1, 1]);
    assert_eq!((cols[0].n, cols[0].missing, cols[0].looks_categorical), (10, 0, false));
    assert_eq!(cols[2].levels, vec![("a".to_string(), 5), ("b".to_string(), 5)]);
    assert_eq!(cols[3].kind, Kind::Category, "few distinct strings are a category");
    assert_eq!(cols[3].levels[0], ("x".to_string(), 4));
    assert!(cols[4].looks_categorical && cols[4].groups(), "0/1 is a likely category");
}

#[test]
fn every_statistic_on_the_menu_matches_gnu_r() {
    let Some(mut e) = session() else { return };
    let cols = profiled(&mut e);
    let at = |n: &str| cols.iter().position(|c| c.name == n).unwrap();
    // id, picks, typed value, GNU R's p-value (or R-squared)
    let cases: Vec<(&str, Vec<Vec<usize>>, &str, f64)> = vec![
        ("t_one", vec![vec![at("score")], vec![]], "80", 0.888049381191773),
        ("t_two", vec![vec![at("score")], vec![at("grp")]], "", 0.994443049300187),
        ("t_paired", vec![vec![at("score")], vec![at("before")]], "", 3.35082182040599e-09),
        ("anova", vec![vec![at("score")], vec![at("grp")]], "", 0.994442507315973),
        ("cor", vec![vec![at("score")], vec![at("before")]], "", 0.772039674938386),
        ("chisq", vec![vec![at("grp")], vec![at("three")]], "", 0.0497870683678639),
        ("lm", vec![vec![at("score")], vec![at("before"), at("flag")]], "", 0.188416331649684),
    ];
    for (id, picks, typed, want) in cases {
        let a = find(id).unwrap();
        let b = a.build("d", &cols, &picks, typed).unwrap();
        let got = ask(&mut e, b.statistic.as_deref().unwrap());
        close(got[0], want);
        assert!(a.says(got[0]).is_some(), "{id} reads in plain words");
    }
}

#[test]
fn every_analysis_runs_and_shows_something() {
    let Some(mut e) = session() else { return };
    let cols = profiled(&mut e);
    for a in ANALYSES {
        let mut picks: Vec<Vec<usize>> = vec![];
        for role in a.roles {
            // the first true category the role takes (a two-group test needs
            // `grp`, not `before`), else the first column; never one already used
            let free = |i: &usize, c: &ColumnProfile| role.accepts.takes(c) && !picks.iter().any(|p: &Vec<usize>| p.contains(i));
            let of_kind = |k: Kind| cols.iter().enumerate().find(|(i, c)| c.kind == k && free(i, c)).map(|(i, _)| i);
            let choice = if role.label == "Outcome (0/1)" { cols.iter().position(|c| c.name == "flag") }
                         else { of_kind(Kind::Category).or_else(|| of_kind(Kind::Number)) };
            picks.push(choice.into_iter().collect());
        }
        let typed = if a.id == "t_one" { "80" } else { "" };
        let b = a.build("d", &cols, &picks, typed).unwrap_or_else(|m| panic!("{}: {m}", a.id));
        let id = e.eval(&b.code.join("\n"));
        let (mut out, mut err, mut plot) = (String::new(), String::new(), false);
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        'wait: while std::time::Instant::now() < deadline {
            for m in e.poll() {
                match m {
                    Msg::Stream { id: i, err: false, text } if i == id => out.push_str(&text),
                    Msg::Stream { id: i, err: true, text } if i == id => err.push_str(&text),
                    Msg::Reply { id: i, body } if i == id => {
                        assert_eq!(body["error"], json!(false), "{}: {err}", a.id);
                        plot = body.get("plot").is_some();
                        break 'wait;
                    }
                    _ => {}
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!out.is_empty() || plot, "{} showed nothing (code {:?})", a.id, b.code);
    }
}

/// The density band: GNU R's `density()` with the same formula. GNU R bins the
/// data for speed and R2 sums directly, so the curves agree to about 0.01%.
#[test]
fn the_density_band_matches_gnu_r() {
    let Some(mut e) = session() else { return };
    let cols = profiled(&mut e);
    let b = find("density").unwrap().build("d", &cols, &[vec![0]], "").unwrap();
    assert_eq!(e.call("eval", json!({ "code": b.code.join("\n") }), Duration::from_secs(20)).unwrap()["error"], json!(false), "{:?}", b.code);
    close(ask(&mut e, ".bw")[0], 5.89016719012);
    let near = |got: f64, want: f64| assert!((got - want).abs() <= 2e-3 * want.abs(), "got {got}, GNU R gives {want}");
    near(ask(&mut e, ".d[['y']][256]")[0], 0.0276303842854);
    near(ask(&mut e, ".se[256]")[0], 0.01150342553318);
    near(ask(&mut e, ".hi[256]")[0], 0.0501770983304);
    near(ask(&mut e, ".lo[256]")[0], 0.00508367024035);
    assert!(find("density").unwrap().note().is_some());
    // the three lines are in the whole figure, two of them dashed
    let file = std::env::temp_dir().join("sakha_density_test.svg");
    let path = file.to_string_lossy().replace('\\', "/");
    ask(&mut e, &format!("save.plot(\"{path}\")"));
    let svg = std::fs::read_to_string(&file).unwrap();
    let _ = std::fs::remove_file(&file);
    assert_eq!(svg.matches("stroke-dasharray").count(), 2, "{svg}");
    assert_eq!(svg.matches("<path").count(), 4, "the invisible base and the three lines");
}

/// What the Data menu's dialogs write, on real files: each separator is read
/// into its columns, and column changes land as they say.
#[test]
fn imports_and_column_changes_work_in_r2() {
    use sakha_core::import::*;
    let Some(mut e) = session() else { return };
    let dir = std::env::temp_dir().join(format!("sakha_import_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let run = |e: &mut Engine, code: &str| assert_eq!(e.call("eval", json!({ "code": code }), Duration::from_secs(20)).unwrap()["error"], json!(false), "{code}");
    for (sep, body) in [(Sep::Comma, "a,b\n1,2\n3,4\n"), (Sep::Tab, "a\tb\n1\t2\n3\t4\n"), (Sep::Semicolon, "a;b\n1;2\n3;4\n"), (Sep::Space, "a b\n1 2\n3 4\n")] {
        let file = dir.join(format!("{}.txt", sep.label()));
        std::fs::write(&file, body).unwrap();
        assert_eq!(guess_separator(body.lines().next().unwrap()), sep);
        let path = file.to_string_lossy().replace('\\', "/");
        run(&mut e, &import_line("imp", &path, true, sep));
        assert_eq!(ask(&mut e, "dim(imp)"), vec![2.0, 2.0], "{sep:?}");
        assert_eq!(ask(&mut e, "imp$b"), vec![2.0, 4.0], "{sep:?}");
    }
    let file = dir.join("nohead.csv");
    std::fs::write(&file, "1,2\n3,4\n").unwrap();
    run(&mut e, &import_line("imp", &file.to_string_lossy().replace('\\', "/"), false, Sep::Comma));
    assert_eq!(ask(&mut e, "dim(imp)"), vec![2.0, 2.0], "the first line is data");
    assert_eq!(ask(&mut e, "imp$V1"), vec![1.0, 3.0]);
    let _ = std::fs::remove_dir_all(&dir);

    // a number to a category and back keeps its values, not its codes
    run(&mut e, "t2 <- data.frame(x = c(10, 20, 10))");
    run(&mut e, &to_category_code("t2", "x"));
    let cat = e.call("value", json!({"code": "t2$x"}), Duration::from_secs(20)).unwrap();
    assert_eq!(cat["value"]["type"], json!("factor"));
    run(&mut e, &to_number_code("t2", "x"));
    assert_eq!(ask(&mut e, "t2$x"), vec![10.0, 20.0, 10.0]);
    run(&mut e, &rename_code("t2", 0, "new name"));
    assert_eq!(e.call("value", json!({"code": "names(t2)"}), Duration::from_secs(20)).unwrap()["value"]["values"][0], json!("new name"));
}

/// The R2 datasets on the Data menu: GNU R's numbers.
#[test]
fn the_datasets_on_the_menu_match_gnu_r() {
    let Some(mut e) = session() else { return };
    assert_eq!(ask(&mut e, "dim(iris)"), vec![150.0, 5.0]);
    close(ask(&mut e, "mean(iris$Sepal.Length)")[0], 5.84333333333333);
    close(ask(&mut e, "sd(iris$Petal.Width)")[0], 0.762237668960347);
    assert_eq!(ask(&mut e, "dim(mtcars)"), vec![32.0, 11.0]);
    close(ask(&mut e, "mean(mtcars$mpg)")[0], 20.090625);
    close(ask(&mut e, "sd(mtcars$wt)")[0], 0.978457442989697);
}

/// When one of these fails R2 has changed: Sakha can drop its workaround.
#[test]
fn the_r2_gaps_sakha_works_around_are_still_there() {
    let Some(mut e) = session() else { return };
    // GNU R: p = 0.0497870683678639 from the 2 x 3 table. R2 keeps only the first factor.
    assert_eq!(ask(&mut e, "chisq.test(table(d$grp, d$three))$p.value"), vec![1.0]);
    // GNU R: p = 1 (exact). R2 differs, so Mann-Whitney stays off the menu.
    close(ask(&mut e, r#"wilcox.test(d$score[d$grp == "a"], d$score[d$grp == "b"])$p.value"#)[0], 0.9168149485280885);
    // GNU R keeps every column. R2's save() and load() lose factor columns,
    // so loading an R2 data file warns; d has 5 columns, one of them a factor.
    let file = std::env::temp_dir().join("sakha_gap_test.r2s");
    let path = file.to_string_lossy().replace('\\', "/");
    let run = |e: &mut Engine, code: String| e.call("eval", json!({ "code": code }), Duration::from_secs(20)).unwrap();
    run(&mut e, format!("save(\"{path}\")"));
    run(&mut e, "rm(d)".to_string());
    run(&mut e, format!("load(\"{path}\")"));
    let _ = std::fs::remove_file(&file);
    assert_eq!(ask(&mut e, "ncol(d)"), vec![4.0]);
    // GNU R draws these. r2 --ide replies without them, so the app asks save.plot().
    let id = e.eval("plot(d$score, d$before)\nabline(h = 6, lty = 2)");
    let end = std::time::Instant::now() + Duration::from_secs(20);
    let mut reply = None;
    while reply.is_none() && std::time::Instant::now() < end {
        for m in e.poll() { if let Msg::Reply { id: i, body } = m { if i == id { reply = Some(body); } } }
        std::thread::sleep(Duration::from_millis(5));
    }
    let plot = reply.unwrap()["plot"].as_str().unwrap_or("").to_string();
    assert!(plot.contains("<svg") && !plot.contains("stroke-dasharray"), "--ide now sends overlays: drop ask_whole_plot");
}
