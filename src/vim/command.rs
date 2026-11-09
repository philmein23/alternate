//! The `/` and `:` lines: search, and the Ex commands a writer reaches for.

use std::ops::Range;

use rune::{Key, KeyEvent, Tx};

use super::motion::{self, Target};
use super::{Cmdline, Search};
use crate::State;

fn fold(c: char, ignore: bool) -> char {
    if ignore { c.to_lowercase().next().unwrap_or(c) } else { c }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Every match, as byte offsets. Literal text, ignoring case unless the pattern has a capital.
pub fn matches(text: &str, pattern: &str, whole: bool) -> Vec<usize> {
    let ignore = !pattern.chars().any(char::is_uppercase);
    let wanted: Vec<char> = pattern.chars().map(|c| fold(c, ignore)).collect();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    if wanted.is_empty() || wanted.len() > chars.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for start in 0..=chars.len() - wanted.len() {
        if !wanted.iter().enumerate().all(|(i, w)| fold(chars[start + i].1, ignore) == *w) {
            continue;
        }
        let before = start > 0 && is_word(chars[start - 1].1);
        let after = chars.get(start + wanted.len()).is_some_and(|&(_, c)| is_word(c));
        if !whole || (!before && !after) {
            out.push(chars[start].0);
        }
    }
    out
}

/// The next match in the direction asked, wrapping around the document.
pub fn next_match(text: &str, search: &Search, at: usize, forward: bool) -> Option<usize> {
    let found = matches(text, &search.pattern, search.whole);
    if forward {
        found.iter().find(|&&m| m > at).or(found.first()).copied()
    } else {
        found.iter().rev().find(|&&m| m < at).or(found.last()).copied()
    }
}

/// `s/from/to/` parts: `from`, `to`, and whether every match on a line goes.
fn parse_substitute(body: &str) -> Option<(String, String, bool)> {
    let mut parts = body.strip_prefix('/')?.splitn(3, '/');
    let from = parts.next()?.to_string();
    let to = parts.next().unwrap_or("").to_string();
    let global = parts.next().unwrap_or("").contains('g');
    (!from.is_empty()).then_some((from, to, global))
}

fn substitute_ranges(text: &str, from: &str, global: bool, within: Range<usize>) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::new();
    let mut line = within.start;
    while line <= within.end {
        let end = motion::line_end(text, line).min(within.end);
        let mut at = line;
        while let Some(i) = text[at..end].find(from) {
            out.push(at + i..at + i + from.len());
            at += i + from.len();
            if !global {
                break;
            }
        }
        if end >= within.end {
            break;
        }
        line = end + 1;
    }
    out
}

impl State {
    pub(super) fn vim_open_cmdline(&mut self, kind: char) {
        self.vim.cmdline = Some(Cmdline { kind, text: String::new() });
    }

    /// Takes every key while the line is open.
    pub(super) fn vim_cmdline_key(&mut self, ev: &KeyEvent, tx: &mut Tx) {
        let Some(line) = self.vim.cmdline.as_mut() else { return };
        match &ev.key {
            Key::Text(t) => line.text.push_str(t),
            Key::Backspace if line.text.pop().is_none() => {
                self.vim.cmdline = None;
                self.vim.reset();
            }
            Key::Escape => {
                self.vim.cmdline = None;
                self.vim.reset();
            }
            Key::Enter => {
                let line = self.vim.cmdline.take().expect("just matched");
                match line.kind {
                    ':' => {
                        self.vim_ex(line.text.trim(), tx);
                        self.vim.reset();
                    }
                    kind => self.vim_run_search(line.text, kind == '/', tx),
                }
            }
            _ => {}
        }
    }

    fn vim_run_search(&mut self, pattern: String, forward: bool, tx: &mut Tx) {
        if !pattern.is_empty() {
            self.vim.search = Some(Search { pattern, forward, whole: false });
        }
        self.vim_search_step(true, tx);
    }

    /// `n`, `N`, and the jump after `/`. `same` keeps the search's own direction.
    pub(super) fn vim_search_step(&mut self, same: bool, tx: &mut Tx) {
        let Some(search) = self.vim.search.clone() else {
            self.vim.reset();
            return;
        };
        let forward = search.forward == same;
        let n = self.vim.total();
        let mut at = self.edit.focus;
        let mut found = None;
        for _ in 0..n {
            found = next_match(&self.edit.text, &search, at, forward);
            match found {
                Some(m) => at = m,
                None => break,
            }
        }
        match found {
            Some(to) => self.vim_carry_out(Target { to, inclusive: false, linewise: false }, tx),
            None => {
                self.file.notice = Some(format!("Not found: {}", search.pattern));
                self.vim.reset();
            }
        }
    }

    /// `*` and `#`: the word under the cursor, whole.
    pub(super) fn vim_search_word(&mut self, forward: bool, tx: &mut Tx) {
        let at = self.edit.focus;
        let Some(object) = motion::object(&self.edit.text, at, true, 'w') else { return self.vim.reset() };
        let pattern = self.edit.text[object.range].trim().to_string();
        if pattern.is_empty() {
            return self.vim.reset();
        }
        self.vim.search = Some(Search { pattern, forward, whole: true });
        self.vim_search_step(true, tx);
    }

    fn vim_ex(&mut self, command: &str, tx: &mut Tx) {
        if let Ok(n) = command.parse::<usize>() {
            let to = motion::first_nonblank(&self.edit.text, motion::line_number(&self.edit.text, n.max(1)));
            return self.vim_jump(to, tx);
        }
        match command {
            "" | "noh" | "nohlsearch" => {}
            "w" | "write" => self.save(tx),
            "wq" | "x" => {
                self.save(tx);
                if !self.file.dirty {
                    tx.close_window();
                }
            }
            "q" | "quit" if self.file.dirty => self.file.notice = Some("Unsaved changes. Use :w to save or :q! to leave anyway.".to_string()),
            "q" | "quit" | "q!" | "quit!" => tx.close_window(),
            _ => {
                let (whole, body) = match command.strip_prefix('%') {
                    Some(rest) => (true, rest),
                    None => (false, command),
                };
                match body.strip_prefix('s').and_then(parse_substitute) {
                    Some((from, to, global)) => self.vim_substitute(&from, &to, global, whole),
                    None => self.file.notice = Some(format!("Not a command here: {command}")),
                }
            }
        }
    }

    /// One undo step, moving every marked word and ghost the same way a trim does.
    fn vim_substitute(&mut self, from: &str, to: &str, global: bool, whole: bool) {
        let at = self.edit.focus;
        let text = self.edit.text.clone();
        let within = if whole { 0..text.len() } else { motion::line_start(&text, at)..motion::line_end(&text, at) };
        let ranges = substitute_ranges(&text, from, global, within);
        let count = ranges.len();
        if count == 0 {
            self.file.notice = Some(format!("Not found: {from}"));
            return;
        }
        let caret = ranges[0].start;
        self.with_undo(|s| {
            s.edit.undo_group(|e| {
                for r in ranges.iter().rev() {
                    e.set_selection(r.start, r.end);
                    e.insert(to);
                }
                e.set_selection(caret, caret);
            });
            for r in ranges.iter().rev() {
                s.doc.splice(r.clone(), to, None);
            }
            s.touch();
        });
        self.file.notice = Some(if count == 1 { "1 change".to_string() } else { format!("{count} changes") });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_ignores_case_until_the_pattern_has_a_capital() {
        assert_eq!(matches("Fog and fog", "fog", false), [0, 8]);
        assert_eq!(matches("Fog and fog", "Fog", false), [0]);
    }

    #[test]
    fn whole_word_search_skips_parts_of_words() {
        assert_eq!(matches("fog foggy fog", "fog", true), [0, 10]);
    }

    #[test]
    fn search_wraps_around() {
        let s = Search { pattern: "a".to_string(), forward: true, whole: false };
        assert_eq!(next_match("a b a", &s, 4, true), Some(0));
        assert_eq!(next_match("a b a", &s, 0, false), Some(4));
    }

    #[test]
    fn search_handles_text_outside_ascii() {
        assert_eq!(matches("caf\u{e9} caf\u{e9}", "\u{e9}", false), [3, 9]);
    }

    #[test]
    fn substitute_parses_its_parts() {
        assert_eq!(parse_substitute("/a/b/g"), Some(("a".to_string(), "b".to_string(), true)));
        assert_eq!(parse_substitute("/a/b"), Some(("a".to_string(), "b".to_string(), false)));
        assert_eq!(parse_substitute("/a/"), Some(("a".to_string(), String::new(), false)));
        assert_eq!(parse_substitute("//b"), None);
    }

    #[test]
    fn substitute_takes_the_first_on_each_line_unless_global() {
        let text = "a a\na a";
        assert_eq!(substitute_ranges(text, "a", false, 0..text.len()), [0..1, 4..5]);
        assert_eq!(substitute_ranges(text, "a", true, 0..3), [0..1, 2..3]);
    }
}
