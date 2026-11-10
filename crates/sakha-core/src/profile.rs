//! Sakha's view of a data frame's variables. This module lists the columns,
//! writes the R2 code that asks about a column ([`queries`]), reads R2's
//! JSON answers into a [`ColumnProfile`] ([`fill`]) and puts them into plain
//! words ([`ColumnProfile::says`]). The numbers are all R2's.

use sakha_engine::Value;

/// How a column is treated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind { Number, Category, Text }

impl Kind {
    pub fn label(self) -> &'static str {
        match self { Kind::Number => "number", Kind::Category => "category", Kind::Text => "text" }
    }

    /// From the `types` of a `view` reply: `num`, `int`, `chr`, `logi`, `factor`.
    pub fn from_view_type(t: &str) -> Kind {
        match t {
            "num" | "int" => Kind::Number,
            "factor" | "logi" => Kind::Category,
            _ => Kind::Text,
        }
    }
}

/// A numeric column as R2 describes it (over the non-missing values).
#[derive(Debug, Clone, PartialEq)]
pub struct NumStats {
    pub mean: f64,
    pub sd: f64,
    pub min: f64,
    pub q1: f64,
    pub median: f64,
    pub q3: f64,
    pub max: f64,
    /// `t.test(x)$conf.int`; NaN when R2 cannot compute it (n < 2).
    pub ci: (f64, f64),
    /// `hist(x, plot = FALSE)$breaks` and `$counts`.
    pub breaks: Vec<f64>,
    pub bins: Vec<usize>,
    pub distinct: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColumnProfile {
    pub name: String,
    pub kind: Kind,
    /// Rows, and of those, missing.
    pub n: usize,
    pub missing: usize,
    pub num: Option<NumStats>,
    /// Levels with counts, most common first (Category and Text).
    pub levels: Vec<(String, usize)>,
    /// A numeric column with few distinct whole values: suggest a factor.
    pub looks_categorical: bool,
    /// The queries' answers are in. Until then the card says "computing".
    pub ready: bool,
}

/// At most this many distinct whole numbers and the column "looks like a
/// category" (a 1–5 rating, a 0/1 flag, a dose level).
const FEW: usize = 10;

impl ColumnProfile {
    pub fn new(name: &str, kind: Kind) -> ColumnProfile {
        ColumnProfile { name: name.to_string(), kind, n: 0, missing: 0, num: None, levels: vec![],
                        looks_categorical: false, ready: false }
    }

    /// Can this column be the grouping variable of an analysis?
    pub fn groups(&self) -> bool { self.kind == Kind::Category || self.looks_categorical }

    /// A plain-language reading of the column ("Sakha says").
    pub fn says(&self) -> String {
        if !self.ready && self.num.is_none() && self.levels.is_empty() {
            return format!("{}: asking R2…", self.name);
        }
        let present = self.n - self.missing;
        let miss = match self.missing {
            0 => String::new(),
            m => format!(" {m} missing ({:.0}%).", 100.0 * m as f64 / self.n.max(1) as f64),
        };
        match (&self.num, self.kind) {
            (Some(s), Kind::Number) => {
                // no skew talk for a column that is really a category
                let skew = if !self.looks_categorical && s.sd > 0.0 && (s.mean - s.median).abs() > 0.2 * s.sd {
                    if s.mean > s.median { " Skewed to the right (a long tail of high values)." }
                    else { " Skewed to the left (a long tail of low values)." }
                } else { "" };
                let hint = if self.looks_categorical {
                    format!(" Only {} distinct whole values: this may be a category.", s.distinct)
                } else { String::new() };
                format!("{}: {present} values, average {} (95% CI {} to {}), typical spread ±{}; \
                         middle half between {} and {}.{skew}{miss}{hint}",
                        self.name, fmt(s.mean), fmt(s.ci.0), fmt(s.ci.1), fmt(s.sd), fmt(s.q1), fmt(s.q3))
            }
            _ if self.kind == Kind::Category && !self.levels.is_empty() => {
                let (top, c) = &self.levels[0];
                let share = 100.0 * *c as f64 / present.max(1) as f64;
                let even = self.levels.len() > 1 && self.levels.last().unwrap().1 as f64 / *c as f64 > 0.8;
                format!("{}: {} groups; most common is \"{top}\" ({share:.0}%).{}{miss}", self.name,
                        self.levels.len(), if even { " The groups are about equally common." } else { "" })
            }
            _ => format!("{}: {present} values, {} distinct (free text).{miss}", self.name, self.levels.len()),
        }
    }
}

/// `df$col`, with backticks when the column name is not a plain R name.
pub fn col_ref(df: &str, col: &str) -> String { format!("{df}${}", name_ref(col)) }

/// A column name as R2 code: backticked when it is not a plain name.
pub fn name_ref(col: &str) -> String {
    let plain = col.chars().next().is_some_and(|c| c.is_alphabetic() || c == '.')
        && col.chars().all(|c| c.is_alphanumeric() || c == '.' || c == '_');
    if plain { col.to_string() } else { format!("`{col}`") }
}

/// The R2 code Sakha evaluates for a column, in the order [`fill`] reads
/// the answers. Each line is ordinary R2 a user could type.
pub fn queries(df: &str, p: &ColumnProfile) -> Vec<String> {
    let c = col_ref(df, &p.name);
    let mut q = vec![format!("length({c})"), format!("sum(is.na({c}))")];
    match p.kind {
        Kind::Number => q.extend([
            format!("summary({c})"),
            format!("sd({c}, na.rm = TRUE)"),
            format!("tryCatch(t.test({c})$conf.int, error = function(e) c(NaN, NaN))"),
            format!("hist({c}, plot = FALSE)"),
            format!("length(unique({c}[!is.na({c})]))"),
            format!("all({c} == round({c}), na.rm = TRUE)"),
        ]),
        Kind::Category | Kind::Text => q.push(format!("table({c})")),
    }
    q
}

/// The numbers of an R2 value as JSON: NA is null, NaN and Inf are strings.
pub fn reals(v: &Value) -> Vec<f64> {
    v["values"].as_array().map(|a| a.iter().map(real).collect()).unwrap_or_default()
}

fn real(x: &Value) -> f64 {
    match x {
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::String(s) if s == "Inf" => f64::INFINITY,
        Value::String(s) if s == "-Inf" => f64::NEG_INFINITY,
        _ => f64::NAN,
    }
}

/// Read the answers to [`queries`] (same order; `None` where R2 gave an
/// error; each the `value` of its reply) into `p`, and mark it ready.
pub fn fill(p: &mut ColumnProfile, answers: &[Option<Value>]) {
    let value = |i: usize| answers.get(i).and_then(|a| a.as_ref());
    let real_at = |i: usize, k: usize| value(i).map(reals)
        .and_then(|r| r.get(k).copied()).unwrap_or(f64::NAN);
    let count = |x: f64| if x.is_finite() && x >= 0.0 { x as usize } else { 0 };
    p.n = count(real_at(0, 0));
    p.missing = count(real_at(1, 0));
    p.ready = true;
    match p.kind {
        Kind::Number => {
            if p.n == p.missing { return; } // nothing to describe
            // summary(): Min., 1st Qu., Median, Mean, 3rd Qu., Max.[, NA's]
            let hist = value(5);
            let field = |name: &str| hist.map(|h| reals(&h["fields"][name])).unwrap_or_default();
            let s = NumStats {
                min: real_at(2, 0), q1: real_at(2, 1), median: real_at(2, 2), mean: real_at(2, 3),
                q3: real_at(2, 4), max: real_at(2, 5), sd: real_at(3, 0), ci: (real_at(4, 0), real_at(4, 1)),
                breaks: field("breaks"), bins: field("counts").into_iter().map(count).collect(),
                distinct: count(real_at(6, 0)),
            };
            let whole = value(7).and_then(|v| v["values"][0].as_bool()).unwrap_or(false);
            p.looks_categorical = s.distinct <= FEW && whole && p.n - p.missing > s.distinct;
            p.num = Some(s);
        }
        Kind::Category | Kind::Text => {
            p.levels = table_levels(value(2));
            // a character column with few distinct values relative to the
            // rows is a category; many, or every value different, and it is
            // free text (names, ids)
            let present = p.n - p.missing;
            if p.kind == Kind::Text && p.levels.len() <= 20.max(present / 20) && p.levels.len() < present {
                p.kind = Kind::Category;
            }
        }
    }
}

/// The levels and counts of a `table()` result, most common first (ties in
/// the table's own order).
fn table_levels(t: Option<&Value>) -> Vec<(String, usize)> {
    let Some(t) = t else { return vec![] };
    let names: Vec<String> = t["names"].as_array()
        .map(|a| a.iter().map(|s| s.as_str().unwrap_or("NA").to_string()).collect()).unwrap_or_default();
    let mut levels: Vec<(String, usize)> = names.into_iter().zip(reals(t))
        .map(|(n, c)| (n, if c.is_finite() { c as usize } else { 0 })).collect();
    levels.sort_by_key(|l| std::cmp::Reverse(l.1));
    levels
}

/// The R2 code behind a variable's detail view, to read and rerun.
pub fn code_for(df: &str, p: &ColumnProfile) -> Vec<String> {
    let c = col_ref(df, &p.name);
    match p.kind {
        Kind::Number => vec![format!("summary({c})"), format!("sd({c}, na.rm = TRUE)"),
                             format!("t.test({c})$conf.int"), format!("hist({c})")],
        Kind::Category => vec![format!("table({c})"), format!("barplot(table({c}))")],
        Kind::Text => vec![format!("length(unique({c}))")],
    }
}

/// The code that turns a numeric column into a category.
pub fn code_as_category(df: &str, p: &ColumnProfile) -> String {
    let c = col_ref(df, &p.name);
    format!("{c} <- factor({c})")
}

/// Totals for the header line: rows, columns, columns with missing values.
pub fn dataset_line(cols: &[ColumnProfile]) -> String {
    let rows = cols.first().map_or(0, |c| c.n);
    let with_na = cols.iter().filter(|c| c.missing > 0).count();
    format!("{rows} rows · {} variables · {}", cols.len(),
            match with_na { 0 => "no missing values".to_string(), 1 => "1 with missing values".into(), k => format!("{k} with missing values") })
}

/// `x` to `digits` significant digits, shortest form, for display on cards.
/// R2's printed values stay R2's: the result panes show its own output.
pub fn fmt_digits(x: f64, digits: usize) -> String {
    if x.is_nan() { return "NA".into(); }
    if x.is_infinite() { return if x > 0.0 { "Inf".into() } else { "-Inf".into() }; }
    if x == 0.0 { return "0".into(); }
    let sci = format!("{:.*e}", digits - 1, x);
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let trim = |s: &str| if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s.to_string() };
    if !(-5..15).contains(&exp) {
        return format!("{}e{}{:02}", trim(mantissa), if exp < 0 { '-' } else { '+' }, exp.abs());
    }
    trim(&format!("{:.*}", (digits as i32 - 1 - exp).max(0) as usize, x))
}

/// A number as R prints it alone (7 significant digits).
pub fn fmt(x: f64) -> String { fmt_digits(x, 7) }

/// A number shortened for a card (4 significant digits).
pub fn fmt_short(x: f64) -> String { fmt_digits(x, 4) }

#[cfg(test)]
mod tests {
    use super::*;
    use sakha_engine::json;

    #[test]
    fn numbers_read_as_r_prints_them() {
        for (x, want) in [(83.58333333333333, "83.58333"), (79.25, "79.25"), (100.0, "100"), (0.1 + 0.2, "0.3"),
                          (3.350821820405988e-09, "3.350822e-09"), (1234567.0, "1234567"), (0.0, "0"),
                          (-2.5, "-2.5"), (1e20, "1e+20")] {
            assert_eq!(fmt(x), want);
        }
        assert_eq!(fmt(f64::NAN), "NA");
        assert_eq!(fmt_short(83.58333333), "83.58");
    }

    #[test]
    fn json_missing_values() {
        let v = json!({"values": [1.5, null, "NaN", "Inf", "-Inf"]});
        let r = reals(&v);
        assert_eq!((r[0], r[3], r[4]), (1.5, f64::INFINITY, f64::NEG_INFINITY));
        assert!(r[1].is_nan() && r[2].is_nan());
    }

    #[test]
    fn repeated_text_is_a_category_and_unique_text_is_not() {
        // length, missing, table()
        let answers = |n: f64| [Some(json!({"values": [n]})), Some(json!({"values": [0.0]})),
                                Some(json!({"names": ["x", "y"], "values": [n / 2.0, n / 2.0]}))];
        let mut unique = ColumnProfile::new("id", Kind::Text);
        fill(&mut unique, &answers(2.0));
        assert_eq!(unique.kind, Kind::Text, "every value different: an id, not a category");
        let mut repeated = ColumnProfile::new("arm", Kind::Text);
        fill(&mut repeated, &answers(4.0));
        assert_eq!(repeated.kind, Kind::Category);
    }

    #[test]
    fn names_are_quoted_when_not_plain() {
        assert_eq!(col_ref("d", "score"), "d$score");
        assert_eq!(col_ref("d", "a b"), "d$`a b`");
        assert_eq!(Kind::from_view_type("factor"), Kind::Category);
        assert_eq!(Kind::from_view_type("chr"), Kind::Text);
    }
}
