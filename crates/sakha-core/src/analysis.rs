//! Sakha's menu. Each [`Analysis`] says what it asks for ([`Role`]s), writes
//! the R2 code that runs it ([`Analysis::build`]) and reads its result in
//! plain words ([`Analysis::says`]). The user picks variables, never types
//! code; the code is still there to read, rerun and export.
//!
//! Only analyses whose R2 numbers were checked against GNU R are on the menu
//! (tests/real_r2.rs). Two are shaped by that check:
//!
//! * Chi-square builds its table with `matrix()` and `sum()`, because
//!   `table(a, b)` currently keeps only `a` in R2.
//! * Mann-Whitney is left out: `wilcox.test` currently gives a different
//!   p-value from GNU R.

use crate::profile::{col_ref, fmt_digits, name_ref, ColumnProfile, Kind};

/// What a role accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accepts {
    Number,
    /// A category, or a number that looks like one.
    Groups,
    /// A category with its levels known.
    Category,
    /// Number or category (a model's predictors).
    Any,
    /// A number typed by the user, with this default (not a variable).
    Typed(&'static str),
}

impl Accepts {
    pub fn takes(self, c: &ColumnProfile) -> bool {
        match self {
            Accepts::Number => c.kind == Kind::Number,
            Accepts::Groups => c.groups(),
            Accepts::Category => c.kind == Kind::Category,
            Accepts::Any => c.kind != Kind::Text,
            Accepts::Typed(_) => false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Role {
    pub label: &'static str,
    pub accepts: Accepts,
    /// Several variables may be picked (a model's predictors).
    pub many: bool,
}

const fn one(label: &'static str, accepts: Accepts) -> Role { Role { label, accepts, many: false } }
const fn many(label: &'static str, accepts: Accepts) -> Role { Role { label, accepts, many: true } }

/// What the analysis's result reads like, once its statistic is back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reading { None, Mean, Groups, Pairs, Correlation, Association, Fit }

#[derive(Debug, Clone, Copy)]
pub struct Analysis {
    /// The menu action is `sakha.run.<id>`.
    pub id: &'static str,
    pub menu: &'static str,
    pub title: &'static str,
    /// One line for the dialog: what it answers.
    pub question: &'static str,
    pub roles: &'static [Role],
    reading: Reading,
}

/// The whole menu, in order.
pub const ANALYSES: &[Analysis] = &[
    Analysis { id: "freq", menu: "Summarize", title: "Frequency table", question: "How often does each value occur?",
               roles: &[one("Variable", Accepts::Category)], reading: Reading::None },
    Analysis { id: "group_summary", menu: "Summarize", title: "Summary by group", question: "How does a number differ across groups, before any test?",
               roles: &[one("Measure", Accepts::Number), one("Group by", Accepts::Groups)], reading: Reading::None },

    Analysis { id: "t_one", menu: "Compare", title: "One-sample t-test", question: "Is the average different from a value you expect?",
               roles: &[one("Measure", Accepts::Number), one("Expected mean", Accepts::Typed("0"))], reading: Reading::Mean },
    Analysis { id: "t_two", menu: "Compare", title: "Two-group t-test", question: "Do two groups have different averages? (Welch, no equal-variance assumption)",
               roles: &[one("Measure", Accepts::Number), one("Two groups", Accepts::Groups)], reading: Reading::Groups },
    Analysis { id: "t_paired", menu: "Compare", title: "Paired t-test", question: "Did the same subjects change between two measurements?",
               roles: &[one("First", Accepts::Number), one("Second", Accepts::Number)], reading: Reading::Pairs },
    Analysis { id: "anova", menu: "Compare", title: "One-way ANOVA", question: "Do the averages differ across several groups?",
               roles: &[one("Measure", Accepts::Number), one("Groups", Accepts::Groups)], reading: Reading::Groups },

    Analysis { id: "cor", menu: "Relate", title: "Correlation", question: "Do two numbers move together? (Pearson)",
               roles: &[one("First", Accepts::Number), one("Second", Accepts::Number)], reading: Reading::Correlation },
    Analysis { id: "chisq", menu: "Relate", title: "Chi-square test", question: "Are two categories related?",
               roles: &[one("Rows", Accepts::Category), one("Columns", Accepts::Category)], reading: Reading::Association },
    Analysis { id: "lm", menu: "Relate", title: "Linear regression", question: "How well do these variables predict a number?",
               roles: &[one("Outcome", Accepts::Number), many("Predictors", Accepts::Any)], reading: Reading::Fit },
    Analysis { id: "glm", menu: "Relate", title: "Logistic regression", question: "How do these variables predict a 0/1 outcome?",
               roles: &[one("Outcome (0/1)", Accepts::Number), many("Predictors", Accepts::Any)], reading: Reading::None },

    Analysis { id: "hist", menu: "Plot", title: "Histogram", question: "How are the values spread?",
               roles: &[one("Variable", Accepts::Number)], reading: Reading::None },
    Analysis { id: "density", menu: "Plot", title: "Density curve with 95% CI", question: "What is the shape of the distribution, and how sure is that shape?",
               roles: &[one("Variable", Accepts::Number)], reading: Reading::None },
    Analysis { id: "box", menu: "Plot", title: "Box plot by group", question: "How do the groups' spreads compare?",
               roles: &[one("Measure", Accepts::Number), one("Group by", Accepts::Groups)], reading: Reading::None },
    Analysis { id: "scatter", menu: "Plot", title: "Scatter plot", question: "How do two numbers relate, point by point?",
               roles: &[one("Across (x)", Accepts::Number), one("Up (y)", Accepts::Number)], reading: Reading::None },
    Analysis { id: "bar", menu: "Plot", title: "Bar chart", question: "How large is each group?",
               roles: &[one("Variable", Accepts::Category)], reading: Reading::None },
];

pub fn find(id: &str) -> Option<&'static Analysis> { ANALYSES.iter().find(|a| a.id == id) }

/// The R2 code of one run.
#[derive(Debug, Clone, PartialEq)]
pub struct Built {
    /// What the result is called: "Two-group t-test: score by grp".
    pub title: String,
    /// One statement per line, run through `eval`; output and plot show.
    pub code: Vec<String>,
    /// Asked quietly afterwards through `value`, for the plain-words line.
    pub statistic: Option<String>,
    /// The variables picked, role by role (a model's predictors joined by
    /// " + "): what a table's row is called.
    pub labels: Vec<String>,
    /// Asked quietly afterwards through `value`: the numbers a publishable
    /// table reads, as one numeric vector laid out as [`crate::tables`] expects.
    pub numbers: Option<String>,
}

impl Analysis {
    /// The code for these picks: one list per role, holding columns of `cols`
    /// (a `Typed` role's text is in `typed`). `Err` says what is missing.
    pub fn build(&self, df: &str, cols: &[ColumnProfile], picks: &[Vec<usize>], typed: &str) -> Result<Built, String> {
        let pick = |role: usize| -> Result<&ColumnProfile, String> {
            picks.get(role).and_then(|p| p.first()).and_then(|&i| cols.get(i))
                .ok_or_else(|| format!("Choose: {}", self.roles[role].label))
        };
        let c = |role: usize| pick(role).map(|p| col_ref(df, &p.name));
        let n = |role: usize| pick(role).map(|p| name_ref(&p.name));
        let shown = |role: usize| pick(role).map(|p| p.name.clone());
        let terms = |role: usize| -> Result<String, String> {
            let names: Vec<String> = picks.get(role).into_iter().flatten()
                .filter_map(|&i| cols.get(i)).map(|p| name_ref(&p.name)).collect();
            if names.is_empty() { Err(format!("Choose: {}", self.roles[role].label)) } else { Ok(names.join(" + ")) }
        };
        let labels: Vec<String> = (0..self.roles.len()).map(|r| picks.get(r).into_iter().flatten()
            .filter_map(|&i| cols.get(i)).map(|p| p.name.clone()).collect::<Vec<_>>().join(" + ")).collect();
        let numbers = crate::tables::numbers_code(self.id, df, cols, picks, typed);
        let (title, code, statistic): (String, Vec<String>, Option<String>) = match self.id {
            "freq" => {
                let x = c(0)?;
                (format!("Frequencies of {}", shown(0)?),
                 vec![format!("table({x})"), format!("round(100 * table({x}) / sum(table({x})), 1)")], None)
            }
            "group_summary" => {
                let (y, g) = (c(0)?, c(1)?);
                (format!("{} by {}", shown(0)?, shown(1)?),
                 vec![format!("tapply({y}, {g}, length)"), format!("tapply({y}, {g}, mean, na.rm = TRUE)"),
                      format!("tapply({y}, {g}, sd, na.rm = TRUE)")], None)
            }
            "t_one" => {
                let mu: f64 = typed.trim().parse().map_err(|_| "The expected mean must be a number".to_string())?;
                let y = c(0)?;
                let call = format!("t.test({y}, mu = {mu})");
                (format!("One-sample t-test: {} against {mu}", shown(0)?), vec![call.clone()], Some(format!("{call}$p.value")))
            }
            "t_two" => {
                let call = format!("t.test({} ~ {}, data = {df})", n(0)?, n(1)?);
                (format!("Two-group t-test: {} by {}", shown(0)?, shown(1)?), vec![call.clone()], Some(format!("{call}$p.value")))
            }
            "t_paired" => {
                let call = format!("t.test({}, {}, paired = TRUE)", c(0)?, c(1)?);
                if picks[0] == picks[1] { return Err("Choose two different variables".into()); }
                (format!("Paired t-test: {} and {}", shown(0)?, shown(1)?), vec![call.clone()], Some(format!("{call}$p.value")))
            }
            "anova" => {
                let call = format!("aov({} ~ {}, data = {df})", n(0)?, n(1)?);
                (format!("One-way ANOVA: {} by {}", shown(0)?, shown(1)?),
                 vec![call.clone()], Some(format!("anova(lm({} ~ factor({}), data = {df}))$p.value", n(0)?, n(1)?)))
            }
            "cor" => {
                let call = format!("cor.test({}, {})", c(0)?, c(1)?);
                if picks[0] == picks[1] { return Err("Choose two different variables".into()); }
                (format!("Correlation: {} and {}", shown(0)?, shown(1)?), vec![call.clone()], Some(format!("{call}$p.value")))
            }
            "chisq" => {
                let (a, b) = (pick(0)?, pick(1)?);
                if picks[0] == picks[1] { return Err("Choose two different variables".into()); }
                if a.levels.len() < 2 || b.levels.len() < 2 { return Err("Each variable needs at least two groups".into()); }
                let call = chisq_call(df, a, b);
                (format!("Chi-square: {} and {}", a.name, b.name), vec![call.clone()], Some(format!("{call}$p.value")))
            }
            "lm" => {
                let model = format!("lm({} ~ {}, data = {df})", n(0)?, terms(1)?);
                (format!("Linear regression of {}", shown(0)?), vec![format!("summary({model})")],
                 Some(format!("summary({model})$r.squared")))
            }
            "glm" => {
                let model = format!("glm({} ~ {}, family = binomial, data = {df})", n(0)?, terms(1)?);
                (format!("Logistic regression of {}", shown(0)?), vec![format!("summary({model})")], None)
            }
            "hist" => (format!("Histogram of {}", shown(0)?), vec![format!("hist({})", c(0)?)], None),
            "density" => (format!("Density of {} with 95% CI", shown(0)?), density_code(&c(0)?, &shown(0)?), None),
            "box" => (format!("Box plot: {} by {}", shown(0)?, shown(1)?),
                      vec![format!("boxplot({} ~ {}, data = {df})", n(0)?, n(1)?)], None),
            "scatter" => (format!("Scatter plot: {} and {}", shown(1)?, shown(0)?), vec![format!("plot({}, {})", c(0)?, c(1)?)], None),
            "bar" => (format!("Bar chart of {}", shown(0)?), vec![format!("barplot(table({}))", c(0)?)], None),
            other => return Err(format!("unknown analysis '{other}'")),
        };
        Ok(Built { title, code, statistic, labels, numbers })
    }

    /// How to read a result with no statistic behind it.
    pub fn note(&self) -> Option<&'static str> {
        (self.id == "density").then_some("Solid line: the density estimate. Dashed lines: its pointwise 95% confidence band (normal approximation, ignoring smoothing bias), wider where the curve is high and when there are few values.")
    }

    /// The result in plain words, from the statistic R2 returned.
    pub fn says(&self, stat: f64) -> Option<String> {
        if stat.is_nan() { return None; }
        let clear = stat < 0.05;
        let p = fmt_p(stat);
        Some(match self.reading {
            Reading::None => return None,
            Reading::Mean => if clear { format!("{p}: the average differs from the expected value (below the 5% level).") }
                             else { format!("{p}: no clear evidence that the average differs from the expected value.") },
            Reading::Groups => if clear { format!("{p}: the group averages differ (below the 5% level).") }
                               else { format!("{p}: no clear evidence that the group averages differ.") },
            Reading::Pairs => if clear { format!("{p}: the two measurements differ on average (below the 5% level).") }
                              else { format!("{p}: no clear evidence of a change between the two measurements.") },
            Reading::Correlation => if clear { format!("{p}: the two variables are correlated (below the 5% level).") }
                                    else { format!("{p}: no clear evidence of a correlation.") },
            Reading::Association => if clear { format!("{p}: the two categories are related (below the 5% level).") }
                                    else { format!("{p}: no clear evidence that the two categories are related.") },
            Reading::Fit => format!("The model explains {:.1}% of the variation in the outcome (R-squared).", 100.0 * stat),
        })
    }
}

/// A density curve with its pointwise 95% band: f(x) +/- 1.96 se, with
/// se^2 = f(x) R(K) / (n h) and R(K) = 1 / (2 sqrt(pi)) for the Gaussian kernel. R2's
/// `density()` returns only `x` and `y`, so the bandwidth is written out: it is
/// R's `bw.nrd0`, the one `density()` uses. Names starting with `.` stay out of
/// the Environment pane. R2's `plot()` ignores `ylim`, so an invisible curve at the
/// band's upper limit sets the axis, and the three lines go on top.
fn density_code(col: &str, name: &str) -> Vec<String> {
    let title = name.replace('\\', "\\\\").replace('"', "\\\"");
    vec![
        format!(".x <- {col}[!is.na({col})]"),
        ".d <- density(.x)".to_string(),
        ".bw <- 0.9 * min(sd(.x), IQR(.x) / 1.34) * length(.x)^(-0.2)".to_string(),
        ".se <- sqrt(.d$y / (2 * sqrt(pi)) / (length(.x) * .bw))".to_string(),
        ".hi <- .d$y + 1.96 * .se".to_string(),
        ".lo <- pmax(.d$y - 1.96 * .se, 0)".to_string(),
        format!("plot(.d$x, .hi, type = \"l\", col = \"white\", main = \"Density of {title}\", xlab = \"{title}\", ylab = \"Density\")"),
        "lines(.d$x, .d$y)".to_string(),
        "lines(.d$x, .hi, lty = 2)".to_string(),
        "lines(.d$x, .lo, lty = 2)".to_string(),
    ]
}

/// `chisq.test` on a contingency table written out as a matrix.
pub(crate) fn chisq_call(df: &str, a: &ColumnProfile, b: &ColumnProfile) -> String {
    let (ca, cb) = (col_ref(df, &a.name), col_ref(df, &b.name));
    let quote = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    let mut cells = Vec::new();
    for lb in &b.levels {
        for la in &a.levels {
            cells.push(format!("sum({ca} == {} & {cb} == {}, na.rm = TRUE)", quote(&la.0), quote(&lb.0)));
        }
    }
    format!("chisq.test(matrix(c({}), nrow = {}))", cells.join(", "), a.levels.len())
}

/// A p-value as a sentence opens with it.
pub fn fmt_p(p: f64) -> String {
    if p < 0.0001 { "p < 0.0001".into() } else { format!("p = {}", fmt_digits(p, 3)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cols() -> Vec<ColumnProfile> {
        let mut grp = ColumnProfile::new("grp", Kind::Category);
        grp.levels = vec![("a".into(), 5), ("b".into(), 5)];
        vec![ColumnProfile::new("a b", Kind::Number), ColumnProfile::new("x", Kind::Number), grp]
    }

    #[test]
    fn code_names_the_picks_and_quotes_odd_names() {
        let c = cols();
        let b = find("t_two").unwrap().build("d", &c, &[vec![0], vec![2]], "").unwrap();
        assert_eq!(b.code, vec!["t.test(`a b` ~ grp, data = d)"]);
        assert_eq!(b.statistic.as_deref(), Some("t.test(`a b` ~ grp, data = d)$p.value"));
        let lm = find("lm").unwrap().build("d", &c, &[vec![1], vec![0, 2]], "").unwrap();
        assert_eq!(lm.code, vec!["summary(lm(x ~ `a b` + grp, data = d))"]);
    }

    #[test]
    fn missing_picks_and_bad_values_say_what_is_wrong() {
        let c = cols();
        assert_eq!(find("hist").unwrap().build("d", &c, &[vec![]], "").unwrap_err(), "Choose: Variable");
        assert_eq!(find("cor").unwrap().build("d", &c, &[vec![], vec![]], "").unwrap_err(), "Choose: First");
        assert!(find("t_one").unwrap().build("d", &c, &[vec![0], vec![]], "abc").unwrap_err().contains("number"));
        assert!(find("cor").unwrap().build("d", &c, &[vec![0], vec![0]], "").unwrap_err().contains("different"));
    }

    #[test]
    fn roles_accept_the_right_columns() {
        let c = cols();
        assert!(Accepts::Number.takes(&c[0]) && !Accepts::Number.takes(&c[2]));
        assert!(Accepts::Groups.takes(&c[2]) && !Accepts::Groups.takes(&c[0]));
        let mut dose = ColumnProfile::new("dose", Kind::Number);
        dose.looks_categorical = true;
        assert!(Accepts::Groups.takes(&dose) && !Accepts::Category.takes(&dose));
    }

    #[test]
    fn results_read_in_plain_words() {
        assert_eq!(fmt_p(0.0000001), "p < 0.0001");
        assert_eq!(fmt_p(0.8880494), "p = 0.888");
        assert!(find("t_two").unwrap().says(0.01).unwrap().contains("differ (below"));
        assert!(find("t_two").unwrap().says(0.5).unwrap().contains("no clear evidence"));
        assert!(find("lm").unwrap().says(0.1884).unwrap().contains("18.8%"));
        assert_eq!(find("hist").unwrap().says(0.5), None);
        assert_eq!(find("t_two").unwrap().says(f64::NAN), None);
    }
}
