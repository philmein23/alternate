//! Works out which word, sentence or paragraph a position or selection means.

use std::ops::Range;

use crate::model::Level;

const ABBREVIATIONS: &[&str] = &["e.g", "i.e", "mr", "mrs", "ms", "dr", "st", "vs", "prof", "sr", "jr", "no", "fig", "cf", "approx"];
const CLOSERS: &[char] = &['"', '\'', '”', '’', ')', ']', '»', '」', '』', '*', '_'];
const WIDE_ENDS: &[char] = &['。', '！', '？'];

fn is_han(c: char) -> bool {
    matches!(c as u32, 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF)
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || (!c.is_ascii() && unicode_mark(c))
}

/// Combining marks, as in Vietnamese or Hindi written with separate accents.
fn unicode_mark(c: char) -> bool {
    matches!(c as u32, 0x0300..=0x036F | 0x0900..=0x0903 | 0x093A..=0x094F | 0x0E31 | 0x0E34..=0x0E3A | 0x0E47..=0x0E4E)
}

fn joins(c: char) -> bool {
    matches!(c, '\'' | '’' | '-' | '\u{2010}' | '\u{2011}')
}

/// The word containing `pos`, or ending at it. Apostrophes and hyphens inside a word belong to it.
pub fn word_at(text: &str, pos: usize) -> Option<Range<usize>> {
    let pos = pos.min(text.len());
    if !text.is_char_boundary(pos) {
        return None;
    }
    let inside = |p: usize| text[p..].chars().next().filter(|c| is_word_char(*c));
    let before = |p: usize| text[..p].chars().next_back().filter(|c| is_word_char(*c));
    let at = match (inside(pos), before(pos)) {
        (Some(c), _) if is_han(c) => return Some(pos..pos + c.len_utf8()),
        (Some(_), _) => pos,
        (None, Some(c)) if is_han(c) => return Some(pos - c.len_utf8()..pos),
        (None, Some(c)) => pos - c.len_utf8(),
        (None, None) => return None,
    };

    let mut start = at;
    while let Some(c) = text[..start].chars().next_back() {
        let joined = joins(c) && text[..start - c.len_utf8()].chars().next_back().is_some_and(|p| is_word_char(p) && !is_han(p));
        if (is_word_char(c) && !is_han(c)) || joined {
            start -= c.len_utf8();
        } else {
            break;
        }
    }
    let mut end = at;
    while let Some(c) = text[end..].chars().next() {
        let next = end + c.len_utf8();
        let joined = joins(c) && text[next..].chars().next().is_some_and(|n| is_word_char(n) && !is_han(n));
        if (is_word_char(c) && !is_han(c)) || joined {
            end = next;
        } else {
            break;
        }
    }
    (start < end).then_some(start..end)
}

/// The paragraph blocks of `text`: runs of lines between blank lines, with a `#` headline on its own.
pub fn paragraphs(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut block: Option<Range<usize>> = None;
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let content = line.trim_end_matches(['\n', '\r']);
        let range = at..at + content.len();
        at += line.len();
        if content.trim().is_empty() {
            out.extend(block.take());
        } else if content.trim_start().starts_with('#') {
            out.extend(block.take());
            out.push(range);
        } else {
            match &mut block {
                Some(b) => b.end = range.end,
                None => block = Some(range),
            }
        }
    }
    out.extend(block);
    out
}

pub fn paragraph_at(text: &str, pos: usize) -> Option<Range<usize>> {
    paragraphs(text).into_iter().find(|p| p.start <= pos && pos <= p.end)
}

/// Whether the `.` at byte `at` is part of an abbreviation or initial rather than a full stop.
fn is_abbreviation(text: &str, at: usize) -> bool {
    let word_start = text[..at].rfind(|c: char| c.is_whitespace() || c == '(' || c == '"' || c == '“').map_or(0, |i| i + text[i..].chars().next().map_or(1, char::len_utf8));
    let word = &text[word_start..at];
    let lower = word.to_lowercase();
    if ABBREVIATIONS.contains(&lower.as_str()) {
        return true;
    }
    let mut letters = word.chars();
    matches!((letters.next(), letters.next()), (Some(c), None) if c.is_uppercase())
}

/// Where the sentence ends if the terminator at `term` closes one, skipping closing quotes.
fn sentence_end(text: &str, term: usize, term_char: char, limit: usize) -> Option<usize> {
    let mut end = term + term_char.len_utf8();
    while end < limit && text[end..].chars().next().is_some_and(|c| CLOSERS.contains(&c) || matches!(c, '.' | '!' | '?' | '…')) {
        end += text[end..].chars().next().map_or(1, char::len_utf8);
    }
    if WIDE_ENDS.contains(&term_char) {
        return Some(end);
    }
    if end < limit && !text[end..].chars().next().is_some_and(char::is_whitespace) {
        return None;
    }
    if term_char == '.' && is_abbreviation(text, term) {
        return None;
    }
    // A lowercase word next means the sentence carries on, as after "etc." or a quoted question.
    let quoted = end > term + term_char.len_utf8();
    let next = text[end..limit].trim_start().chars().next();
    if (term_char == '.' || quoted) && next.is_some_and(char::is_lowercase) {
        return None;
    }
    Some(end)
}

/// The sentences of `para` within `text`, without the spaces between them.
pub fn sentences_in(text: &str, para: Range<usize>) -> Vec<Range<usize>> {
    let limit = para.end;
    let mut out = Vec::new();
    let mut start = para.start;
    let mut iter = text[para.clone()].char_indices();
    while let Some((i, c)) = iter.next() {
        let at = para.start + i;
        if !matches!(c, '.' | '!' | '?' | '…') && !WIDE_ENDS.contains(&c) {
            continue;
        }
        let Some(end) = sentence_end(text, at, c, limit) else { continue };
        out.push(trimmed(text, start..end));
        start = end;
        while para.start + iter.offset() < end {
            iter.next();
        }
    }
    if !text[start..limit].trim().is_empty() {
        out.push(trimmed(text, start..limit));
    }
    out
}

fn trimmed(text: &str, r: Range<usize>) -> Range<usize> {
    let s = &text[r.clone()];
    let lead = s.len() - s.trim_start().len();
    r.start + lead..r.start + s.trim_end().len()
}

/// The sentence holding `pos`. A position in the gap after a sentence belongs to that sentence.
pub fn sentence_at(text: &str, pos: usize) -> Option<Range<usize>> {
    let para = paragraph_at(text, pos)?;
    let all = sentences_in(text, para);
    let found = all.iter().rev().find(|s| s.start <= pos).or(all.first())?;
    Some(found.clone())
}

/// The target a selection means: the smallest level it matches exactly, or a sentence-style target
/// holding exactly what was selected.
pub fn target_for_selection(text: &str, sel: Range<usize>) -> Option<(Level, Range<usize>)> {
    let sel = trimmed(text, sel);
    if sel.is_empty() {
        return None;
    }
    if word_at(text, sel.start).as_ref() == Some(&sel) {
        return Some((Level::Word, sel));
    }
    if sentence_at(text, sel.start).as_ref() == Some(&sel) {
        return Some((Level::Sentence, sel));
    }
    if paragraph_at(text, sel.start).as_ref() == Some(&sel) {
        return Some((Level::Paragraph, sel));
    }
    Some((Level::Sentence, sel))
}

/// The range a position means at `level`.
pub fn range_at(text: &str, pos: usize, level: Level) -> Option<Range<usize>> {
    match level {
        Level::Word => word_at(text, pos),
        Level::Sentence => sentence_at(text, pos),
        Level::Paragraph => paragraph_at(text, pos),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ESSAY: &str = "# The Harbor\n\nDr. Lin didn't mind the well-known fog, e.g. the kind that sits at 3.5 metres. \
\"Is that all?\" she asked. The ferry — old, slow — left anyway!\n\nA second block\nruns over two lines. It ends here…\n\n港の朝は静かだ。霧が晴れた！\n";

    fn word(text: &str, needle: &str) -> Option<String> {
        let at = text.find(needle).unwrap();
        word_at(text, at).map(|r| text[r].to_string())
    }

    fn sentence(text: &str, needle: &str) -> String {
        let at = text.find(needle).unwrap();
        text[sentence_at(text, at).unwrap()].to_string()
    }

    #[test]
    fn apostrophes_and_hyphens_stay_in_a_word() {
        assert_eq!(word(ESSAY, "didn't").unwrap(), "didn't");
        assert_eq!(word(ESSAY, "well-known").unwrap(), "well-known");
        assert_eq!(word("the writer’s desk", "writer").unwrap(), "writer’s");
    }

    #[test]
    fn a_dash_between_words_splits_them() {
        assert_eq!(word(ESSAY, "old").unwrap(), "old");
        assert_eq!(word("one--two", "two").unwrap(), "two");
    }

    #[test]
    fn a_position_right_after_a_word_means_that_word() {
        let text = "one two";
        assert_eq!(word_at(text, 3), Some(0..3));
    }

    #[test]
    fn empty_space_has_no_word() {
        assert_eq!(word_at("a  b", 2), None);
        assert_eq!(word_at("", 0), None);
    }

    #[test]
    fn accented_and_decomposed_letters_are_letters() {
        assert_eq!(word("café noir", "caf").unwrap(), "café");
        let decomposed = "Vie\u{0323}\u{0302}t Nam";
        assert_eq!(word_at(decomposed, 0).map(|r| decomposed[r].to_string()).unwrap(), "Vie\u{0323}\u{0302}t");
    }

    #[test]
    fn han_characters_are_one_word_each() {
        assert_eq!(word("港の朝", "朝").unwrap(), "朝");
    }

    #[test]
    fn abbreviations_and_decimals_do_not_end_a_sentence() {
        let s = sentence(ESSAY, "Dr.");
        assert!(s.starts_with("Dr. Lin") && s.ends_with("3.5 metres."), "{s}");
    }

    #[test]
    fn a_closing_quote_stays_with_its_sentence() {
        assert_eq!(sentence(ESSAY, "Is that"), "\"Is that all?\" she asked.");
    }

    #[test]
    fn exclamation_ends_a_sentence() {
        assert_eq!(sentence(ESSAY, "The ferry"), "The ferry — old, slow — left anyway!");
    }

    #[test]
    fn sentences_cross_a_soft_line_break() {
        assert_eq!(sentence(ESSAY, "A second"), "A second block\nruns over two lines.");
    }

    #[test]
    fn an_ellipsis_at_the_end_closes_the_sentence() {
        assert_eq!(sentence(ESSAY, "It ends"), "It ends here…");
    }

    #[test]
    fn wide_punctuation_ends_a_sentence_without_a_space() {
        assert_eq!(sentence(ESSAY, "港"), "港の朝は静かだ。");
        assert_eq!(sentence(ESSAY, "霧"), "霧が晴れた！");
    }

    #[test]
    fn a_lowercase_continuation_is_the_same_sentence() {
        assert_eq!(sentence("We packed tents, rope, etc. and left. Then it rained.", "We"), "We packed tents, rope, etc. and left.");
    }

    #[test]
    fn initials_do_not_end_a_sentence() {
        assert_eq!(sentence("J. Smith wrote it. Nobody read it.", "wrote"), "J. Smith wrote it.");
    }

    #[test]
    fn text_with_no_final_stop_is_still_a_sentence() {
        assert_eq!(sentence("No stop here", "stop"), "No stop here");
    }

    #[test]
    fn paragraphs_split_on_blank_lines_and_headlines_stand_alone() {
        let found: Vec<_> = paragraphs(ESSAY).into_iter().map(|r| ESSAY[r].to_string()).collect();
        assert_eq!(found.len(), 4);
        assert_eq!(found[0], "# The Harbor");
        assert!(found[2].starts_with("A second block\nruns"));
    }

    #[test]
    fn crlf_lines_keep_clean_ranges() {
        let text = "one\r\n\r\ntwo\r\n";
        let found: Vec<_> = paragraphs(text).into_iter().map(|r| text[r].to_string()).collect();
        assert_eq!(found, ["one", "two"]);
    }

    #[test]
    fn a_blank_line_is_in_no_paragraph() {
        assert_eq!(paragraph_at("one\n\n\ntwo", 4), None);
    }

    #[test]
    fn a_selection_uses_the_level_it_matches() {
        let text = "One two. Three four.\n\nNext block.";
        assert_eq!(target_for_selection(text, 4..7), Some((Level::Word, 4..7)));
        assert_eq!(target_for_selection(text, 0..8), Some((Level::Sentence, 0..8)));
        assert_eq!(target_for_selection(text, 0..20), Some((Level::Paragraph, 0..20)));
    }

    #[test]
    fn an_arbitrary_selection_keeps_its_exact_range() {
        let text = "One two. Three four.";
        assert_eq!(target_for_selection(text, 4..14), Some((Level::Sentence, 4..14)));
    }

    #[test]
    fn surrounding_spaces_do_not_count() {
        let text = "One two three";
        assert_eq!(target_for_selection(text, 3..8), Some((Level::Word, 4..7)));
    }

    #[test]
    fn a_blank_selection_means_nothing() {
        assert_eq!(target_for_selection("a   b", 1..4), None);
    }
}
