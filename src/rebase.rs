//! Keeps byte ranges right as the text changes. Rune reports no edit deltas, so an edit is recovered
//! by comparing the text before and after it.

use std::ops::Range;

/// One replaced span: `removed` bytes at `start` in the old text became `inserted` bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edit {
    pub start: usize,
    pub removed: usize,
    pub inserted: usize,
}

impl Edit {
    fn old_end(self) -> usize {
        self.start + self.removed
    }

    /// `p` moved by the edit's change in length, for a position after the edit.
    fn shifted(self, p: usize) -> usize {
        p + self.inserted - self.removed
    }
}

/// The single edit turning `old` into `new`, or `None` when they are equal. `caret` is the caret's
/// byte offset in `new`. Where text repeats, as when typing `e` before `effect`, more than one edit
/// explains the change, and only the caret says which one happened: it sits at the end of the
/// inserted text, so everything after it is unchanged.
pub fn diff(old: &str, new: &str, caret: Option<usize>) -> Option<Edit> {
    if old == new {
        return None;
    }
    let (a, b) = (old.as_bytes(), new.as_bytes());
    let suffix_limit = caret.map_or(a.len().min(b.len()), |c| b.len().saturating_sub(c).min(a.len()));
    let mut suffix = a.iter().rev().zip(b.iter().rev()).take(suffix_limit).take_while(|(x, y)| x == y).count();
    while suffix > 0 && !(old.is_char_boundary(a.len() - suffix) && new.is_char_boundary(b.len() - suffix)) {
        suffix -= 1;
    }
    let prefix_limit = (a.len() - suffix).min(b.len() - suffix);
    let mut prefix = a.iter().zip(b).take(prefix_limit).take_while(|(x, y)| x == y).count();
    while prefix > 0 && !(old.is_char_boundary(prefix) && new.is_char_boundary(prefix)) {
        prefix -= 1;
    }
    Some(Edit { start: prefix, removed: a.len() - suffix - prefix, inserted: b.len() - suffix - prefix })
}

/// Where `r` lands after `edit`, or `None` when the edit removed all of it.
///
/// An insertion exactly at either edge stays outside the range: typing before a range shifts it and
/// typing after it leaves it alone. An edit inside the range grows or shrinks it. An edit reaching
/// over one edge keeps what survives, and the inserted text falls outside.
pub fn rebase(r: Range<usize>, edit: Edit) -> Option<Range<usize>> {
    let (a, b) = (edit.start, edit.old_end());
    if b <= r.start {
        return Some(edit.shifted(r.start)..edit.shifted(r.end));
    }
    if a >= r.end {
        return Some(r);
    }
    match (a <= r.start, b >= r.end) {
        (true, true) => None,
        (true, false) if a < r.start => Some(a + edit.inserted..edit.shifted(r.end)),
        (false, true) => Some(r.start..a),
        _ => Some(r.start..edit.shifted(r.end)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(old: &str, new: &str, caret: Option<usize>, r: Range<usize>) -> Option<Range<usize>> {
        diff(old, new, caret).and_then(|e| rebase(r, e))
    }

    #[test]
    fn equal_text_is_no_edit() {
        assert_eq!(diff("same", "same", None), None);
    }

    #[test]
    fn typing_before_a_range_shifts_it() {
        assert_eq!(apply("the cat", "xthe cat", Some(1), 4..7), Some(5..8));
    }

    #[test]
    fn typing_right_at_the_start_shifts_it() {
        assert_eq!(apply("the cat", "the xcat", Some(5), 4..7), Some(5..8));
    }

    #[test]
    fn typing_after_a_range_leaves_it() {
        assert_eq!(apply("the cat", "the cats", Some(8), 0..3), Some(0..3));
    }

    #[test]
    fn typing_right_at_the_end_leaves_it() {
        assert_eq!(apply("the cat", "the cats", Some(8), 4..7), Some(4..7));
    }

    #[test]
    fn typing_inside_grows_it() {
        assert_eq!(apply("the cat", "the coat", Some(6), 4..7), Some(4..8));
    }

    #[test]
    fn replacing_inside_resizes_it() {
        assert_eq!(apply("the cat", "the cloat", Some(7), 4..7), Some(4..9));
    }

    #[test]
    fn deleting_across_the_whole_range_removes_it() {
        assert_eq!(apply("the cat sat", "the sat", Some(4), 4..7), None);
        assert_eq!(apply("the cat sat", "t sat", Some(1), 4..7), None);
    }

    #[test]
    fn deleting_over_the_left_edge_keeps_the_rest() {
        assert_eq!(apply("the cat", "tat", Some(1), 4..7), Some(1..3));
    }

    #[test]
    fn deleting_over_the_right_edge_keeps_the_rest() {
        assert_eq!(apply("the cat sat", "the ca sat", None, 4..7), Some(4..6));
        assert_eq!(apply("the cat sat", "the cat", Some(7), 4..7), Some(4..7));
        assert_eq!(apply("the cat sat", "the cab", Some(7), 4..7), Some(4..6));
    }

    #[test]
    fn replacing_over_the_left_edge_puts_the_new_text_outside() {
        assert_eq!(apply("the cat", "thXYat", Some(4), 4..7), Some(4..6));
    }

    #[test]
    fn the_caret_settles_which_repeated_letter_was_typed() {
        assert_eq!(diff("the effect", "the eeffect", Some(5)), Some(Edit { start: 4, removed: 0, inserted: 1 }));
        assert_eq!(apply("the effect", "the eeffect", Some(5), 4..10), Some(5..11));
    }

    #[test]
    fn without_a_caret_a_repeat_counts_from_the_end() {
        assert_eq!(diff("aa", "aaa", None), Some(Edit { start: 0, removed: 0, inserted: 1 }));
    }

    #[test]
    fn a_shared_lead_byte_does_not_split_a_character() {
        // é is C3 A9 and è is C3 A8: they share a first byte.
        let e = diff("café", "cafè", Some(5)).unwrap();
        assert_eq!(e, Edit { start: 3, removed: 2, inserted: 2 });
    }

    #[test]
    fn a_shared_final_byte_does_not_split_a_character() {
        // Ã is C3 83 and Ń is C5 83: they share a last byte.
        let e = diff("xÃy", "xŃy", None).unwrap();
        assert_eq!(e, Edit { start: 1, removed: 2, inserted: 2 });
    }

    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0 as usize
        }

        fn below(&mut self, n: usize) -> usize {
            self.next() % n.max(1)
        }

        fn boundary(&mut self, s: &str) -> usize {
            let at = self.below(s.len() + 1);
            (at..=s.len()).find(|&i| s.is_char_boundary(i)).unwrap_or(s.len())
        }

        fn insertion(&mut self) -> String {
            let alphabet = ['a', 'e', ' ', 'é', '語', '🙂', '\n'];
            (0..self.below(5)).map(|_| alphabet[self.below(alphabet.len())]).collect()
        }
    }

    #[test]
    fn random_edits_keep_every_range_in_bounds_and_on_char_boundaries() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for _ in 0..200 {
            let mut text = String::from("Une très bonne idée 語 and a 🙂 at the end of the line.");
            let mut ranges: Vec<Range<usize>> = (0..6)
                .map(|_| {
                    let (a, b) = (rng.boundary(&text), rng.boundary(&text));
                    a.min(b)..a.max(b)
                })
                .filter(|r| !r.is_empty())
                .collect();
            for _ in 0..80 {
                let (a, b) = (rng.boundary(&text), rng.boundary(&text));
                let (a, b) = (a.min(b), a.max(b));
                let insert = rng.insertion();
                let mut new = String::with_capacity(text.len() + insert.len());
                new.push_str(&text[..a]);
                new.push_str(&insert);
                new.push_str(&text[b..]);
                let caret = a + insert.len();
                if let Some(edit) = diff(&text, &new, Some(caret)) {
                    assert_eq!(new[edit.start..edit.start + edit.inserted].len(), edit.inserted);
                    ranges = ranges.into_iter().filter_map(|r| rebase(r, edit)).collect();
                }
                text = new;
                for r in &ranges {
                    assert!(r.start < r.end, "{r:?} is empty in {text:?}");
                    assert!(r.end <= text.len(), "{r:?} is out of bounds in {text:?}");
                    assert!(text.is_char_boundary(r.start) && text.is_char_boundary(r.end), "{r:?} splits a character in {text:?}");
                }
            }
        }
    }
}
