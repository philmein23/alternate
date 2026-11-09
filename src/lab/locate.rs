//! Finds a model's quote in the document. Only exact matches count, so a pass can point at text
//! but can't change or invent any. Curly and straight quotes are treated as the same.

use std::ops::Range;

fn fold(c: char) -> char {
    match c {
        '\u{2018}' | '\u{2019}' => '\'',
        '\u{201c}' | '\u{201d}' => '"',
        '\u{a0}' => ' ',
        other => other,
    }
}

fn matches_at(text: &str, start: usize, quote: &str) -> Option<usize> {
    let mut chars = text[start..].char_indices();
    let mut end = start;
    for q in quote.chars() {
        let (at, c) = chars.next()?;
        if fold(c) != fold(q) {
            return None;
        }
        end = start + at + c.len_utf8();
    }
    Some(end)
}

/// The byte range of the `occurrence`th match of `quote`, counting from 1. A quote that isn't
/// there, or an occurrence past the last one, finds nothing.
pub fn locate(text: &str, quote: &str, occurrence: usize) -> Option<Range<usize>> {
    if quote.is_empty() {
        return None;
    }
    let mut seen = 0;
    for (start, _) in text.char_indices() {
        if let Some(end) = matches_at(text, start, quote) {
            seen += 1;
            if seen == occurrence.max(1) {
                return Some(start..end);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "the cat, the cat and the \u{201c}dog\u{201d} don\u{2019}t care";

    #[test]
    fn it_finds_the_occurrence_asked_for() {
        assert_eq!(locate(TEXT, "the cat", 1), Some(0..7));
        assert_eq!(locate(TEXT, "the cat", 2), Some(9..16));
    }

    #[test]
    fn an_occurrence_past_the_end_finds_nothing() {
        assert_eq!(locate(TEXT, "the cat", 3), None);
    }

    #[test]
    fn straight_quotes_match_curly_text() {
        let r = locate(TEXT, "the \"dog\" don't", 1).unwrap();
        assert_eq!(&TEXT[r], "the \u{201c}dog\u{201d} don\u{2019}t");
    }

    #[test]
    fn a_made_up_quote_is_dropped() {
        assert_eq!(locate(TEXT, "the cats", 1), None);
        assert_eq!(locate(TEXT, "", 1), None);
    }

    #[test]
    fn multi_byte_text_stays_on_char_boundaries() {
        let text = "caf\u{e9} \u{65e5}\u{672c} caf\u{e9}";
        let r = locate(text, "caf\u{e9}", 2).unwrap();
        assert!(text.is_char_boundary(r.start) && text.is_char_boundary(r.end));
    }

    #[test]
    fn case_matters() {
        assert_eq!(locate(TEXT, "The cat", 1), None);
    }
}
