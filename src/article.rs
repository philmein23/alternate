//! Keeps "a" and "an" right when the word after them changes.

use std::ops::Range;

/// Words spelled with a vowel that sound like a consonant.
const SAYS_CONSONANT: &[&str] = &["one", "once", "uni", "use", "usual", "user", "uter", "utop", "euro", "eu", "ewe", "ubiq", "unanim"];
/// Exceptions to `SAYS_CONSONANT`, where "un" is the ordinary prefix.
const SAYS_VOWEL_UN: &[&str] = &["unim", "unin", "unid"];
/// Words spelled with a consonant that sound like a vowel.
const SAYS_VOWEL: &[&str] = &["hour", "honest", "honor", "honour", "heir"];

/// Whether `word` takes "an", or `None` when it doesn't begin with a letter.
pub fn wants_an(word: &str) -> Option<bool> {
    let lower = word.to_lowercase();
    let first = lower.chars().next().filter(|c| c.is_alphabetic())?;
    if SAYS_VOWEL.iter().any(|p| lower.starts_with(p)) {
        return Some(true);
    }
    if SAYS_VOWEL_UN.iter().any(|p| lower.starts_with(p)) {
        return Some(true);
    }
    if SAYS_CONSONANT.iter().any(|p| lower.starts_with(p)) {
        return Some(false);
    }
    Some(matches!(first, 'a' | 'e' | 'i' | 'o' | 'u'))
}

fn with_case_of(old: &str, an: bool) -> String {
    let lower = if an { "an" } else { "a" };
    if old.len() > 1 && old.chars().all(char::is_uppercase) {
        return lower.to_uppercase();
    }
    if old.chars().next().is_some_and(char::is_uppercase) {
        let mut out = String::from("A");
        out.push_str(&lower[1..]);
        return out;
    }
    lower.to_string()
}

/// When an article sits just before byte `at` and doesn't suit `next`, its range and replacement.
pub fn fix(text: &str, at: usize, next: &str) -> Option<(Range<usize>, String)> {
    let before = &text[..at];
    let gap = before.trim_end();
    if gap.len() == before.len() || !before[gap.len()..].chars().all(|c| c == ' ') {
        return None;
    }
    let start = gap.rfind(|c: char| !c.is_alphabetic()).map_or(0, |i| i + gap[i..].chars().next().map_or(1, char::len_utf8));
    let article = &gap[start..];
    if !matches!(article.to_lowercase().as_str(), "a" | "an") {
        return None;
    }
    let word = next.trim_start();
    let word = &word[..word.find(|c: char| !c.is_alphanumeric() && c != '\'' && c != '’').unwrap_or(word.len())];
    let fixed = with_case_of(article, wants_an(word)?);
    (fixed != article).then_some((start..gap.len(), fixed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, word: &str, next: &str) -> Option<String> {
        let at = text.find(word).unwrap();
        fix(text, at, next).map(|(r, s)| format!("{}{}{}", &text[..r.start], s, &text[r.end..]))
    }

    #[test]
    fn a_becomes_an_before_a_vowel() {
        assert_eq!(run("Take a thumbtack.", "thumbtack", "eraser").unwrap(), "Take an thumbtack.");
    }

    #[test]
    fn an_becomes_a_before_a_consonant() {
        assert_eq!(run("Take an eraser.", "eraser", "thumbtack").unwrap(), "Take a eraser.");
    }

    #[test]
    fn a_right_article_is_left_alone() {
        assert_eq!(run("Take a thumbtack.", "thumbtack", "pencil"), None);
        assert_eq!(run("Take an eraser.", "eraser", "apple"), None);
    }

    #[test]
    fn capitals_are_kept() {
        assert_eq!(run("A thumbtack.", "thumbtack", "eraser").unwrap(), "An thumbtack.");
        assert_eq!(run("An eraser.", "eraser", "pen").unwrap(), "A eraser.");
        assert_eq!(run("AN ERASER", "ERASER", "PEN").unwrap(), "A ERASER");
    }

    #[test]
    fn silent_h_takes_an() {
        assert_eq!(run("a day", "day", "hour").unwrap(), "an day");
        assert_eq!(run("an hour", "hour", "honest man"), None);
    }

    #[test]
    fn a_vowel_that_sounds_like_y_takes_a() {
        assert_eq!(run("an hour", "hour", "university").unwrap(), "a hour");
        assert_eq!(run("a day", "day", "one-off"), None);
        assert_eq!(run("a day", "day", "European"), None);
    }

    #[test]
    fn the_prefix_un_takes_an() {
        assert_eq!(run("a day", "day", "uninteresting").unwrap(), "an day");
        assert_eq!(run("a day", "day", "unimportant").unwrap(), "an day");
        assert_eq!(run("a day", "day", "uniform"), None);
    }

    #[test]
    fn no_preceding_article_changes_nothing() {
        assert_eq!(run("the day", "day", "hour"), None);
        assert_eq!(run("day", "day", "hour"), None);
    }

    #[test]
    fn a_word_ending_in_a_is_not_an_article() {
        assert_eq!(run("data day", "day", "hour"), None);
    }

    #[test]
    fn a_number_is_left_alone() {
        assert_eq!(run("a day", "day", "8 days"), None);
    }

    #[test]
    fn a_line_break_between_them_is_left_alone() {
        assert_eq!(run("a\nday", "day", "hour"), None);
    }
}
