//! Ghosted text: faded on the page, still there, and left out of what gets posted.

use std::ops::Range;

use crate::State;
use crate::editor::Mode;
use crate::segment;

impl State {
    pub fn ghost_at(&self, pos: usize) -> Option<Range<usize>> {
        self.doc.ghosts.iter().find(|g| g.range.contains(&pos)).map(|g| g.range.clone())
    }

    /// What a keyboard command acts on: the selection, or else the word at the caret.
    pub fn caret_range(&self) -> Option<Range<usize>> {
        let (a, b) = self.edit.range();
        if a != b {
            return Some(a..b);
        }
        self.ghost_at(a).or_else(|| segment::word_at(&self.doc.text, a))
    }

    /// Revives the ghost that `range` starts in, or fades `range` if it starts in none.
    pub fn toggle_ghost(&mut self, range: Range<usize>) {
        if range.is_empty() {
            return;
        }
        self.with_undo(|s| {
            match s.ghost_at(range.start) {
                Some(ghost) => s.doc.ghosts.retain(|g| g.range != ghost),
                None => s.doc.add_ghost(range.clone()),
            }
            s.mode = Mode::Active;
            s.touch();
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::State;
    use crate::model::Doc;

    #[test]
    fn ghosting_then_toggling_again_revives() {
        let mut s = State::with_doc(Doc::new("one two three"));
        s.toggle_ghost(4..7);
        assert_eq!(s.doc.ghosts[0].range, 4..7);
        s.toggle_ghost(4..7);
        assert!(s.doc.ghosts.is_empty());
    }

    #[test]
    fn undo_brings_a_ghost_back_without_touching_the_text() {
        let mut s = State::with_doc(Doc::new("one two three"));
        s.toggle_ghost(4..7);
        s.undo();
        assert!(s.doc.ghosts.is_empty());
        assert_eq!(s.edit.text, "one two three");
        s.redo();
        assert_eq!(s.doc.ghosts[0].range, 4..7);
    }

    #[test]
    fn the_caret_means_its_word_or_the_ghost_it_is_in() {
        let mut s = State::with_doc(Doc::new("one two three"));
        s.edit.set_selection(5, 5);
        assert_eq!(s.caret_range(), Some(4..7));
        s.toggle_ghost(0..7);
        assert_eq!(s.caret_range(), Some(0..7));
    }
}
