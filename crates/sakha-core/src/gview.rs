//! Gview's graphs: R2 plot code whose settings can be changed.
//!
//! A [`Graph`] is the R2 code that draws it, one statement per line. One
//! line is the graph's call (`hist`, `boxplot`, `barplot` or `plot`), and
//! each setting the user changes (title, axis names, colours, point shape)
//! is an argument of that call. The settings panel, a click on the graph
//! and the Script view all edit the same code, so they always agree. Code
//! edited in the Script view is read back the same way, and its other
//! arguments stay as the user wrote them.

/// The functions whose calls Gview edits.
const GRAPH_FNS: &[&str] = &["hist", "boxplot", "barplot", "plot"];

/// One argument: `name = value`, or a value alone.
#[derive(Debug, Clone, PartialEq)]
struct Arg {
    name: Option<String>,
    value: String,
}

/// `f(a, b = 1)` split into its name and arguments at the top level:
/// commas inside strings, brackets and nested calls do not split.
fn parse_call(line: &str) -> Option<(String, Vec<Arg>)> {
    let line = line.trim();
    let open = line.find('(')?;
    let name = line[..open].trim();
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '.' || c == '_') { return None; }
    if !line.ends_with(')') { return None; }
    let inner = &line[open + 1..line.len() - 1];
    let mut parts: Vec<String> = vec![];
    let (mut depth, mut quote, mut cur) = (0i32, None::<char>, String::new());
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                cur.push(c);
                if c == '\\' { if let Some(n) = chars.next() { cur.push(n); } }
                else if c == q { quote = None; }
            }
            None => match c {
                '"' | '\'' | '`' => { quote = Some(c); cur.push(c); }
                '(' | '[' | '{' => { depth += 1; cur.push(c); }
                ')' | ']' | '}' => { depth -= 1; cur.push(c); }
                ',' if depth == 0 => parts.push(std::mem::take(&mut cur)),
                _ => cur.push(c),
            },
        }
    }
    if depth != 0 || quote.is_some() { return None; }
    if !cur.trim().is_empty() || !parts.is_empty() { parts.push(cur); }
    let args = parts.into_iter().map(|p| {
        let p = p.trim().to_string();
        // `name = value`, not `==` and not a `=` inside the value
        match p.find('=') {
            Some(i) if !p[i + 1..].starts_with('=') && p[..i].trim().chars().all(|c| c.is_alphanumeric() || c == '.' || c == '_')
                       && !p[..i].trim().is_empty() =>
                Arg { name: Some(p[..i].trim().to_string()), value: p[i + 1..].trim().to_string() },
            _ => Arg { name: None, value: p },
        }
    }).collect();
    Some((name.to_string(), args))
}

fn write_call(name: &str, args: &[Arg]) -> String {
    let a: Vec<String> = args.iter().map(|a| match &a.name { Some(n) => format!("{n} = {}", a.value), None => a.value.clone() }).collect();
    format!("{name}({})", a.join(", "))
}

/// Physical lines joined into statements: a call spread over several
/// lines (open brackets, or a trailing comma) is one statement.
fn statements(lines: &[String]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut cur = String::new();
    let mut depth = 0i32;
    for l in lines.iter().flat_map(|l| l.lines()) {
        if cur.is_empty() { cur = l.trim_end().to_string(); } else { cur.push(' '); cur.push_str(l.trim()); }
        let mut quote = None::<char>;
        let mut esc = false;
        for c in l.chars() {
            match quote {
                Some(q) => { if esc { esc = false; } else if c == '\\' { esc = true; } else if c == q { quote = None; } }
                None => match c { '"' | '\'' | '`' => quote = Some(c), '(' | '[' | '{' => depth += 1, ')' | ']' | '}' => depth -= 1, '#' => break, _ => {} },
            }
        }
        if depth <= 0 && !cur.trim_end().ends_with(',') {
            if !cur.trim().is_empty() { out.push(std::mem::take(&mut cur)); }
            cur.clear();
            depth = 0;
        }
    }
    if !cur.trim().is_empty() { out.push(cur); }
    out
}

/// A string as R2 code.
pub fn quote(s: &str) -> String { format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")) }

/// R2 code for a string, back to the string; `None` if it is not one.
pub fn unquote(code: &str) -> Option<String> {
    let c = code.trim();
    let q = c.chars().next().filter(|q| *q == '"' || *q == '\'')?;
    if c.len() < 2 || !c.ends_with(q) { return None; }
    let mut out = String::new();
    let mut it = c[1..c.len() - 1].chars();
    while let Some(ch) = it.next() {
        if ch == '\\' { if let Some(n) = it.next() { out.push(match n { 'n' => '\n', 't' => '\t', o => o }); } } else { out.push(ch); }
    }
    Some(out)
}

/// What a setting is, for the panel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Setting {
    /// A string argument: title and axis names.
    Text,
    /// A colour, from [`COLOURS`].
    Colour,
    /// A point shape, from [`SHAPES`].
    Shape,
    /// A number between two limits, changed by a step.
    Number { min: f64, max: f64, step: f64, default: f64 },
}

/// One setting of a graph: its argument, its label and its kind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Control {
    pub arg: &'static str,
    pub label: &'static str,
    pub kind: Setting,
}

const fn c(arg: &'static str, label: &'static str, kind: Setting) -> Control { Control { arg, label, kind } }

const TITLE: Control = c("main", "Title", Setting::Text);
const XLAB: Control = c("xlab", "X axis name", Setting::Text);
const YLAB: Control = c("ylab", "Y axis name", Setting::Text);
const TITLE_SIZE: Control = c("cex.main", "Title size", Setting::Number { min: 0.6, max: 3.0, step: 0.2, default: 1.2 });
const FILL: Control = c("col", "Fill colour", Setting::Colour);
const BORDER: Control = c("border", "Border colour", Setting::Colour);

/// Colours offered, as names people know and the hex R2 draws.
pub const COLOURS: &[(&str, &str)] = &[
    ("Blue", "#4682B4"), ("Navy", "#1F3A68"), ("Teal", "#2A9D8F"), ("Green", "#3B8B3B"), ("Gold", "#E9C46A"),
    ("Orange", "#F4A261"), ("Red", "#C0392B"), ("Purple", "#7B4FA0"), ("Grey", "#9E9E9E"), ("Light grey", "#D9D9D9"),
    ("Black", "#000000"), ("White", "#FFFFFF"),
];

/// Point shapes offered: R's `pch` numbers.
pub const SHAPES: &[(&str, u32)] = &[("Open circle", 1), ("Circle", 16), ("Square", 15), ("Triangle", 17), ("Diamond", 18), ("Cross", 4), ("Plus", 3)];

/// The colour's name, if it is one offered.
pub fn colour_name(code: &str) -> Option<&'static str> {
    let v = unquote(code)?;
    COLOURS.iter().find(|(_, hex)| hex.eq_ignore_ascii_case(&v)).map(|(n, _)| *n)
}

/// Which part of a graph a click landed on, in the SVG's own coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part { Title, XLabel, YLabel, Drawing }

/// R2 lays its plots out the same way every time: the title along the top,
/// the x axis name along the bottom, the y axis name up the left side.
pub fn part_at(x: f32, y: f32, w: f32, h: f32) -> Part {
    if y < h * 0.08 { Part::Title }
    else if y > h * 0.92 { Part::XLabel }
    else if x < w * 0.05 { Part::YLabel }
    else { Part::Drawing }
}

/// A graph's R2 code, and which line is its call.
#[derive(Debug, Clone, PartialEq)]
pub struct Graph {
    pub lines: Vec<String>,
    pub call: usize,
}

impl Graph {
    /// The graph in this code: its first `hist`, `boxplot`, `barplot` or
    /// `plot` call. `None` if there is none to edit.
    pub fn from_code(lines: &[String]) -> Option<Graph> {
        let lines = statements(lines);
        let call = lines.iter().position(|l| parse_call(l).is_some_and(|(f, _)| GRAPH_FNS.contains(&f.as_str())))?;
        Some(Graph { lines, call })
    }

    /// The graph's function.
    pub fn function(&self) -> String { parse_call(&self.lines[self.call]).map(|(f, _)| f).unwrap_or_default() }

    /// A line drawn with `type = "l"` (a density curve): it has no fill or points.
    fn is_curve(&self) -> bool { self.get("type").is_some() }

    pub fn code(&self) -> String { self.lines.join("\n") }

    /// The code for reading and editing: a long graph call with one
    /// argument per line. [`Graph::from_code`] reads it back.
    pub fn script(&self) -> String {
        let mut out: Vec<String> = self.lines.clone();
        let line = &self.lines[self.call];
        if line.chars().count() > 50 {
            if let Some((f, args)) = parse_call(line) {
                let pad = " ".repeat(f.chars().count() + 1);
                let parts: Vec<String> = args.iter().map(|a| match &a.name { Some(n) => format!("{n} = {}", a.value), None => a.value.clone() }).collect();
                out[self.call] = format!("{f}({})", parts.join(&format!(",\n{pad}")));
            }
        }
        out.join("\n")
    }

    /// The settings this graph has, in the panel's order.
    pub fn controls(&self) -> Vec<Control> {
        let mut v = vec![TITLE, XLAB, YLAB, TITLE_SIZE];
        match self.function().as_str() {
            "hist" => v.extend([FILL, BORDER, c("breaks", "Bars (about)", Setting::Number { min: 2.0, max: 60.0, step: 1.0, default: 10.0 })]),
            "boxplot" | "barplot" => v.extend([FILL, BORDER]),
            "plot" if !self.is_curve() => v.extend([c("col", "Point colour", Setting::Colour), c("pch", "Point shape", Setting::Shape),
                                                    c("cex", "Point size", Setting::Number { min: 0.4, max: 4.0, step: 0.2, default: 1.0 })]),
            _ => {}
        }
        v
    }

    /// The argument's code as written, if the call has it.
    pub fn get(&self, arg: &str) -> Option<String> {
        let (_, args) = parse_call(&self.lines[self.call])?;
        args.into_iter().find(|a| a.name.as_deref() == Some(arg)).map(|a| a.value)
    }

    pub fn get_text(&self, arg: &str) -> Option<String> { self.get(arg).and_then(|v| unquote(&v)) }

    pub fn get_num(&self, arg: &str) -> Option<f64> { self.get(arg).and_then(|v| v.trim().parse().ok()) }

    /// Set the argument to this code, or remove it (R2's default) with `None`.
    pub fn set(&mut self, arg: &str, code: Option<String>) {
        let Some((f, mut args)) = parse_call(&self.lines[self.call]) else { return };
        match (args.iter().position(|a| a.name.as_deref() == Some(arg)), code) {
            (Some(i), Some(v)) => args[i].value = v,
            (Some(i), None) => { args.remove(i); }
            (None, Some(v)) => args.push(Arg { name: Some(arg.to_string()), value: v }),
            (None, None) => return,
        }
        self.lines[self.call] = write_call(&f, &args);
    }

    /// A text setting; an empty one goes back to R2's default.
    pub fn set_text(&mut self, arg: &str, s: &str) {
        self.set(arg, (!s.is_empty()).then(|| quote(s)));
    }

    pub fn set_num(&mut self, arg: &str, x: f64) {
        let v = (x * 100.0).round() / 100.0;
        self.set(arg, Some(if v.fract() == 0.0 { format!("{v:.0}") } else { format!("{v}") }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(code: &str) -> Graph { Graph::from_code(&[code.to_string()]).unwrap() }

    #[test]
    fn settings_become_arguments_and_back() {
        let mut h = g("hist(trial$score)");
        h.set_text("main", "Scores, \"final\"");
        h.set_text("xlab", "Score");
        h.set("col", Some(quote("#4682B4")));
        assert_eq!(h.code(), r##"hist(trial$score, main = "Scores, \"final\"", xlab = "Score", col = "#4682B4")"##);
        assert_eq!(h.get_text("main").as_deref(), Some("Scores, \"final\""));
        assert_eq!(colour_name(&h.get("col").unwrap()), Some("Blue"));
        h.set_text("xlab", "");
        assert_eq!(h.get("xlab"), None, "empty is R2's default again");
        h.set_num("breaks", 12.0);
        assert_eq!(h.get_num("breaks"), Some(12.0));
    }

    #[test]
    fn calls_split_only_at_their_own_commas() {
        let b = g("boxplot(score ~ grp, data = d[d$x > 1, ], main = \"a, (b)\", names = c(\"x\", \"y\"))");
        assert_eq!(b.get("data").as_deref(), Some("d[d$x > 1, ]"));
        assert_eq!(b.get_text("main").as_deref(), Some("a, (b)"));
        assert_eq!(b.get("names").as_deref(), Some("c(\"x\", \"y\")"));
        let p = g("plot(x, y == 2)");
        assert_eq!(p.get("y"), None, "== is not an argument name");
    }

    #[test]
    fn the_graph_is_the_first_plotting_call_and_curves_have_no_fill() {
        let d = Graph::from_code(&[".x <- d$s[!is.na(d$s)]".into(), ".d <- density(.x)".into(),
                                   "plot(.d$x, .d$y, type = \"l\", main = \"D\")\nlines(.d$x, .d$y)".into()]).unwrap();
        assert_eq!(d.call, 2);
        assert_eq!(d.lines.len(), 4, "multi-line strings are split");
        assert!(!d.controls().iter().any(|c| c.arg == "col"));
        assert!(g("plot(a, b)").controls().iter().any(|c| c.arg == "pch"));
        assert!(Graph::from_code(&["summary(d)".into()]).is_none());
    }

    #[test]
    fn the_script_lists_arguments_one_per_line_and_reads_back() {
        let mut h = g("hist(mtcars$mpg, main = \"Fuel economy, (32 cars)\", xlab = \"Miles per gallon\")");
        h.set("col", Some(quote("#4682B4")));
        let s = h.script();
        assert_eq!(s, "hist(mtcars$mpg,\n     main = \"Fuel economy, (32 cars)\",\n     xlab = \"Miles per gallon\",\n     col = \"#4682B4\")");
        assert_eq!(Graph::from_code(&[s]).unwrap(), Graph::from_code(&[h.code()]).unwrap());
    }

    #[test]
    fn clicks_find_the_part_of_the_graph() {
        assert_eq!(part_at(300.0, 18.0, 600.0, 400.0), Part::Title);
        assert_eq!(part_at(329.0, 392.0, 600.0, 400.0), Part::XLabel);
        assert_eq!(part_at(15.0, 198.0, 600.0, 400.0), Part::YLabel);
        assert_eq!(part_at(300.0, 200.0, 600.0, 400.0), Part::Drawing);
    }
}
