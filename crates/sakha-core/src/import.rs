//! Bringing data in, and changing a column's attributes: the R2 code behind
//! Sakha's Data menu. Only options R2 honours are offered. `read.csv` currently
//! ignores `na.strings` and `dec` without an error, so no dialog asks for them.

use crate::profile::col_ref;

/// A field separator a text file can use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sep { Comma, Tab, Semicolon, Space }

impl Sep {
    pub const ALL: [Sep; 4] = [Sep::Comma, Sep::Tab, Sep::Semicolon, Sep::Space];

    pub fn label(self) -> &'static str {
        match self { Sep::Comma => "Comma", Sep::Tab => "Tab", Sep::Semicolon => "Semicolon", Sep::Space => "Space" }
    }

    fn ch(self) -> char {
        match self { Sep::Comma => ',', Sep::Tab => '\t', Sep::Semicolon => ';', Sep::Space => ' ' }
    }

    fn code(self) -> &'static str {
        match self { Sep::Comma => ",", Sep::Tab => "\\t", Sep::Semicolon => ";", Sep::Space => " " }
    }
}

/// The separator the file's first line uses most, ignoring quoted text.
/// A tab, semicolon or comma wins over a space, which plain text is full of.
pub fn guess_separator(first_line: &str) -> Sep {
    let mut quoted = false;
    let unquoted: String = first_line.chars().filter(|c| { if *c == '"' { quoted = !quoted; } !quoted }).collect();
    let count = |s: Sep| unquoted.matches(s.ch()).count();
    [Sep::Tab, Sep::Semicolon, Sep::Comma].into_iter().filter(|s| count(*s) > 0).max_by_key(|s| count(*s))
        .unwrap_or(if count(Sep::Space) > 0 { Sep::Space } else { Sep::Comma })
}

/// A valid R name from any text: other characters become `.`, and a name must
/// start with a letter.
pub fn clean_name(raw: &str) -> String {
    let mut name: String = raw.trim().chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '.' { c } else { '.' }).collect();
    if !name.chars().next().is_some_and(|c| c.is_alphabetic()) { name.insert(0, 'X'); }
    name
}

/// `name <- read.csv("path", ...)`, naming only what differs from the defaults.
pub fn import_line(name: &str, path: &str, header: bool, sep: Sep) -> String {
    let mut args = format!("\"{path}\"");
    if !header { args.push_str(", header = FALSE"); }
    if sep != Sep::Comma { args.push_str(&format!(", sep = \"{}\"", sep.code())); }
    format!("{name} <- read.csv({args})")
}

/// `name <- read.parquet("path")`.
pub fn parquet_line(name: &str, path: &str) -> String { format!("{name} <- read.parquet(\"{path}\")") }

/// Column `index` (from 0) gets a new name.
pub fn rename_code(df: &str, index: usize, new: &str) -> String {
    format!("names({df})[{}] <- \"{}\"", index + 1, new.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The column becomes a category.
pub fn to_category_code(df: &str, col: &str) -> String {
    let c = col_ref(df, col);
    format!("{c} <- factor({c})")
}

/// The column becomes a number: a category's labels are read as numbers, not
/// its internal codes, so `factor(c(10, 20))` gives 10 and 20. Text that is not
/// a number becomes NA.
pub fn to_number_code(df: &str, col: &str) -> String {
    let c = col_ref(df, col);
    format!("{c} <- as.numeric(as.character({c}))")
}

/// Why a new column name will not do, if so.
pub fn rename_problem(new: &str, existing: &[&str]) -> Option<String> {
    if new.trim().is_empty() { return Some("A variable needs a name.".into()); }
    if existing.contains(&new) { return Some(format!("There is already a variable called {new}.")); }
    None
}

/// The names in the text `data()` prints: "Available datasets: iris, mtcars".
pub fn dataset_names(printed: &str) -> Vec<String> {
    printed.split_once(':').map(|(_, list)| list).unwrap_or("").split(',')
        .map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

/// R2's datasets that do not match R's yet, kept off the list. R2's `airquality`
/// has 30 rows; R's has 153.
pub const DATASETS_HELD_BACK: [&str; 1] = ["airquality"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_separator_is_guessed_from_the_first_line() {
        assert_eq!(guess_separator("id,score,grp"), Sep::Comma);
        assert_eq!(guess_separator("id\tscore"), Sep::Tab);
        assert_eq!(guess_separator("id;score;a,b"), Sep::Semicolon);
        assert_eq!(guess_separator("id score"), Sep::Space);
        assert_eq!(guess_separator("\"a, b, c\";x"), Sep::Semicolon, "quoted text does not count");
        assert_eq!(guess_separator("single"), Sep::Comma);
    }

    #[test]
    fn lines_name_only_what_differs_from_the_defaults() {
        assert_eq!(import_line("d", "a.csv", true, Sep::Comma), "d <- read.csv(\"a.csv\")");
        assert_eq!(import_line("d", "a.txt", false, Sep::Tab), "d <- read.csv(\"a.txt\", header = FALSE, sep = \"\\t\")");
        assert_eq!(parquet_line("d", "a.parquet"), "d <- read.parquet(\"a.parquet\")");
    }

    #[test]
    fn names_are_cleaned_and_checked() {
        assert_eq!(clean_name("my data-2024"), "my.data.2024");
        assert_eq!(clean_name("2024 trial"), "X2024.trial");
        assert_eq!(rename_problem("", &["a"]).unwrap(), "A variable needs a name.");
        assert!(rename_problem("a", &["a"]).unwrap().contains("already"));
        assert_eq!(rename_problem("b", &["a"]), None);
    }

    #[test]
    fn attribute_code_is_plain_r2() {
        assert_eq!(rename_code("d", 1, "new name"), "names(d)[2] <- \"new name\"");
        assert_eq!(to_category_code("d", "a b"), "d$`a b` <- factor(d$`a b`)");
        assert_eq!(to_number_code("d", "x"), "d$x <- as.numeric(as.character(d$x))");
    }

    #[test]
    fn dataset_names_come_from_what_data_prints() {
        assert_eq!(dataset_names("Available datasets: iris, mtcars, airquality\n"), ["iris", "mtcars", "airquality"]);
        assert!(dataset_names("nothing here").is_empty());
    }
}
