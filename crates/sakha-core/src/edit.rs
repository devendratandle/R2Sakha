//! The Data tab's edits, as R2 code. Typing in a cell, computing a variable,
//! recoding, filtering, sorting and deleting each become a line R2 runs and
//! the session script keeps, so edited data can always be rebuilt from its
//! file. Nothing is changed outside R2.

use crate::gview::quote;
use crate::import::clean_name;
use crate::profile::{col_ref, Kind};

/// One edit: what the script's comment says, and the code.
#[derive(Debug, Clone, PartialEq)]
pub struct Edit { pub title: String, pub code: Vec<String> }

fn edit(title: String, line: String) -> Edit { Edit { title, code: vec![line] } }

/// What was typed, as an R2 value for a column of this kind. Nothing typed
/// (or `NA`) is a missing value.
pub fn literal(typed: &str, kind: Kind) -> Result<String, String> {
    let t = typed.trim();
    if t.is_empty() || t == "NA" { return Ok("NA".into()); }
    match kind {
        Kind::Number => t.parse::<f64>().map(|_| t.to_string()).map_err(|_| format!("\"{t}\" is not a number.")),
        _ => Ok(quote(t)),
    }
}

/// Row `row` (from 1) of `col` gets the typed value. A category takes a value
/// it has not had before as a new level.
pub fn cell(df: &str, row: usize, col: &str, kind: Kind, typed: &str) -> Result<Edit, String> {
    let v = literal(typed, kind)?;
    let c = col_ref(df, col);
    let mut code = Vec::new();
    if kind == Kind::Category && v != "NA" {
        code.push(format!("{c} <- factor({c}, levels = union(levels({c}), {v}))"));
    }
    code.push(format!("{df}[{row}, {}] <- {v}", quote(col)));
    let shown = if v == "NA" { "NA" } else { typed.trim() };
    Ok(Edit { title: format!("{col}, row {row}: {shown}"), code })
}

/// A new variable from a formula over the others: `bmi` from `weight / height^2`.
pub fn compute(df: &str, name: &str, formula: &str) -> Result<Edit, String> {
    let (name, f) = (name.trim(), formula.trim());
    if name.is_empty() { return Err("The new variable needs a name.".into()); }
    if clean_name(name) != name { return Err(format!("Use a plain name, such as {}.", clean_name(name))); }
    if f.is_empty() { return Err("Type a formula, such as (a + b) / 2.".into()); }
    Ok(edit(format!("{name} computed as {f}"), format!("{df}${name} <- with({df}, {f})")))
}

/// A variable recoded into `new` (which may be the variable itself).
/// A number is cut into groups at the points given (`10, 20` gives three
/// groups); a category's values are replaced as `old = new, old2 = new`.
pub fn recode(df: &str, col: &str, kind: Kind, new: &str, rules: &str) -> Result<Edit, String> {
    let new = new.trim();
    if new.is_empty() || clean_name(new) != new { return Err(format!("Use a plain name, such as {}.", clean_name(if new.is_empty() { col } else { new }))); }
    let (c, n) = (col_ref(df, col), format!("{df}${new}"));
    if kind == Kind::Number {
        let mut cuts: Vec<f64> = Vec::new();
        for p in rules.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            cuts.push(p.parse().map_err(|_| format!("\"{p}\" is not a number. Give cut points such as 10, 20."))?);
        }
        if cuts.is_empty() { return Err("Give the points to cut at, such as 10, 20.".into()); }
        cuts.sort_by(f64::total_cmp);
        let points: Vec<String> = cuts.iter().map(|x| x.to_string()).collect();
        return Ok(edit(format!("{col} cut at {} into {new}", points.join(", ")),
                       format!("{n} <- cut({c}, breaks = c(-Inf, {}, Inf))", points.join(", "))));
    }
    let mut pairs: Vec<(String, String)> = Vec::new();
    for p in rules.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let Some((old, to)) = p.split_once('=') else { return Err(format!("\"{p}\" needs an = sign: old = new.")) };
        pairs.push((old.trim().to_string(), to.trim().to_string()));
    }
    if pairs.is_empty() { return Err("Give the changes as old = new, old2 = new.".into()); }
    // nested, so that a = b, b = c does not turn every a into c
    let mut expr = format!("as.character({c})");
    for (old, to) in pairs.iter().rev() {
        expr = format!("ifelse({c} == {}, {}, {expr})", quote(old), quote(to));
    }
    let said: Vec<String> = pairs.iter().map(|(o, t)| format!("{o} = {t}")).collect();
    Ok(edit(format!("{col} recoded into {new}: {}", said.join(", ")), format!("{n} <- factor({expr})")))
}

/// Keep only the rows where the condition holds: `age >= 18 & group == "a"`.
pub fn filter(df: &str, condition: &str) -> Result<Edit, String> {
    let c = condition.trim();
    if c.is_empty() { return Err("Type a condition, such as age >= 18.".into()); }
    Ok(edit(format!("rows kept where {c}"), format!("{df} <- {df}[which(with({df}, {c})), ]")))
}

/// The rows in the order of one variable.
pub fn sort(df: &str, col: &str, descending: bool) -> Edit {
    let c = col_ref(df, col);
    let (how, arg) = if descending { ("largest first", ", decreasing = TRUE") } else { ("smallest first", "") };
    edit(format!("sorted by {col}, {how}"), format!("{df} <- {df}[order({c}{arg}), ]"))
}

pub fn delete_variable(df: &str, col: &str) -> Edit {
    edit(format!("{col} deleted"), format!("{} <- NULL", col_ref(df, col)))
}

/// Row `row` (from 1) removed.
pub fn delete_row(df: &str, row: usize) -> Edit {
    edit(format!("row {row} deleted"), format!("{df} <- {df}[-{row}, ]"))
}

/// An empty row at the end, to type into.
pub fn add_row(df: &str) -> Edit {
    edit("a row added".into(), format!("{df}[nrow({df}) + 1, ] <- NA"))
}

/// The lines that bring the data back as it was first loaded, for Undo. One
/// of R2's own datasets (`mtcars <- mtcars`) is found again once the edited
/// copy is out of the way.
pub fn reload(df: &str, load: &str) -> Vec<String> {
    let own = load.split_once("<-").is_some_and(|(a, b)| a.trim() == df && b.trim() == df);
    if own { vec![format!("rm({df})"), load.to_string()] } else { vec![load.to_string()] }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_cell_becomes_one_line() {
        assert_eq!(cell("d", 3, "score", Kind::Number, " 4.5 ").unwrap().code, ["d[3, \"score\"] <- 4.5"]);
        assert_eq!(cell("d", 3, "score", Kind::Number, "").unwrap().code, ["d[3, \"score\"] <- NA"]);
        assert!(cell("d", 3, "score", Kind::Number, "tall").is_err());
        assert_eq!(cell("d", 1, "name", Kind::Text, "a \"b\"").unwrap().code, ["d[1, \"name\"] <- \"a \\\"b\\\"\""]);
        assert_eq!(cell("d", 2, "g", Kind::Category, "new").unwrap().code,
                   ["d$g <- factor(d$g, levels = union(levels(d$g), \"new\"))", "d[2, \"g\"] <- \"new\""]);
    }

    #[test]
    fn the_other_edits() {
        assert_eq!(compute("d", "bmi", "w / h^2").unwrap().code, ["d$bmi <- with(d, w / h^2)"]);
        assert!(compute("d", "my var", "1").is_err());
        assert_eq!(recode("d", "age", Kind::Number, "band", "40, 20").unwrap().code, ["d$band <- cut(d$age, breaks = c(-Inf, 20, 40, Inf))"]);
        assert_eq!(recode("d", "g", Kind::Category, "g2", "a = b, b = c").unwrap().code,
                   ["d$g2 <- factor(ifelse(d$g == \"a\", \"b\", ifelse(d$g == \"b\", \"c\", as.character(d$g))))"]);
        assert!(recode("d", "g", Kind::Category, "g2", "a b").is_err());
        assert_eq!(filter("d", "age >= 18").unwrap().code, ["d <- d[which(with(d, age >= 18)), ]"]);
        assert_eq!(sort("d", "age", true).code, ["d <- d[order(d$age, decreasing = TRUE), ]"]);
        assert_eq!(delete_variable("d", "my col").code, ["d$`my col` <- NULL"]);
        assert_eq!(delete_row("d", 4).code, ["d <- d[-4, ]"]);
    }
}
