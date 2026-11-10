//! Which lines Ctrl+Enter runs: the whole top-level statement under the
//! cursor, not just its line.
//!
//! A statement continues onto the next line while a bracket is open, a
//! string is open, or the line ends with a binary operator or a comma
//! (`x <- a +` / `df |>` / `f(a,`). Blank and comment-only lines belong to
//! no statement. Strings and comments are skipped while counting, so a
//! `(` inside `"..."` or after `#` does not open anything.

/// Top-level statements as inclusive `(first_line, last_line)` pairs.
pub fn statements(lines: &[String]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut depth: i32 = 0;
    let mut quote: Option<char> = None;
    for (n, line) in lines.iter().enumerate() {
        let (code_end, d, q) = scan(line, depth, quote);
        depth = d;
        quote = q;
        let code = line.chars().take(code_end).collect::<String>();
        let blank = code.trim().is_empty();
        if start.is_none() {
            if blank && quote.is_none() { continue; }
            start = Some(n);
        }
        let continues = quote.is_some() || depth > 0 || ends_with_operator(code.trim_end());
        if !continues {
            out.push((start.take().unwrap(), n));
            depth = 0;
        }
    }
    if let Some(s) = start { out.push((s, lines.len() - 1)); }
    out
}

/// The statement containing `line`; on a blank or comment line, the next
/// statement below it (or the last one above, at the end of the file).
pub fn statement_at(lines: &[String], line: usize) -> Option<(usize, usize)> {
    let all = statements(lines);
    all.iter().copied().find(|&(a, b)| a <= line && line <= b)
        .or_else(|| all.iter().copied().find(|&(a, _)| a > line))
        .or_else(|| all.last().copied())
}

/// Bracket depth and open quote after `line`, starting from the given
/// state, plus the column where a `#` comment starts (or the line length).
fn scan(line: &str, mut depth: i32, mut quote: Option<char>) -> (usize, i32, Option<char>) {
    let c: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if let Some(q) = quote {
            if ch == '\\' { i += 2; continue; }
            if ch == q { quote = None; }
            i += 1;
            continue;
        }
        match ch {
            '#' => return (i, depth, None),
            '"' | '\'' | '`' => quote = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = (depth - 1).max(0),
            _ => {}
        }
        i += 1;
    }
    (c.len(), depth, quote)
}

/// Does this (comment-free, right-trimmed) code end mid-expression?
fn ends_with_operator(code: &str) -> bool {
    if code.ends_with("%") {
        // a closed %op% (%>%, %in%, %*%) has two or more '%'
        return code.matches('%').count() >= 2;
    }
    ["<-", "<<-", "=", "+", "-", "*", "/", "^", "|>", "|", "&", "~", ",", "<", ">", "!", "$", "@", ":"]
        .iter().any(|op| code.ends_with(op))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<String> { s.split('\n').map(String::from).collect() }

    #[test]
    fn splits_simple_and_multi_line_statements() {
        let src = lines("x <- 1\n\n# note\nf <- function(a) {\n  a + 1\n}\ny <- x +\n  2\nd |>\n  head()");
        assert_eq!(statements(&src), vec![(0, 0), (3, 5), (6, 7), (8, 9)]);
    }

    #[test]
    fn brackets_in_strings_and_comments_do_not_count() {
        let src = lines("s <- \"(\" # )\nt <- 2\nu <- '{'\nv <- 3");
        assert_eq!(statements(&src), vec![(0, 0), (1, 1), (2, 2), (3, 3)]);
    }

    #[test]
    fn open_calls_strings_and_trailing_commas_continue() {
        let src = lines("plot(x,\n     y)\nmsg <- \"two\nlines\"\nz <- c(1, 2)");
        assert_eq!(statements(&src), vec![(0, 1), (2, 3), (4, 4)]);
        let pipe = lines("a %>%\n  b\nc %in% d");
        assert_eq!(statements(&pipe), vec![(0, 1), (2, 2)]);
    }

    #[test]
    fn statement_at_skips_blank_lines_forward() {
        let src = lines("a <- 1\n\nb <- function() {\n  2\n}\n");
        assert_eq!(statement_at(&src, 3), Some((2, 4)));
        assert_eq!(statement_at(&src, 1), Some((2, 4)));
        assert_eq!(statement_at(&src, 5), Some((2, 4)));
        assert_eq!(statement_at(&lines(""), 0), None);
    }
}
