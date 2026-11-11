//! The console's state: transcript, the `+` continuation and history.
//! Ardon Studio's own copy of Ardon-R2's console buffer (r2-console), without
//! its engine types: here the engine is a separate process, and its
//! output arrives in chunks (`stream` lines) that need not end a line.

/// Semantic role of a transcript line; the painter picks its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// First line of an expression — the "R2> ..." prompt.
    Input,
    /// Continuation of a multi-line expression — the "+ ..." prompt.
    Continuation,
    Output,
    Error,
    /// Welcome text and IDE notes.
    Banner,
}

#[derive(Debug, Clone)]
pub struct ConsoleLine {
    pub kind: LineKind,
    pub text: String,
}

/// What the host should do after `submit_line()`.
#[derive(Debug, Clone)]
pub enum SubmitAction {
    /// Expression complete; evaluate this source (multi-line OK).
    Submit(String),
    /// Blank submission — emit a fresh prompt, do nothing else.
    Empty,
}

pub struct ConsoleBuffer {
    transcript: Vec<ConsoleLine>,
    /// Lines of an in-progress multi-line expression; empty at `R2>`.
    continuation: String,
    history: Vec<String>,
    /// Position while browsing history with ↑/↓.
    history_cursor: Option<usize>,
    max_lines: usize,
    /// The last line came from a stream chunk without a final newline:
    /// the next chunk of the same kind continues it.
    open_line: Option<LineKind>,
    /// (fresh input, continuation)
    prompts: (String, String),
}

impl Default for ConsoleBuffer {
    fn default() -> Self { Self::new() }
}

impl ConsoleBuffer {
    pub fn new() -> Self {
        Self {
            transcript: Vec::new(),
            continuation: String::new(),
            history: Vec::new(),
            history_cursor: None,
            max_lines: 10_000,
            open_line: None,
            prompts: ("R2>".into(), "+".into()),
        }
    }

    pub fn transcript(&self) -> &[ConsoleLine] { &self.transcript }
    pub fn history(&self) -> &[String] { &self.history }

    /// R2's prompts: `R2>` for fresh input, `+` mid-continuation, unless
    /// R2 names its own in `hello`.
    pub fn current_prompt(&self) -> &str {
        if self.continuation.is_empty() { &self.prompts.0 } else { &self.prompts.1 }
    }

    /// The prompts R2 uses.
    pub fn set_prompts(&mut self, main: &str, more: &str) { self.prompts = (main.to_string(), more.to_string()); }

    /// Esc: drop a half-typed multi-line expression, as R does.
    pub fn cancel_continuation(&mut self) {
        self.continuation.clear();
        self.history_cursor = None;
    }

    /// R2 found submitted source unfinished (an open string, say, which
    /// the bracket count here cannot see): take it back and ask for more.
    pub fn resume_continuation(&mut self, src: &str) {
        self.continuation = src.to_string();
        if self.history.last().map(String::as_str) == Some(src) { self.history.pop(); }
    }

    /// The user pressed Enter on `line`.
    pub fn submit_line(&mut self, line: String) -> SubmitAction {
        let kind = if self.continuation.is_empty() { LineKind::Input } else { LineKind::Continuation };
        let prompt = self.current_prompt().to_string();
        self.push_line(ConsoleLine { kind, text: format!("{} {}", prompt, line.trim_end()) });

        if line.trim().is_empty() {
            self.continuation.clear();
            self.history_cursor = None;
            return SubmitAction::Empty;
        }
        if !self.continuation.is_empty() { self.continuation.push('\n'); }
        self.continuation.push_str(&line);

        // R2's own parser decides whether the statement is complete: an
        // unfinished one comes back `incomplete`, and `resume_continuation`
        // asks for more, as R2's console does.
        let src = std::mem::take(&mut self.continuation);
        self.push_history(src.clone());
        self.history_cursor = None;
        SubmitAction::Submit(src)
    }

    /// A chunk of R2's printed output, as it arrives.
    pub fn push_stream(&mut self, text: &str, err: bool) {
        let kind = if err { LineKind::Error } else { LineKind::Output };
        let mut parts = text.split('\n').peekable();
        let mut first = true;
        while let Some(part) = parts.next() {
            let last = parts.peek().is_none();
            if last && part.is_empty() {
                // the chunk ended its line
                self.open_line = None;
                return;
            }
            match self.transcript.last_mut() {
                Some(l) if first && self.open_line == Some(kind) => l.text.push_str(part),
                _ => self.push_line(ConsoleLine { kind, text: part.to_string() }),
            }
            first = false;
        }
        self.open_line = Some(kind);
    }

    pub fn push_output(&mut self, text: &str) { self.push_lines(text, LineKind::Output); }
    pub fn push_error(&mut self, text: &str) { self.push_lines(text, LineKind::Error); }
    pub fn push_banner(&mut self, text: &str) { self.push_lines(text, LineKind::Banner); }

    fn push_lines(&mut self, text: &str, kind: LineKind) {
        for line in text.split('\n') {
            self.push_line(ConsoleLine { kind, text: line.to_string() });
        }
    }

    /// Wipe the transcript (Clear console). History is kept.
    pub fn clear(&mut self) {
        self.transcript.clear();
        self.open_line = None;
    }

    fn push_line(&mut self, line: ConsoleLine) {
        self.open_line = None;
        self.transcript.push(line);
        if self.transcript.len() > self.max_lines {
            let drop = self.transcript.len() - self.max_lines;
            self.transcript.drain(0..drop);
        }
    }

    fn push_history(&mut self, src: String) {
        if self.history.last() == Some(&src) { return; }
        self.history.push(src);
        if self.history.len() > 1000 {
            let drop = self.history.len() - 1000;
            self.history.drain(0..drop);
        }
    }

    /// ↑: step back through history.
    pub fn history_up(&mut self) -> Option<String> {
        if self.history.is_empty() { return None; }
        let next = match self.history_cursor {
            None => self.history.len() - 1,
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.history_cursor = Some(next);
        Some(self.history[next].clone())
    }

    /// ↓: step forward; past the end gives an empty line.
    pub fn history_down(&mut self) -> Option<String> {
        let i = self.history_cursor?;
        if i + 1 < self.history.len() {
            self.history_cursor = Some(i + 1);
            Some(self.history[i + 1].clone())
        } else {
            self.history_cursor = None;
            Some(String::new())
        }
    }
}

/// What a complete statement from the console means to the IDE.
#[derive(Debug, PartialEq)]
pub enum Typed {
    /// `q()` / `quit()`: ask before quitting, rather than let the engine
    /// process exit under the window.
    Quit,
    /// `clear()` / `cls()`: the transcript is the IDE's to clear.
    Clear,
    /// `View(x)`: open the data viewer on this expression.
    View(String),
    /// Everything else goes to R2.
    Code,
}

pub fn classify(src: &str) -> Typed {
    let compact: String = src.chars().filter(|c| !c.is_whitespace()).collect();
    match compact.trim_end_matches(';') {
        "q()" | "quit()" => return Typed::Quit,
        "clear()" | "cls()" => return Typed::Clear,
        _ => {}
    }
    view_argument(src.trim().trim_end_matches(';').trim_end()).map_or(Typed::Code, Typed::View)
}

/// The first argument of a call that is exactly `View(...)`.
fn view_argument(s: &str) -> Option<String> {
    let inner = s.strip_prefix("View(")?;
    let (mut depth, mut quote, mut first_end) = (1i32, None::<char>, None);
    for (i, ch) in inner.char_indices() {
        if let Some(q) = quote {
            if ch == q { quote = None; }
            continue;
        }
        match ch {
            '"' | '\'' | '`' => quote = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    // the closing bracket must end the statement
                    if i + 1 != inner.len() { return None; }
                    let arg = inner[..first_end.unwrap_or(i)].trim();
                    return if arg.is_empty() { None } else { Some(arg.to_string()) };
                }
            }
            ',' if depth == 1 && first_end.is_none() => first_end = Some(i),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(b: &ConsoleBuffer) -> Vec<(LineKind, &str)> {
        b.transcript().iter().map(|l| (l.kind, l.text.as_str())).collect()
    }

    #[test]
    fn continuation_and_history() {
        let mut b = ConsoleBuffer::new();
        // R2 answers `incomplete`: the line comes back for more
        let SubmitAction::Submit(src) = b.submit_line("for (i in 1:3) {".into()) else { panic!() };
        b.resume_continuation(&src);
        assert_eq!(b.current_prompt(), "+");
        match b.submit_line("}".into()) {
            SubmitAction::Submit(s) => assert_eq!(s, "for (i in 1:3) {\n}"),
            other => panic!("{other:?}"),
        }
        assert_eq!(b.history().len(), 1);
        assert_eq!(texts(&b)[1], (LineKind::Continuation, "+ }"));
    }

    #[test]
    fn r2_can_hand_unfinished_input_back() {
        let mut b = ConsoleBuffer::new();
        let SubmitAction::Submit(src) = b.submit_line("s <- \"abc".into()) else { panic!() };
        b.resume_continuation(&src);
        assert_eq!(b.current_prompt(), "+");
        assert!(b.history().is_empty());
        match b.submit_line("def\"".into()) {
            SubmitAction::Submit(s) => assert_eq!(s, "s <- \"abc\ndef\""),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn stream_chunks_join_into_lines() {
        let mut b = ConsoleBuffer::new();
        b.push_stream("a", false);
        b.push_stream("b\n[1] 2\n", false);
        b.push_stream("Warning", true);
        b.push_stream(": careful\n", true);
        assert_eq!(texts(&b), vec![(LineKind::Output, "ab"), (LineKind::Output, "[1] 2"),
                                   (LineKind::Error, "Warning: careful")]);
        b.push_stream("x", false);
        b.submit_line("1".into());
        assert_eq!(texts(&b).last().unwrap().1, "R2> 1", "a new prompt line is never joined");
    }

    #[test]
    fn r2_names_the_prompts() {
        let mut b = ConsoleBuffer::new();
        b.set_prompts(">>", "..");
        assert_eq!(b.current_prompt(), ">>");
        b.submit_line("1".into());
        assert_eq!(texts(&b)[0].1, ">> 1");
    }

    #[test]
    fn the_ide_keeps_what_is_its_own() {
        assert_eq!(classify(" q( ) "), Typed::Quit);
        assert_eq!(classify("quit();"), Typed::Quit);
        assert_eq!(classify("q(\"no\")"), Typed::Code);
        assert_eq!(classify("eq()"), Typed::Code);
        assert_eq!(classify("cls()"), Typed::Clear);
        assert_eq!(classify("View(d)"), Typed::View("d".into()));
        assert_eq!(classify("View( d[d$x > 1, ] , \"big\" );"), Typed::View("d[d$x > 1, ]".into()));
        assert_eq!(classify("View(a); mean(b)"), Typed::Code);
        assert_eq!(classify("View()"), Typed::Code);
        assert_eq!(classify("x <- 1"), Typed::Code);
    }
}
