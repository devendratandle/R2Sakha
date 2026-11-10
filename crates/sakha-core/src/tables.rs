//! Publishable tables, built from the results the user has already run.
//!
//! Researchers often run the same test on several variables (a t-test for
//! each outcome, a correlation for each pair) and then copy the numbers by
//! hand into one table. Here every result that has a table keeps the numbers
//! R2 returned for it ([`numbers_code`] is the R2 code that asks for them),
//! and [`build_all`] gathers the results of each analysis into one table in
//! APA style: test statistic, degrees of freedom, p, confidence interval and
//! an effect size. Tables are written out as HTML (for Word or a web page),
//! CSV (for a spreadsheet) or tab-separated text (for pasting).
//!
//! **The numbers are R2's.** The only arithmetic done here is on R2's numbers:
//! effect sizes (Cohen's d, eta squared, Cramér's V, McFadden's R²) and the
//! Fisher-z confidence interval of a correlation, each given in the table's note.

use sakha_engine::Value;

use crate::analysis::chisq_call;
use crate::profile::{col_ref, name_ref, reals, ColumnProfile};

/// Names R2 code here leaves behind start with `.`, so they stay out of the
/// Environment pane, like the density curve's.
const T: &str = ".sk";
const M: &str = ".skm";

/// The numbers R2 returned for one result, with their names when R2 gave any.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Numbers {
    pub values: Vec<f64>,
    pub names: Vec<String>,
}

impl Numbers {
    /// From a `value` reply's `value`; `None` if it is not numeric.
    pub fn from_value(v: &Value) -> Option<Numbers> {
        let values = reals(v);
        if values.is_empty() { return None; }
        let names = v["names"].as_array().map(|a| a.iter().map(|n| n.as_str().unwrap_or("").to_string()).collect())
            .unwrap_or_default();
        Some(Numbers { values, names })
    }

    fn at(&self, i: usize) -> f64 { self.values.get(i).copied().unwrap_or(f64::NAN) }

    fn name(&self, i: usize) -> String { self.names.get(i).cloned().unwrap_or_default() }
}

/// Which analyses make tables.
pub fn tabulates(id: &str) -> bool {
    matches!(id, "freq" | "group_summary" | "t_one" | "t_two" | "t_paired" | "anova" | "cor" | "chisq" | "lm" | "glm")
}

/// The R2 code that returns the numbers a table needs for these picks, as
/// one numeric vector. Its layout, per analysis, is what [`build`] reads.
pub fn numbers_code(id: &str, df: &str, cols: &[ColumnProfile], picks: &[Vec<usize>], typed: &str) -> Option<String> {
    let col = |r: usize| picks.get(r).and_then(|p| p.first()).and_then(|&i| cols.get(i));
    let c = |r: usize| col(r).map(|p| col_ref(df, &p.name));
    let n = |r: usize| col(r).map(|p| name_ref(&p.name));
    let terms = |r: usize| {
        let t: Vec<String> = picks.get(r).into_iter().flatten().filter_map(|&i| cols.get(i)).map(|p| name_ref(&p.name)).collect();
        (!t.is_empty()).then(|| t.join(" + "))
    };
    Some(match id {
        // counts, named by category
        "freq" => format!("c(table({}))", c(0)?),
        // n, M, SD per group, each named by group
        "group_summary" => {
            let (y, g) = (c(0)?, c(1)?);
            format!("c(tapply(!is.na({y}), {g}, sum), tapply({y}, {g}, mean, na.rm = TRUE), tapply({y}, {g}, sd, na.rm = TRUE))")
        }
        // M, SD, n, t, df, p, CI low, CI high, test value
        "t_one" => {
            let mu: f64 = typed.trim().parse().ok()?;
            let y = c(0)?;
            format!("{{ {T} <- t.test({y}, mu = {mu}); {M} <- {y}[!is.na({y})]; \
                     c(mean({M}), sd({M}), length({M}), {T}$statistic, {T}$parameter, {T}$p.value, {T}$conf.int, {mu}) }}")
        }
        // M1, M2, SD1, SD2, n1, n2 (named by group), t, df, p, CI low, CI high
        "t_two" => {
            let (y, g) = (c(0)?, c(1)?);
            format!("{{ {T} <- t.test({} ~ {}, data = {df}); c(tapply({y}, {g}, mean, na.rm = TRUE), \
                     tapply({y}, {g}, sd, na.rm = TRUE), tapply(!is.na({y}), {g}, sum), \
                     {T}$statistic, {T}$parameter, {T}$p.value, {T}$conf.int) }}", n(0)?, n(1)?)
        }
        // M1, SD1, M2, SD2, n, t, df, p, CI low, CI high, SD of the differences
        "t_paired" => {
            let (x, y) = (c(0)?, c(1)?);
            format!("{{ {T} <- t.test({x}, {y}, paired = TRUE); {M} <- !is.na({x}) & !is.na({y}); \
                     c(mean({x}[{M}]), sd({x}[{M}]), mean({y}[{M}]), sd({y}[{M}]), sum({M}), \
                     {T}$statistic, {T}$parameter, {T}$p.value, {T}$conf.int, sd({x}[{M}] - {y}[{M}])) }}")
        }
        // F, df between, df within, p, SS between, SS total
        "anova" => format!("{{ {T} <- summary(aov({} ~ factor({}), data = {df})); \
                            c({T}$f.statistic, {T}$df.between, {T}$df.within, {T}$p.value, {T}$ss.between, {T}$ss.total) }}", n(0)?, n(1)?),
        // r, df, p, t, n
        "cor" => {
            let (x, y) = (c(0)?, c(1)?);
            format!("{{ {T} <- cor.test({x}, {y}); c({T}$estimate, {T}$df, {T}$p.value, {T}$statistic, sum(!is.na({x}) & !is.na({y}))) }}")
        }
        // chi-square, df, p, N, the smaller number of categories
        "chisq" => {
            let (a, b) = (col(0)?, col(1)?);
            if a.levels.len() < 2 || b.levels.len() < 2 { return None; }
            let (ca, cb) = (c(0)?, c(1)?);
            let k = a.levels.len().min(b.levels.len());
            format!("{{ {T} <- {}; c({T}$statistic, {T}$parameter, {T}$p.value, sum(!is.na({ca}) & !is.na({cb})), {k}) }}",
                    chisq_call(df, a, b))
        }
        // B (named by term), SE, p for each of the k terms; R², adjusted R², F, residual df
        "lm" => format!("{{ {M} <- lm({} ~ {}, data = {df}); {T} <- summary({M}); \
                         c(coef({M}), {T}$std.errors, {T}$p.values, {T}$r.squared, {T}$adj.r.squared, {T}$f.statistic, {T}$df) }}",
                        n(0)?, terms(1)?),
        // B (named by term), SE, p for each of the k terms; AIC, deviance, null deviance, residual df
        "glm" => format!("{{ {M} <- glm({} ~ {}, family = binomial, data = {df}); {T} <- summary({M}); \
                          c(coef({M}), {T}$std.errors, {T}$p.values, {T}$aic, {T}$deviance, {T}$null.deviance, {T}$df.residual) }}",
                         n(0)?, terms(1)?),
        _ => return None,
    })
}

/// One result, as a table reads it.
#[derive(Debug, Clone, Copy)]
pub struct Entry<'a> {
    /// The analysis's id on the menu.
    pub analysis: &'a str,
    /// The variables picked, role by role.
    pub labels: &'a [String],
    pub numbers: &'a Numbers,
}

impl Entry<'_> {
    fn label(&self, i: usize) -> &str { self.labels.get(i).map(String::as_str).unwrap_or("") }
}

/// A finished table, in APA style.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    /// The analysis it gathers.
    pub analysis: String,
    /// The title line under "Table N".
    pub title: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    /// The note under the table: what the columns mean.
    pub note: String,
}

/// One table per analysis that has tables, in the order the analyses were
/// first run; each gathers that analysis's entries in their order.
pub fn build_all(entries: &[Entry]) -> Vec<Table> {
    let mut ids: Vec<&str> = vec![];
    for e in entries {
        if tabulates(e.analysis) && !ids.contains(&e.analysis) { ids.push(e.analysis); }
    }
    ids.into_iter().filter_map(|id| {
        let of: Vec<Entry> = entries.iter().filter(|e| e.analysis == id).copied().collect();
        build(id, &of)
    }).collect()
}

const STARS: &str = "* p < .05. ** p < .01. *** p < .001.";

/// The table of `id` from its entries; entries whose numbers do not have the
/// expected layout (R2 could not compute them) are left out.
pub fn build(id: &str, entries: &[Entry]) -> Option<Table> {
    let table = |title: &str, columns: &[&str], rows: Vec<Vec<String>>, note: String| {
        (!rows.is_empty()).then(|| Table { analysis: id.to_string(), title: title.to_string(),
                                            columns: columns.iter().map(|c| c.to_string()).collect(), rows, note })
    };
    let fits = |e: &&Entry, len: usize| e.numbers.values.len() == len;
    match id {
        "freq" => {
            let mut rows = vec![];
            for e in entries {
                let total: f64 = e.numbers.values.iter().filter(|v| v.is_finite()).sum();
                for (k, &v) in e.numbers.values.iter().enumerate() {
                    rows.push(vec![if k == 0 { e.label(0).to_string() } else { String::new() }, e.numbers.name(k),
                                   count(v), fixed(100.0 * v / total, 1)]);
                }
            }
            table("Frequencies", &["Variable", "Category", "n", "%"], rows, "Percentages are of the non-missing values.".into())
        }
        "group_summary" => {
            let mut rows = vec![];
            for e in entries.iter().filter(|e| e.numbers.values.len() % 3 == 0) {
                let g = e.numbers.values.len() / 3;
                for k in 0..g {
                    let x = &e.numbers;
                    rows.push(vec![if k == 0 { format!("{} by {}", e.label(0), e.label(1)) } else { String::new() },
                                   x.name(k), count(x.at(k)), fixed(x.at(g + k), 2), fixed(x.at(2 * g + k), 2)]);
                }
            }
            table("Descriptive statistics by group", &["Variable", "Group", "n", "M", "SD"], rows,
                  "n counts the non-missing values in each group.".into())
        }
        "t_one" => {
            let rows = entries.iter().filter(|e| fits(e, 9)).map(|e| {
                let x = e.numbers;
                let (m, sd, n, t, df, p, lo, hi, mu) = (x.at(0), x.at(1), x.at(2), x.at(3), x.at(4), x.at(5), x.at(6), x.at(7), x.at(8));
                vec![e.label(0).into(), count(n), fixed(m, 2), fixed(sd, 2), short(mu), fixed(t, 2), dfs(df), p_apa(p),
                     ci(lo, hi), fixed((m - mu) / sd, 2)]
            }).collect();
            table("One-sample t-tests", &["Variable", "n", "M", "SD", "Test value", "t", "df", "p", "95% CI", "d"], rows,
                  "CI = confidence interval of the mean. d = Cohen's d, (M − test value) / SD.".into())
        }
        "t_two" => {
            let ok: Vec<&Entry> = entries.iter().filter(|e| fits(e, 11)).collect();
            let levels = |e: &Entry| (e.numbers.name(0), e.numbers.name(1));
            let first = ok.first()?;
            // one grouping for all rows: its groups head the columns
            let shared = ok.iter().all(|e| e.label(1) == first.label(1) && levels(e) == levels(first));
            let (l1, l2) = levels(first);
            let rows = ok.iter().map(|e| {
                let x = e.numbers;
                let (m1, m2, s1, s2, n1, n2) = (x.at(0), x.at(1), x.at(2), x.at(3), x.at(4), x.at(5));
                let (t, df, p, lo, hi) = (x.at(6), x.at(7), x.at(8), x.at(9), x.at(10));
                let pooled = (((n1 - 1.0) * s1 * s1 + (n2 - 1.0) * s2 * s2) / (n1 + n2 - 2.0)).sqrt();
                let mut row = vec![e.label(0).to_string()];
                if !shared { row.push(format!("{}: {} vs {}", e.label(1), x.name(0), x.name(1))); }
                row.extend([m_sd(m1, s1), m_sd(m2, s2), fixed(t, 2), dfs(df), p_apa(p), ci(lo, hi), fixed((m1 - m2) / pooled, 2)]);
                row
            }).collect();
            let (h1, h2) = if shared { (format!("{l1}: M (SD)"), format!("{l2}: M (SD)")) }
                           else { ("Group 1: M (SD)".to_string(), "Group 2: M (SD)".to_string()) };
            let mut cols: Vec<&str> = vec!["Variable"];
            if !shared { cols.push("Groups"); }
            cols.extend([h1.as_str(), h2.as_str(), "t", "df", "p", "95% CI", "d"]);
            let by = if shared { format!(" by {}", first.label(1)) } else { String::new() };
            let diff = if shared { format!("{l1} − {l2}") } else { "group 1 − group 2".into() };
            table(&format!("Two-group t-tests{by}"), &cols, rows,
                  format!("Welch's t-test (equal variances not assumed). CI = 95% confidence interval of the difference in means ({diff}). \
                           d = Cohen's d with the pooled SD."))
        }
        "t_paired" => {
            let rows = entries.iter().filter(|e| fits(e, 11)).map(|e| {
                let x = e.numbers;
                let (m1, s1, m2, s2, n) = (x.at(0), x.at(1), x.at(2), x.at(3), x.at(4));
                let (t, df, p, lo, hi, sdd) = (x.at(5), x.at(6), x.at(7), x.at(8), x.at(9), x.at(10));
                vec![format!("{} – {}", e.label(0), e.label(1)), count(n), m_sd(m1, s1), m_sd(m2, s2), fixed(t, 2), dfs(df), p_apa(p),
                     ci(lo, hi), fixed((m1 - m2) / sdd, 2)]
            }).collect();
            table("Paired t-tests", &["Pair", "n", "First: M (SD)", "Second: M (SD)", "t", "df", "p", "95% CI", "d"], rows,
                  "CI = 95% confidence interval of the mean difference (first − second). \
                   d = mean difference / SD of the differences.".into())
        }
        "anova" => {
            let ok: Vec<&Entry> = entries.iter().filter(|e| fits(e, 6)).collect();
            let first = ok.first()?;
            let shared = ok.iter().all(|e| e.label(1) == first.label(1));
            let rows = ok.iter().map(|e| {
                let x = e.numbers;
                let mut row = vec![e.label(0).to_string()];
                if !shared { row.push(e.label(1).to_string()); }
                row.extend([fixed(x.at(0), 2), format!("{}, {}", dfs(x.at(1)), dfs(x.at(2))), p_apa(x.at(3)), unit(x.at(4) / x.at(5), 2)]);
                row
            }).collect();
            let cols: &[&str] = if shared { &["Variable", "F", "df", "p", "η²"] } else { &["Variable", "Groups", "F", "df", "p", "η²"] };
            let by = if shared { format!(" across {}", first.label(1)) } else { String::new() };
            table(&format!("One-way ANOVAs{by}"), cols, rows,
                  "df = between groups, within groups. η² = between-groups sum of squares / total sum of squares.".into())
        }
        "cor" => cor_table(entries),
        "chisq" => {
            let rows = entries.iter().filter(|e| fits(e, 5)).map(|e| {
                let x = e.numbers;
                let (chi, df, p, n, k) = (x.at(0), x.at(1), x.at(2), x.at(3), x.at(4));
                vec![format!("{} × {}", e.label(0), e.label(1)), count(n), fixed(chi, 2), dfs(df), p_apa(p),
                     unit((chi / (n * (k - 1.0))).sqrt(), 2)]
            }).collect();
            table("Chi-square tests of association", &["Variables", "N", "χ²", "df", "p", "Cramér's V"], rows,
                  "V = Cramér's V, √(χ² / (N × (k − 1))), k the smaller number of categories.".into())
        }
        "lm" | "glm" => models_table(id, entries),
        _ => None,
    }
}

/// Correlations: a matrix once three or more variables are involved, the
/// way they are published; for one or two pairs, a row per pair.
fn cor_table(entries: &[Entry]) -> Option<Table> {
    let ok: Vec<&Entry> = entries.iter().filter(|e| e.numbers.values.len() == 5).collect();
    if ok.is_empty() { return None; }
    let fisher = |r: f64, n: f64| {
        let (z, se) = (r.atanh(), 1.0 / (n - 3.0).sqrt());
        ((z - 1.959_963_985 * se).tanh(), (z + 1.959_963_985 * se).tanh())
    };
    let mut vars: Vec<&str> = vec![];
    for e in &ok {
        for v in [e.label(0), e.label(1)] { if !vars.contains(&v) { vars.push(v); } }
    }
    let note_ci = "CI = 95% confidence interval (Fisher's z).";
    if vars.len() < 3 {
        let rows = ok.iter().map(|e| {
            let x = e.numbers;
            let (lo, hi) = fisher(x.at(0), x.at(4));
            vec![format!("{} – {}", e.label(0), e.label(1)), count(x.at(4)), unit(x.at(0), 2), ci_unit(lo, hi), dfs(x.at(1)), p_apa(x.at(2))]
        }).collect();
        return Some(Table { analysis: "cor".into(), title: "Pearson correlations".into(),
                            columns: ["Variables", "n", "r", "95% CI", "df", "p"].iter().map(|s| s.to_string()).collect(),
                            rows, note: note_ci.into() });
    }
    let mut columns = vec!["Variable".to_string()];
    columns.extend((1..vars.len()).map(|k| k.to_string()));
    let mut ns: Vec<f64> = vec![];
    let rows = vars.iter().enumerate().map(|(i, v)| {
        let mut row = vec![format!("{}. {v}", i + 1)];
        for (j, w) in vars.iter().enumerate().take(vars.len() - 1) {
            row.push(if j == i { "—".into() } else if j > i { String::new() } else {
                ok.iter().rev().find(|e| (e.label(0) == *v && e.label(1) == *w) || (e.label(0) == *w && e.label(1) == *v))
                    .map(|e| { ns.push(e.numbers.at(4)); format!("{}{}", unit(e.numbers.at(0), 2), stars(e.numbers.at(2))) })
                    .unwrap_or_default()
            });
        }
        row
    }).collect();
    ns.sort_by(|a, b| a.total_cmp(b));
    ns.dedup();
    let n = match ns.as_slice() { [one] => format!("n = {}. ", count(*one)),
                                  [lo, .., hi] => format!("n = {} to {}. ", count(*lo), count(*hi)), [] => String::new() };
    Some(Table { analysis: "cor".into(), title: "Pearson correlations".into(), columns, rows,
                 note: format!("{n}Pairwise complete observations; a blank cell is a pair not run. {STARS}") })
}

/// Regression models side by side: a column per model, a row per term,
/// each cell B (SE) with stars; the fit below.
fn models_table(id: &str, entries: &[Entry]) -> Option<Table> {
    // after the k terms' B, SE and p: four numbers of fit, for both models
    let extra = 4;
    let ok: Vec<&Entry> = entries.iter()
        .filter(|e| e.numbers.values.len() > extra && (e.numbers.values.len() - extra) % 3 == 0).collect();
    if ok.is_empty() { return None; }
    let k_of = |e: &Entry| (e.numbers.values.len() - extra) / 3;
    let mut terms: Vec<String> = vec![];
    for e in &ok {
        for i in 0..k_of(e) {
            let t = e.numbers.name(i);
            if !terms.contains(&t) { terms.push(t); }
        }
    }
    let mut columns = vec!["Predictor".to_string()];
    columns.extend(ok.iter().enumerate().map(|(m, e)| format!("Model {}: {}", m + 1, e.label(0))));
    let mut rows: Vec<Vec<String>> = terms.iter().map(|t| {
        let mut row = vec![if t == "(Intercept)" { "Intercept".to_string() } else { t.clone() }];
        row.extend(ok.iter().map(|e| {
            let k = k_of(e);
            (0..k).find(|&i| e.numbers.name(i) == *t).map(|i| {
                let x = e.numbers;
                format!("{}{} ({})", fixed(x.at(i), 2), stars(x.at(2 * k + i)), fixed(x.at(k + i), 2))
            }).unwrap_or_default()
        }));
        row
    }).collect();
    let fit = |label: &str, f: &dyn Fn(&Numbers, usize) -> String| {
        let mut row = vec![label.to_string()];
        row.extend(ok.iter().map(|e| f(e.numbers, 3 * k_of(e))));
        row
    };
    if id == "lm" {
        rows.push(fit("R²", &|x, b| unit(x.at(b), 3)));
        rows.push(fit("Adjusted R²", &|x, b| unit(x.at(b + 1), 3)));
        rows.push(fit("F", &|x, b| format!("{} ({}, {})", fixed(x.at(b + 2), 2), dfs((b / 3) as f64 - 1.0), dfs(x.at(b + 3)))));
        rows.push(fit("N", &|x, b| count(x.at(b + 3) + (b / 3) as f64)));
        Some(Table { analysis: id.into(), title: "Linear regressions".into(), columns, rows,
                     note: format!("Unstandardized coefficients, B (SE). F (df model, df residual). {STARS}") })
    } else {
        rows.push(fit("AIC", &|x, b| fixed(x.at(b), 2)));
        rows.push(fit("McFadden's R²", &|x, b| unit(1.0 - x.at(b + 1) / x.at(b + 2), 3)));
        rows.push(fit("N", &|x, b| count(x.at(b + 3) + (b / 3) as f64)));
        Some(Table { analysis: id.into(), title: "Logistic regressions".into(), columns, rows,
                     note: format!("Coefficients on the log-odds scale, B (SE); exp(B) is the odds ratio. \
                                    McFadden's R² = 1 − deviance / null deviance. {STARS}") })
    }
}

// ── APA number style

fn missing(x: f64) -> bool { !x.is_finite() }

/// Fixed decimals, with a true minus sign.
pub fn fixed(x: f64, dp: usize) -> String {
    if missing(x) { return "—".into(); }
    let s = format!("{:.*}", dp, x);
    // -0.00 is 0.00
    if s.starts_with('-') && s[1..].chars().all(|c| c == '0' || c == '.') { return s[1..].to_string(); }
    s.replacen('-', "−", 1)
}

/// A number that cannot exceed 1 (r, R², η², V): no leading zero.
pub fn unit(x: f64, dp: usize) -> String {
    let s = fixed(x, dp);
    if let Some(rest) = s.strip_prefix("0.") { format!(".{rest}") }
    else if let Some(rest) = s.strip_prefix("−0.") { format!("−.{rest}") }
    else { s }
}

/// p as APA writes it: three decimals, no leading zero, "< .001" below.
pub fn p_apa(p: f64) -> String {
    if missing(p) { return "—".into(); }
    if p < 0.001 { "< .001".into() } else { unit(p, 3) }
}

fn stars(p: f64) -> &'static str {
    if missing(p) { "" } else if p < 0.001 { "***" } else if p < 0.01 { "**" } else if p < 0.05 { "*" } else { "" }
}

/// Whole degrees of freedom as they are; Welch's fractional ones to 2 decimals.
fn dfs(df: f64) -> String { if missing(df) { "—".into() } else if df.fract() == 0.0 { format!("{df:.0}") } else { fixed(df, 2) } }

fn count(n: f64) -> String { if missing(n) { "—".into() } else { format!("{n:.0}") } }

/// A test value as typed: no trailing zeros.
fn short(x: f64) -> String { if x.fract() == 0.0 { count(x) } else { fixed(x, 2) } }

fn m_sd(m: f64, sd: f64) -> String { format!("{} ({})", fixed(m, 2), fixed(sd, 2)) }

fn ci(lo: f64, hi: f64) -> String { format!("[{}, {}]", fixed(lo, 2), fixed(hi, 2)) }

fn ci_unit(lo: f64, hi: f64) -> String { format!("[{}, {}]", unit(lo, 2), unit(hi, 2)) }

// ── writing tables out

fn esc(s: &str) -> String { s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;") }

/// The tables as one HTML page, APA style (rules above and below the head and
/// at the foot only, numbers centred); it opens in a browser and pastes into Word.
pub fn to_html(tables: &[Table], heading: &str) -> String {
    let mut h = String::from("<!doctype html>\n<html><head><meta charset=\"utf-8\">\n");
    h += &format!("<title>{}</title>\n", esc(heading));
    h += "<style>\nbody { font-family: 'Times New Roman', Times, serif; font-size: 12pt; margin: 2em; color: #000; background: #fff; }\n\
          .table { margin: 0 0 2.5em; }\n.num { font-weight: bold; margin: 0; }\n.title { font-style: italic; margin: 0.6em 0 0.4em; }\n\
          table { border-collapse: collapse; }\nth, td { padding: 0.25em 0.8em; text-align: center; white-space: nowrap; }\n\
          th:first-child, td:first-child { text-align: left; }\nthead tr:first-child th { border-top: 1px solid #000; }\n\
          thead th { border-bottom: 1px solid #000; font-weight: normal; }\ntbody tr:last-child td { border-bottom: 1px solid #000; }\n\
          .note { margin: 0.4em 0 0; max-width: 46em; }\n.note i { font-style: italic; }\n</style></head><body>\n";
    for (i, t) in tables.iter().enumerate() {
        h += &format!("<div class=\"table\">\n<p class=\"num\">Table {}</p>\n<p class=\"title\">{}</p>\n<table>\n<thead><tr>",
                      i + 1, esc(&t.title));
        for c in &t.columns { h += &format!("<th>{}</th>", italic_symbols(&esc(c))); }
        h += "</tr></thead>\n<tbody>\n";
        for r in &t.rows {
            h += "<tr>";
            for c in r { h += &format!("<td>{}</td>", esc(c)); }
            h += "</tr>\n";
        }
        h += &format!("</tbody>\n</table>\n<p class=\"note\"><i>Note.</i> {}</p>\n</div>\n", esc(&t.note));
    }
    h += "</body></html>\n";
    h
}

/// APA sets statistical symbols in italics.
fn italic_symbols(head: &str) -> String {
    const SYMBOLS: &[&str] = &["n", "N", "M", "SD", "t", "df", "p", "d", "F", "r", "B", "SE", "V"];
    let words: Vec<String> = head.split(' ').map(|w| {
        let core = w.trim_end_matches([':', ',', ')']).trim_start_matches('(');
        if SYMBOLS.contains(&core) { w.replacen(core, &format!("<i>{core}</i>"), 1) } else { w.to_string() }
    }).collect();
    words.join(" ")
}

/// One table as CSV: the title, the head, the rows and the note, so the file
/// is complete on its own in a spreadsheet.
pub fn to_csv(t: &Table) -> String {
    let cell = |s: &str| if s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() };
    let line = |cells: &[String]| cells.iter().map(|c| cell(c)).collect::<Vec<_>>().join(",");
    let mut out = format!("{}\n{}\n", cell(&t.title), line(&t.columns));
    for r in &t.rows { out += &line(r); out.push('\n'); }
    out += &format!("{}\n", cell(&format!("Note. {}", t.note)));
    out
}

/// One table as tab-separated text, for pasting into Word or a spreadsheet.
pub fn to_tsv(t: &Table, number: usize) -> String {
    let mut out = format!("Table {number}\n{}\n{}\n", t.title, t.columns.join("\t"));
    for r in &t.rows { out += &r.join("\t"); out.push('\n'); }
    out += &format!("Note. {}\n", t.note);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nums(values: &[f64], names: &[&str]) -> Numbers {
        Numbers { values: values.to_vec(), names: names.iter().map(|s| s.to_string()).collect() }
    }

    #[test]
    fn numbers_are_written_the_apa_way() {
        assert_eq!(p_apa(0.0004), "< .001");
        assert_eq!(p_apa(0.0347), ".035");
        assert_eq!(unit(-0.8677, 2), "−.87");
        assert_eq!(fixed(-3.7671, 2), "−3.77");
        assert_eq!(fixed(-0.001, 2), "0.00");
        assert_eq!(dfs(18.3322), "18.33");
        assert_eq!(dfs(29.0), "29");
        assert_eq!(fixed(f64::NAN, 2), "—");
    }

    #[test]
    fn t_tests_on_several_variables_make_one_table() {
        // mtcars: mpg and wt by am, as R2 returned them
        let mpg = nums(&[17.147368, 24.392308, 3.833966, 6.166504, 19.0, 13.0, -3.767123, 18.332252, 0.0013736, -11.280194, -3.209684],
                       &["0", "1", "0", "1", "0", "1", "", "", "", "", ""]);
        let wt = nums(&[3.768895, 2.411000, 0.777400, 0.616982, 19.0, 13.0, 5.493905, 29.234, 0.0000063, 1.36, 2.36],
                      &["0", "1", "0", "1", "0", "1", "", "", "", "", ""]);
        let l1 = vec!["mpg".to_string(), "am".to_string()];
        let l2 = vec!["wt".to_string(), "am".to_string()];
        let t = build("t_two", &[Entry { analysis: "t_two", labels: &l1, numbers: &mpg },
                                 Entry { analysis: "t_two", labels: &l2, numbers: &wt }]).unwrap();
        assert_eq!(t.title, "Two-group t-tests by am");
        assert_eq!(t.columns[1], "0: M (SD)");
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[0][..6], ["mpg", "17.15 (3.83)", "24.39 (6.17)", "−3.77", "18.33", ".001"]);
        assert_eq!(t.rows[1][5], "< .001");
        // d with the pooled SD: (17.147 - 24.392) / 4.902
        assert_eq!(t.rows[0][7], "−1.48");
    }

    #[test]
    fn a_bad_reply_leaves_its_row_out_and_an_empty_table_is_none() {
        let bad = nums(&[1.0, 2.0], &[]);
        let l = vec!["x".to_string(), "g".to_string()];
        assert!(build("t_two", &[Entry { analysis: "t_two", labels: &l, numbers: &bad }]).is_none());
        assert!(build_all(&[Entry { analysis: "hist", labels: &l, numbers: &bad }]).is_empty());
    }

    #[test]
    fn three_variables_correlate_as_a_matrix() {
        let l = |a: &str, b: &str| vec![a.to_string(), b.to_string()];
        let (ab, ac, bc) = (l("a", "b"), l("a", "c"), l("b", "c"));
        let n1 = nums(&[0.5, 30.0, 0.004, 3.1, 32.0], &[]);
        let n2 = nums(&[-0.2, 30.0, 0.27, -1.1, 32.0], &[]);
        let n3 = nums(&[0.9, 30.0, 0.0000001, 11.0, 32.0], &[]);
        let t = build("cor", &[Entry { analysis: "cor", labels: &ab, numbers: &n1 }, Entry { analysis: "cor", labels: &ac, numbers: &n2 },
                               Entry { analysis: "cor", labels: &bc, numbers: &n3 }]).unwrap();
        assert_eq!(t.columns, ["Variable", "1", "2"]);
        assert_eq!(t.rows[0], ["1. a", "—", ""]);
        assert_eq!(t.rows[1], ["2. b", ".50**", "—"]);
        assert_eq!(t.rows[2], ["3. c", "−.20", ".90***"]);
        assert!(t.note.starts_with("n = 32."));
    }

    #[test]
    fn models_sit_side_by_side() {
        let a = nums(&[37.23, -3.88, -0.03, 1.60, 0.63, 0.009, 2.6e-20, 1.1e-6, 0.0015, 0.827, 0.815, 69.2, 29.0],
                     &["(Intercept)", "wt", "hp", "", "", "", "", "", "", "", "", "", ""]);
        let b = nums(&[37.29, -5.34, 1.88, 0.56, 8.2e-19, 1.3e-10, 0.753, 0.745, 91.4, 30.0], &["(Intercept)", "wt"]);
        let (la, lb) = (vec!["mpg".to_string(), "wt + hp".to_string()], vec!["mpg".to_string(), "wt".to_string()]);
        let t = build("lm", &[Entry { analysis: "lm", labels: &la, numbers: &a }, Entry { analysis: "lm", labels: &lb, numbers: &b }]).unwrap();
        assert_eq!(t.columns, ["Predictor", "Model 1: mpg", "Model 2: mpg"]);
        assert_eq!(t.rows[1], ["wt", "−3.88*** (0.63)", "−5.34*** (0.56)"]);
        assert_eq!(t.rows[2], ["hp", "−0.03** (0.01)", ""]);
        assert_eq!(t.rows[3], ["R²", ".827", ".753"]);
        assert_eq!(t.rows[5], ["F", "69.20 (2, 29)", "91.40 (1, 30)"]);
        assert_eq!(t.rows[6], ["N", "32", "32"]);
    }

    #[test]
    fn tables_write_out_as_html_csv_and_text() {
        let t = Table { analysis: "x".into(), title: "A, \"b\"".into(), columns: vec!["Variable".into(), "M (SD)".into()],
                        rows: vec![vec!["a<b".into(), "1.00 (2.00)".into()]], note: "n.".into() };
        let h = to_html(std::slice::from_ref(&t), "Tables");
        assert!(h.contains("<p class=\"num\">Table 1</p>") && h.contains("<td>a&lt;b</td>") && h.contains("<th><i>M</i> (<i>SD</i>)</th>"));
        assert_eq!(to_csv(&t), "\"A, \"\"b\"\"\"\nVariable,M (SD)\na<b,1.00 (2.00)\nNote. n.\n");
        assert!(to_tsv(&t, 2).starts_with("Table 2\nA, \"b\"\nVariable\tM (SD)\n"));
    }
}
