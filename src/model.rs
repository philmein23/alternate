//! What a document is, apart from how it looks.

use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::rebase::{self, Edit};

pub type TargetId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Word,
    Sentence,
    Paragraph,
}

/// Who wrote an alternative: shown as a bullet or a small robot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Mine,
    Ai,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alt {
    pub text: String,
    pub source: Source,
}

/// A word, sentence or paragraph holding alternatives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub id: TargetId,
    pub range: Range<usize>,
    pub level: Level,
    /// The text from before any swapping. The mark lights up when the page differs from it.
    pub original: String,
    pub alts: Vec<Alt>,
    pub active: usize,
}

impl Target {
    pub fn is_original(&self, text: &str) -> bool {
        text[self.range.clone()] == self.original
    }
}

/// Text faded on the page without being removed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ghost {
    pub range: Range<usize>,
}

/// A place a Lab pass pointed at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LabMark {
    pub range: Range<usize>,
    pub reason: String,
    pub pass: crate::lab::Pass,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Doc {
    /// Where the last Lab pass pointed. Follows edits, and isn't saved.
    pub marks: Vec<LabMark>,
    pub text: String,
    pub targets: Vec<Target>,
    pub ghosts: Vec<Ghost>,
    pub overflow: String,
    next_id: TargetId,
}

impl Doc {
    pub fn new(text: impl Into<String>) -> Doc {
        Doc { text: text.into(), ..Doc::default() }
    }

    pub fn next_id(&mut self) -> TargetId {
        self.next_id += 1;
        self.next_id
    }

    /// Takes in the field's text after any change, with the caret's byte offset, and moves every
    /// range to match. Typing inside a target edits its current alternative.
    pub fn sync_text(&mut self, new: &str, caret: Option<usize>) -> Option<Edit> {
        let edit = rebase::diff(&self.text, new, caret)?;
        self.text = new.to_string();
        self.rebase_all(edit, None);
        Some(edit)
    }

    /// Replaces a span of the text directly and moves every range. `keep` names a target the
    /// caller will place itself.
    pub fn splice(&mut self, range: Range<usize>, with: &str, keep: Option<TargetId>) -> Edit {
        self.text.replace_range(range.clone(), with);
        let edit = Edit { start: range.start, removed: range.len(), inserted: with.len() };
        self.rebase_all(edit, keep);
        edit
    }

    fn rebase_all(&mut self, edit: Edit, keep: Option<TargetId>) {
        let text = &self.text;
        self.targets.retain_mut(|t| {
            if Some(t.id) == keep {
                return true;
            }
            let was_original = t.alts.get(t.active).is_some_and(|a| a.text == t.original);
            let touched = edit.start < t.range.end && edit.start + edit.removed > t.range.start;
            let Some(range) = rebase::rebase(t.range.clone(), edit) else { return false };
            t.range = range;
            if touched && let Some(alt) = t.alts.get_mut(t.active) {
                alt.text = text[t.range.clone()].to_string();
                if was_original {
                    t.original = alt.text.clone();
                }
            }
            true
        });
        self.ghosts = merged(self.ghosts.iter().filter_map(|g| rebase::rebase(g.range.clone(), edit)).collect());
        self.marks.retain_mut(|m| match rebase::rebase(m.range.clone(), edit) {
            Some(range) => {
                m.range = range;
                true
            }
            None => false,
        });
    }

    /// Fades `range`, joining any ghost it touches.
    pub fn add_ghost(&mut self, range: Range<usize>) {
        let mut ranges: Vec<_> = self.ghosts.iter().map(|g| g.range.clone()).collect();
        ranges.push(range);
        self.ghosts = merged(ranges);
    }
}

/// Sorted ranges with overlapping and touching ones joined.
fn merged(mut ranges: Vec<Range<usize>>) -> Vec<Ghost> {
    ranges.sort_by_key(|r| r.start);
    let mut out: Vec<Ghost> = Vec::with_capacity(ranges.len());
    for r in ranges {
        match out.last_mut() {
            Some(last) if r.start <= last.range.end => last.range.end = last.range.end.max(r.end),
            _ => out.push(Ghost { range: r }),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_target(text: &str, word: &str, alts: &[&str]) -> Doc {
        let mut doc = Doc::new(text);
        let start = text.find(word).unwrap();
        let id = doc.next_id();
        doc.targets.push(Target {
            id,
            range: start..start + word.len(),
            level: Level::Word,
            original: word.to_string(),
            alts: alts.iter().map(|t| Alt { text: t.to_string(), source: Source::Mine }).collect(),
            active: 0,
        });
        doc
    }

    #[test]
    fn typing_inside_a_target_edits_the_current_alternative() {
        let mut doc = with_target("the cat sat", "cat", &["cat", "dog"]);
        doc.sync_text("the coat sat", Some(6));
        let t = &doc.targets[0];
        assert_eq!(t.alts[0].text, "coat");
        assert_eq!(t.alts[1].text, "dog");
        assert_eq!(&doc.text[t.range.clone()], "coat");
    }

    #[test]
    fn editing_the_original_row_keeps_it_the_original() {
        let mut doc = with_target("the cat sat", "cat", &["cat", "dog"]);
        doc.sync_text("the coat sat", Some(6));
        assert!(doc.targets[0].is_original(&doc.text));
    }

    #[test]
    fn editing_another_row_leaves_the_original_alone() {
        let mut doc = with_target("the dog sat", "dog", &["cat", "dog"]);
        doc.targets[0].original = "cat".to_string();
        doc.targets[0].active = 1;
        doc.sync_text("the dogs sat", Some(5));
        assert_eq!(doc.targets[0].original, "cat");
        assert!(!doc.targets[0].is_original(&doc.text));
    }

    #[test]
    fn typing_elsewhere_leaves_the_alternatives_alone() {
        let mut doc = with_target("the cat sat", "cat", &["cat", "dog"]);
        doc.sync_text("xthe cat sat", Some(1));
        doc.sync_text("xthe cat sat!", Some(13));
        let t = &doc.targets[0];
        assert_eq!(t.range, 5..8);
        assert_eq!(t.alts[0].text, "cat");
    }

    #[test]
    fn deleting_a_whole_target_removes_it() {
        let mut doc = with_target("the cat sat", "cat", &["cat", "dog"]);
        doc.sync_text("the sat", Some(4));
        assert!(doc.targets.is_empty());
    }

    #[test]
    fn deleting_part_of_a_target_shrinks_it_and_updates_the_row() {
        let mut doc = with_target("the cat sat", "cat", &["cat", "dog"]);
        doc.sync_text("tat sat", Some(1));
        let t = &doc.targets[0];
        assert_eq!(t.range, 1..3);
        assert_eq!(t.alts[0].text, "at");
    }

    #[test]
    fn ghosts_move_with_the_text() {
        let mut doc = Doc::new("one two three");
        doc.add_ghost(4..7);
        doc.sync_text("xone two three", Some(1));
        assert_eq!(doc.ghosts, vec![Ghost { range: 5..8 }]);
    }

    #[test]
    fn a_ghost_deleted_whole_is_gone() {
        let mut doc = Doc::new("one two three");
        doc.add_ghost(4..7);
        doc.sync_text("one three", Some(4));
        assert!(doc.ghosts.is_empty());
    }

    #[test]
    fn adjacent_ghosts_merge() {
        let mut doc = Doc::new("one two three");
        doc.add_ghost(0..3);
        doc.add_ghost(3..7);
        assert_eq!(doc.ghosts, vec![Ghost { range: 0..7 }]);
    }

    #[test]
    fn deleting_the_text_between_two_ghosts_merges_them() {
        let mut doc = Doc::new("one two three");
        doc.add_ghost(0..3);
        doc.add_ghost(4..7);
        doc.sync_text("onetwo three", Some(3));
        assert_eq!(doc.ghosts, vec![Ghost { range: 0..6 }]);
    }

    #[test]
    fn multibyte_targets_stay_on_boundaries() {
        let mut doc = with_target("une idée claire", "idée", &["idée"]);
        doc.sync_text("une 語idée claire", Some(7));
        let t = &doc.targets[0];
        assert_eq!(&doc.text[t.range.clone()], "idée");
    }
}
