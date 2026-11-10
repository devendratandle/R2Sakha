//! Syntax colouring for R2 (R) source, one line at a time.
//!
//! Not the parser's lexer: that one stops at the first error, and while
//! someone types, the text is unfinished most of the time — an open
//! string, half a number. This tokenizer never fails; it only labels
//! spans. The one thing that crosses lines is an open string, carried in
//! [`LineState`] so a multi-line string stays coloured as a string.

/// What a span of characters is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind { Plain, Keyword, Constant, Str, Number, Comment, Operator, Call }

/// A labelled run of characters: `start..end` in character columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span { pub start: usize, pub end: usize, pub kind: Kind }

/// The state a line starts in: inside a string opened by this quote, or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LineState { pub open_quote: Option<char> }

const KEYWORDS: &[&str] = &[
    "if", "else", "for", "while", "repeat", "function", "return", "next", "break", "in",
];
const CONSTANTS: &[&str] = &[
    "TRUE", "FALSE", "T", "F", "NULL", "NA", "NaN", "Inf",
    "NA_integer_", "NA_real_", "NA_character_",
];

/// Spans for one line, and the state the NEXT line starts in.
pub fn highlight_line(line: &str, state: LineState) -> (Vec<Span>, LineState) {
    let c: Vec<char> = line.chars().collect();
    let mut spans = Vec::new();
    let mut i = 0;
    let mut push = |start: usize, end: usize, kind: Kind| {
        if end > start { spans.push(Span { start, end, kind }); }
    };
    // A string carried over from the previous line.
    if let Some(q) = state.open_quote {
        match close_string(&c, 0, q) {
            Some(end) => { push(0, end, Kind::Str); i = end; }
            None => { push(0, c.len(), Kind::Str); return (spans, state); }
        }
    }
    while i < c.len() {
        let ch = c[i];
        if ch == '#' { push(i, c.len(), Kind::Comment); break; }
        if ch == '"' || ch == '\'' || ch == '`' {
            match close_string(&c, i + 1, ch) {
                Some(end) => {
                    // Backticks quote a NAME, not a string value.
                    push(i, end, if ch == '`' { Kind::Plain } else { Kind::Str });
                    i = end;
                }
                None if ch == '`' => { push(i, c.len(), Kind::Plain); break; }
                None => { push(i, c.len(), Kind::Str); return (spans, LineState { open_quote: Some(ch) }); }
            }
            continue;
        }
        if ch.is_ascii_digit() || (ch == '.' && c.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            let start = i;
            while i < c.len() && (c[i].is_ascii_alphanumeric() || c[i] == '.'
                                  || ((c[i] == '+' || c[i] == '-') && matches!(c[i - 1], 'e' | 'E'))) {
                i += 1;
            }
            push(start, i, Kind::Number);
            continue;
        }
        if ch.is_alphabetic() || ch == '.' || ch == '_' {
            let start = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '.' || c[i] == '_') { i += 1; }
            let word: String = c[start..i].iter().collect();
            let mut j = i;
            while j < c.len() && c[j] == ' ' { j += 1; }
            let kind = if KEYWORDS.contains(&word.as_str()) { Kind::Keyword }
                else if CONSTANTS.contains(&word.as_str()) { Kind::Constant }
                else if c.get(j) == Some(&'(') { Kind::Call }
                else { Kind::Plain };
            push(start, i, kind);
            continue;
        }
        // Assignment and pipe operators are worth seeing at a glance.
        let two: String = c[i..(i + 2).min(c.len())].iter().collect();
        let three: String = c[i..(i + 3).min(c.len())].iter().collect();
        if three == "<<-" || three == "->>" { push(i, i + 3, Kind::Operator); i += 3; continue; }
        if matches!(two.as_str(), "<-" | "->" | "|>" | "==" | "!=" | "<=" | ">=" | "&&" | "||") {
            push(i, i + 2, Kind::Operator); i += 2; continue;
        }
        if matches!(ch, '=' | '+' | '-' | '*' | '/' | '^' | '<' | '>' | '!' | '&' | '|' | '~' | '$' | '@' | ':') {
            push(i, i + 1, Kind::Operator); i += 1; continue;
        }
        i += 1;
    }
    (spans, LineState::default())
}

/// Index just past the closing `q` at or after `from`, honouring `\`
/// escapes; `None` if the line ends inside the string.
fn close_string(c: &[char], from: usize, q: char) -> Option<usize> {
    let mut i = from;
    while i < c.len() {
        if c[i] == '\\' { i += 2; continue; }
        if c[i] == q { return Some(i + 1); }
        i += 1;
    }
    None
}

/// The state each line starts in, for a whole buffer (index = line).
pub fn line_states(lines: &[String]) -> Vec<LineState> {
    let mut out = Vec::with_capacity(lines.len());
    let mut st = LineState::default();
    for l in lines {
        out.push(st);
        st = highlight_line(l, st).1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &str) -> Vec<(String, Kind)> {
        let c: Vec<char> = line.chars().collect();
        highlight_line(line, LineState::default()).0.iter()
            .map(|s| (c[s.start..s.end].iter().collect(), s.kind)).collect()
    }

    #[test]
    fn labels_the_parts_of_a_typical_line() {
        let k = kinds(r#"fit <- lm(y ~ x, data = d) # model"#);
        assert!(k.contains(&("<-".into(), Kind::Operator)));
        assert!(k.contains(&("lm".into(), Kind::Call)));
        assert!(k.contains(&("~".into(), Kind::Operator)));
        assert!(k.contains(&("# model".into(), Kind::Comment)));
        assert!(k.contains(&("fit".into(), Kind::Plain)));
    }

    #[test]
    fn keywords_constants_numbers_strings() {
        let k = kinds(r#"if (x > 1.5e-3) print("a \"b\"") else NA"#);
        assert!(k.contains(&("if".into(), Kind::Keyword)));
        assert!(k.contains(&("else".into(), Kind::Keyword)));
        assert!(k.contains(&("NA".into(), Kind::Constant)));
        assert!(k.contains(&("1.5e-3".into(), Kind::Number)));
        assert!(k.contains(&(r#""a \"b\"""#.into(), Kind::Str)));
        assert!(k.contains(&("print".into(), Kind::Call)));
    }

    #[test]
    fn a_hash_inside_a_string_is_not_a_comment_and_strings_span_lines() {
        let k = kinds(r##"s <- "#not" # yes"##);
        assert!(k.contains(&(r##""#not""##.into(), Kind::Str)));
        assert!(k.contains(&("# yes".into(), Kind::Comment)));
        let lines: Vec<String> = vec!["x <- 'open".into(), "still # string".into(), "done' + 1".into()];
        let st = line_states(&lines);
        assert_eq!(st[1].open_quote, Some('\''));
        let (sp, after) = highlight_line(&lines[1], st[1]);
        assert_eq!(sp, vec![Span { start: 0, end: 14, kind: Kind::Str }]);
        assert_eq!(after.open_quote, Some('\''));
        assert_eq!(st[2].open_quote, Some('\''));
        let (sp2, end) = highlight_line(&lines[2], st[2]);
        assert_eq!(sp2[0], Span { start: 0, end: 5, kind: Kind::Str });
        assert_eq!(end.open_quote, None);
    }

    #[test]
    fn never_fails_on_unfinished_input() {
        for s in ["\"", "`", "1e", ".5", "x <-", "f(", "\\", "é # ü"] {
            let _ = highlight_line(s, LineState::default());
        }
    }
}
