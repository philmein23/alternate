//! Where Vim motions and text objects land. All of it is plain text arithmetic on byte offsets.

use std::ops::Range;

use crate::segment;

/// Where a motion ends, and how an operator should read it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub to: usize,
    pub inclusive: bool,
    pub linewise: bool,
}

fn plain(to: usize) -> Target {
    Target { to, inclusive: false, linewise: false }
}

fn inclusive(to: usize) -> Target {
    Target { to, inclusive: true, linewise: false }
}

pub fn lines(to: usize) -> Target {
    Target { to, inclusive: false, linewise: true }
}

pub fn next_char(text: &str, at: usize) -> usize {
    text[at..].chars().next().map_or(at, |c| at + c.len_utf8())
}

pub fn prev_char(text: &str, at: usize) -> usize {
    text[..at].char_indices().next_back().map_or(0, |(i, _)| i)
}

pub fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |i| i + 1)
}

pub fn line_end(text: &str, at: usize) -> usize {
    text[at..].find('\n').map_or(text.len(), |i| at + i)
}

pub fn first_nonblank(text: &str, at: usize) -> usize {
    let start = line_start(text, at);
    let line = &text[start..line_end(text, start)];
    start + line.len() - line.trim_start_matches([' ', '\t']).len()
}

/// In Normal mode the cursor sits on a character, so never past the end of a line.
pub fn clamp(text: &str, at: usize) -> usize {
    let at = at.min(text.len());
    let at_end = at == text.len() || text[at..].starts_with('\n');
    if at_end && at > 0 && !text[..at].ends_with('\n') { prev_char(text, at) } else { at }
}

/// The start of 1-based line `n`, or of the last line.
pub fn line_number(text: &str, n: usize) -> usize {
    let mut start = 0;
    for _ in 1..n {
        match text[start..].find('\n') {
            Some(i) => start += i + 1,
            None => break,
        }
    }
    start
}

pub fn last_line(text: &str) -> usize {
    line_start(text, text.len())
}

/// The line `n` below (or above, when negative) the one holding `at`.
pub fn line_offset(text: &str, at: usize, n: isize) -> usize {
    let mut start = line_start(text, at);
    for _ in 0..n.unsigned_abs() {
        if n > 0 {
            match text[start..].find('\n') {
                Some(i) => start += i + 1,
                None => break,
            }
        } else if start == 0 {
            break;
        } else {
            start = line_start(text, start - 1);
        }
    }
    start
}

fn class(c: char, big: bool) -> u8 {
    if c.is_whitespace() {
        0
    } else if big || c.is_alphanumeric() || c == '_' {
        1
    } else {
        2
    }
}

pub fn next_word(text: &str, at: usize, big: bool) -> usize {
    let mut chars = text[at..].char_indices().peekable();
    if let Some(&(_, first)) = chars.peek() {
        let start = class(first, big);
        while start != 0 && chars.peek().is_some_and(|&(_, c)| class(c, big) == start) {
            chars.next();
        }
    }
    while chars.peek().is_some_and(|&(_, c)| class(c, big) == 0) {
        chars.next();
    }
    chars.peek().map_or(text.len(), |&(i, _)| at + i)
}

pub fn prev_word(text: &str, at: usize, big: bool) -> usize {
    let mut chars = text[..at].char_indices().rev().peekable();
    while chars.peek().is_some_and(|&(_, c)| class(c, big) == 0) {
        chars.next();
    }
    let Some(&(mut start, first)) = chars.peek() else { return 0 };
    let run = class(first, big);
    while let Some(&(i, c)) = chars.peek() {
        if class(c, big) != run {
            break;
        }
        start = i;
        chars.next();
    }
    start
}

/// The last character of this word, or of the next one when already on it.
pub fn word_end(text: &str, at: usize, big: bool) -> usize {
    let from = next_char(text, at);
    let mut chars = text[from..].char_indices().peekable();
    while chars.peek().is_some_and(|&(_, c)| class(c, big) == 0) {
        chars.next();
    }
    let Some(&(mut last, first)) = chars.peek() else { return prev_char(text, text.len()) };
    let run = class(first, big);
    while let Some(&(i, c)) = chars.peek() {
        if class(c, big) != run {
            break;
        }
        last = i;
        chars.next();
    }
    from + last
}

/// The last character of the previous word.
pub fn prev_word_end(text: &str, at: usize, big: bool) -> usize {
    let mut chars = text[..at].char_indices().rev().peekable();
    let here = text[at..].chars().next().map_or(0, |c| class(c, big));
    while here != 0 && chars.peek().is_some_and(|&(_, c)| class(c, big) == here) {
        chars.next();
    }
    while chars.peek().is_some_and(|&(_, c)| class(c, big) == 0) {
        chars.next();
    }
    chars.peek().map_or(0, |&(i, _)| i)
}

pub fn next_paragraph(text: &str, at: usize) -> usize {
    let from = at + text[at..].len() - text[at..].trim_start_matches('\n').len();
    text[from..].find("\n\n").map_or(text.len(), |i| from + i + 1)
}

pub fn prev_paragraph(text: &str, at: usize) -> usize {
    let end = text[..at].trim_end_matches('\n').len();
    text[..end].rfind("\n\n").map_or(0, |i| i + 1)
}

fn sentence_starts(text: &str) -> Vec<usize> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = Vec::new();
    if let Some(first) = chars.iter().find(|(_, c)| !c.is_whitespace()) {
        out.push(first.0);
    }
    for i in 0..chars.len() {
        let c = chars[i].1;
        let blank_line = c == '\n' && chars.get(i + 1).is_some_and(|&(_, n)| n == '\n');
        if !matches!(c, '.' | '!' | '?') && !blank_line {
            continue;
        }
        let mut j = i + 1;
        while chars.get(j).is_some_and(|&(_, n)| matches!(n, '"' | '\'' | ')' | ']' | '\u{201d}' | '\u{2019}')) {
            j += 1;
        }
        if !chars.get(j).is_some_and(|&(_, n)| n.is_whitespace()) {
            continue;
        }
        while chars.get(j).is_some_and(|&(_, n)| n.is_whitespace()) {
            j += 1;
        }
        if let Some(&(pos, _)) = chars.get(j) {
            out.push(pos);
        }
    }
    out.dedup();
    out
}

pub fn next_sentence(text: &str, at: usize) -> usize {
    sentence_starts(text).into_iter().find(|&s| s > at).unwrap_or(text.len())
}

pub fn prev_sentence(text: &str, at: usize) -> usize {
    sentence_starts(text).into_iter().rev().find(|&s| s < at).unwrap_or(0)
}

const PAIRS: [(u8, u8); 3] = [(b'(', b')'), (b'[', b']'), (b'{', b'}')];

fn matching(text: &str, at: usize) -> Option<usize> {
    let b = text.as_bytes();
    let (open, close, forward) = PAIRS.iter().find_map(|&(o, c)| if b[at] == o { Some((o, c, true)) } else if b[at] == c { Some((o, c, false)) } else { None })?;
    let mut depth = 0i32;
    let mut i = at as isize;
    while i >= 0 && (i as usize) < b.len() {
        let x = b[i as usize];
        if x == open {
            depth += 1;
        } else if x == close {
            depth -= 1;
        }
        if depth == 0 {
            return Some(i as usize);
        }
        i += if forward { 1 } else { -1 };
    }
    None
}

/// `%`: from the first bracket at or after the cursor on this line to its partner.
pub fn bracket(text: &str, at: usize) -> Option<usize> {
    let end = line_end(text, at);
    let start = text[at..end].bytes().position(|b| PAIRS.iter().any(|&(o, c)| b == o || b == c))?;
    matching(text, at + start)
}

/// `f`, `F`, `t` and `T`, within the line.
pub fn find_char(text: &str, at: usize, kind: char, wanted: char, n: usize) -> Option<Target> {
    if matches!(kind, 'f' | 't') {
        let end = line_end(text, at);
        let from = next_char(text, at).min(end);
        let (pos, _) = text[from..end].char_indices().filter(|&(_, c)| c == wanted).nth(n - 1)?;
        let pos = from + pos;
        Some(inclusive(if kind == 't' { prev_char(text, pos) } else { pos }))
    } else {
        let start = line_start(text, at);
        let (pos, _) = text[start..at].char_indices().rev().filter(|&(_, c)| c == wanted).nth(n - 1)?;
        let pos = start + pos;
        Some(plain(if kind == 'T' { next_char(text, pos) } else { pos }))
    }
}

fn repeat(mut at: usize, n: usize, f: impl Fn(usize) -> usize) -> usize {
    for _ in 0..n {
        at = f(at);
    }
    at
}

/// A motion that depends only on the text. `op` says an operator is waiting for it.
pub fn simple(text: &str, at: usize, c: char, n: usize, op: bool) -> Option<Target> {
    Some(match c {
        'h' => {
            let start = line_start(text, at);
            plain(repeat(at, n, |i| if i > start { prev_char(text, i) } else { i }))
        }
        'l' => {
            let end = line_end(text, at);
            let stop = if op { end } else { clamp(text, end) };
            plain(repeat(at, n, |i| if i < stop { next_char(text, i) } else { i }))
        }
        '0' => plain(line_start(text, at)),
        '^' => plain(first_nonblank(text, at)),
        '$' => {
            let line = line_offset(text, at, n as isize - 1);
            let end = line_end(text, line);
            if end == line { plain(end) } else { inclusive(prev_char(text, end)) }
        }
        'w' | 'W' => {
            let big = c == 'W';
            let to = repeat(at, n, |i| next_word(text, i, big));
            // An operator stops at the end of the line rather than eating the line break.
            let moved = &text[at..to];
            let kept = moved.trim_end().len();
            let to = if op && moved[kept..].contains('\n') { at + kept } else { to };
            plain(to)
        }
        'b' | 'B' => plain(repeat(at, n, |i| prev_word(text, i, c == 'B'))),
        'e' | 'E' => inclusive(repeat(at, n, |i| word_end(text, i, c == 'E'))),
        '}' => plain(repeat(at, n, |i| next_paragraph(text, i))),
        '{' => plain(repeat(at, n, |i| prev_paragraph(text, i))),
        ')' => plain(repeat(at, n, |i| next_sentence(text, i))),
        '(' => plain(repeat(at, n, |i| prev_sentence(text, i))),
        '%' => inclusive(bracket(text, at)?),
        '+' => lines(first_nonblank(text, line_offset(text, at, n as isize))),
        '-' => lines(first_nonblank(text, line_offset(text, at, -(n as isize)))),
        '_' => lines(first_nonblank(text, line_offset(text, at, n as isize - 1))),
        _ => return None,
    })
}

/// A text object's range, and whether it covers whole lines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Object {
    pub range: Range<usize>,
    pub linewise: bool,
}

fn chars_of(range: Range<usize>) -> Object {
    Object { range, linewise: false }
}

fn word_object(text: &str, at: usize, inner: bool, big: bool) -> Option<Object> {
    let here = text[at..].chars().next().map(|c| class(c, big))?;
    let same = |c: char| class(c, big) == here;
    let start = at - text[..at].chars().rev().take_while(|&c| same(c) && c != '\n').map(char::len_utf8).sum::<usize>();
    let end = at + text[at..].chars().take_while(|&c| same(c) && c != '\n').map(char::len_utf8).sum::<usize>();
    if inner {
        return Some(chars_of(start..end));
    }
    let trailing: usize = text[end..].chars().take_while(|&c| c == ' ' || c == '\t').map(char::len_utf8).sum();
    if trailing > 0 || here == 0 {
        return Some(chars_of(start..end + trailing));
    }
    let leading: usize = text[..start].chars().rev().take_while(|&c| c == ' ' || c == '\t').map(char::len_utf8).sum();
    Some(chars_of(start - leading..end))
}

fn is_blank(text: &str, start: usize) -> bool {
    text[start..line_end(text, start)].trim().is_empty()
}

fn paragraph_object(text: &str, at: usize, inner: bool) -> Object {
    let here = is_blank(text, line_start(text, at));
    let mut start = line_start(text, at);
    while start > 0 {
        let above = line_start(text, start - 1);
        if is_blank(text, above) != here {
            break;
        }
        start = above;
    }
    let mut end = line_end(text, at);
    while end < text.len() && is_blank(text, end + 1) == here {
        end = line_end(text, end + 1);
    }
    if !inner && !here {
        while end < text.len() && is_blank(text, end + 1) {
            end = line_end(text, end + 1);
        }
    }
    Object { range: start..end, linewise: true }
}

fn quote_object(text: &str, at: usize, inner: bool, q: char) -> Option<Object> {
    let start = line_start(text, at);
    let line = &text[start..line_end(text, start)];
    let marks: Vec<usize> = line.char_indices().filter(|&(_, c)| c == q).map(|(i, _)| start + i).collect();
    let (open, close) = marks.as_chunks::<2>().0.iter().map(|p| (p[0], p[1])).find(|&(_, c)| c >= at)?;
    let width = q.len_utf8();
    if inner {
        return Some(chars_of(open + width..close));
    }
    let end = close + width;
    let trailing: usize = text[end..].chars().take_while(|&c| c == ' ' || c == '\t').map(char::len_utf8).sum();
    Some(chars_of(open..end + trailing))
}

fn enclosing(text: &str, at: usize, open: u8, close: u8) -> Option<(usize, usize)> {
    let b = text.as_bytes();
    if b.is_empty() {
        return None;
    }
    let mut depth = 0;
    let mut left = None;
    for i in (0..=at.min(b.len().saturating_sub(1))).rev() {
        if b[i] == close && i != at {
            depth += 1;
        } else if b[i] == open {
            if depth == 0 {
                left = Some(i);
                break;
            }
            depth -= 1;
        }
    }
    let left = left?;
    let mut depth = 0;
    for (i, &x) in b.iter().enumerate().skip(left + 1) {
        if x == open {
            depth += 1;
        } else if x == close {
            if depth == 0 {
                return Some((left, i));
            }
            depth -= 1;
        }
    }
    None
}

/// `iw`, `ap`, `i"`, `a(` and the rest, named by the character after `i` or `a`.
pub fn object(text: &str, at: usize, inner: bool, c: char) -> Option<Object> {
    match c {
        'w' => word_object(text, at, inner, false),
        'W' => word_object(text, at, inner, true),
        'p' => Some(paragraph_object(text, at, inner)),
        's' => {
            let r = segment::sentence_at(text, at)?;
            let trailing: usize = text[r.end..].chars().take_while(|&c| c == ' ' || c == '\t').map(char::len_utf8).sum();
            Some(chars_of(r.start..if inner { r.end } else { r.end + trailing }))
        }
        '"' | '\'' | '`' => quote_object(text, at, inner, c),
        _ => {
            let (open, close) = match c {
                '(' | ')' | 'b' => (b'(', b')'),
                '[' | ']' => (b'[', b']'),
                '{' | '}' | 'B' => (b'{', b'}'),
                '<' | '>' => (b'<', b'>'),
                _ => return None,
            };
            let (l, r) = enclosing(text, at, open, close)?;
            Some(chars_of(if inner { l + 1..r } else { l..r + 1 }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: &str = "one two, three\n\nnext para here\nsecond\n\nlast";

    #[test]
    fn w_goes_to_word_starts_and_stops_on_punctuation() {
        assert_eq!(next_word(T, 0, false), 4);
        assert_eq!(next_word(T, 4, false), 7);
        assert_eq!(next_word(T, 7, false), 9);
        assert_eq!(next_word(T, 4, true), 9);
    }

    #[test]
    fn b_goes_back_to_word_starts() {
        assert_eq!(prev_word(T, 9, false), 7);
        assert_eq!(prev_word(T, 6, false), 4);
        assert_eq!(prev_word(T, 0, false), 0);
    }

    #[test]
    fn e_and_ge_land_on_the_last_letter() {
        assert_eq!(word_end(T, 0, false), 2);
        assert_eq!(word_end(T, 2, false), 6);
        assert_eq!(prev_word_end(T, 4, false), 2);
    }

    #[test]
    fn paragraphs_stop_on_blank_lines() {
        let first = next_paragraph(T, 0);
        assert_eq!(first, T.find("\n\n").unwrap() + 1);
        assert_eq!(prev_paragraph(T, first), 0);
    }

    #[test]
    fn sentences_step_over_punctuation() {
        let s = "First one. Second \"two.\" Third?\n\nNew.";
        assert_eq!(next_sentence(s, 0), 11);
        assert_eq!(next_sentence(s, 11), s.find("Third").unwrap());
        assert_eq!(prev_sentence(s, 11), 0);
        assert_eq!(next_sentence(s, s.find("Third").unwrap()), s.find("New").unwrap());
    }

    #[test]
    fn dollar_lands_on_the_last_character_of_the_line() {
        assert_eq!(simple(T, 0, '$', 1, false), Some(Target { to: 13, inclusive: true, linewise: false }));
    }

    #[test]
    fn dw_stops_at_the_end_of_the_line() {
        let text = "one two\nthree";
        assert_eq!(simple(text, 4, 'w', 1, true).unwrap().to, 7);
        assert_eq!(simple(text, 4, 'w', 1, false).unwrap().to, 8);
    }

    #[test]
    fn find_stays_on_the_line() {
        assert_eq!(find_char(T, 0, 'f', 't', 1).unwrap().to, 4);
        assert_eq!(find_char(T, 0, 't', 't', 1).unwrap().to, 3);
        assert_eq!(find_char(T, 0, 'f', 'x', 1), None);
        assert_eq!(find_char(T, 6, 'F', 'o', 1).unwrap().to, 0);
    }

    #[test]
    fn percent_finds_nested_partners() {
        let s = "a (b (c) d) e";
        assert_eq!(bracket(s, 0), Some(10));
        assert_eq!(bracket(s, 10), Some(2));
        assert_eq!(bracket(s, 5), Some(7));
    }

    #[test]
    fn word_objects_inner_and_around() {
        assert_eq!(object(T, 5, true, 'w').unwrap().range, 4..7);
        assert_eq!(object("one two", 5, false, 'w').unwrap().range, 3..7);
        assert_eq!(object("one two", 1, false, 'w').unwrap().range, 0..4);
    }

    #[test]
    fn quote_and_bracket_objects() {
        let s = "say \"hello there\" and (more (nested) here)";
        assert_eq!(&s[object(s, 8, true, '"').unwrap().range], "hello there");
        assert_eq!(&s[object(s, 25, true, '(').unwrap().range], "more (nested) here");
        assert_eq!(&s[object(s, 30, false, ')').unwrap().range], "(nested)");
    }

    #[test]
    fn paragraph_objects_take_whole_blocks() {
        let inner = object(T, 20, true, 'p').unwrap();
        assert!(inner.linewise);
        assert_eq!(&T[inner.range], "next para here\nsecond");
        let around = object(T, 20, false, 'p').unwrap();
        assert_eq!(&T[around.range], "next para here\nsecond\n");
    }

    #[test]
    fn line_numbers_and_clamps() {
        assert_eq!(line_number(T, 3), 16);
        assert_eq!(line_number(T, 999), last_line(T));
        assert_eq!(clamp("one\ntwo", 3), 2);
        assert_eq!(clamp("one\n\ntwo", 4), 4);
        assert_eq!(clamp("one", 3), 2);
    }

    #[test]
    fn nothing_lands_inside_a_character() {
        let text = "\u{1f600} caf\u{e9} \u{65e5}\u{672c}\n\n\u{5e9}\u{5dc}";
        for at in text.char_indices().map(|(i, _)| i) {
            for big in [false, true] {
                for got in [next_word(text, at, big), prev_word(text, at, big), word_end(text, at, big), prev_word_end(text, at, big)] {
                    assert!(text.is_char_boundary(got), "{got}");
                }
            }
            for got in [next_paragraph(text, at), prev_paragraph(text, at), next_sentence(text, at), prev_sentence(text, at), clamp(text, at)] {
                assert!(text.is_char_boundary(got), "{got}");
            }
        }
    }
}
