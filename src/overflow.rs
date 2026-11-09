//! Overflow: a side stash for text that doesn't belong on the page yet.

use std::ops::Range;

use rune::{Key, KeyEvent, TextEdit, Tx};

use crate::State;
use crate::editor::Mode;

/// Taking a word from the middle of a line shouldn't leave two spaces where it was.
pub fn tidy(text: &str, range: Range<usize>) -> Range<usize> {
    let before = text[..range.start].chars().next_back();
    let after = text[range.end..].chars().next();
    let closes = after.is_none_or(|c| c == ' ' || c == '\n' || c.is_ascii_punctuation());
    if before == Some(' ') && closes { range.start - 1..range.end } else { range }
}

impl State {
    pub fn set_overflow(&mut self, text: String) {
        self.overflow_edit = TextEdit::new(text.clone());
        self.doc.overflow = text;
    }

    pub fn overflow_changed(&mut self) {
        self.doc.overflow = self.overflow_edit.text.clone();
        self.touch();
    }

    pub fn toggle_overflow(&mut self) {
        self.overflow_open = !self.overflow_open;
        if !self.overflow_open {
            self.overflow_edit.focused = false;
            self.edit.focused = true;
        }
    }

    /// Moves `range` off the page and into Overflow.
    pub fn stash(&mut self, range: Range<usize>) {
        let taken = self.doc.text[range.clone()].trim().to_string();
        if taken.is_empty() {
            return;
        }
        let range = tidy(&self.doc.text, range);
        self.with_undo(|s| {
            s.edit.set_selection(range.start, range.end);
            s.edit.insert("");
            s.sync_field();
            let joined = if s.doc.overflow.is_empty() { taken } else { format!("{}\n{taken}", s.doc.overflow) };
            s.set_overflow(joined);
            s.overflow_open = true;
            s.mode = Mode::Active;
            s.touch();
        });
    }

    /// The selected Overflow text, or the line the caret is on.
    fn overflow_snippet(&self) -> Option<String> {
        let (a, b) = self.overflow_edit.range();
        let text = &self.overflow_edit.text;
        let snippet = if a != b {
            &text[a..b]
        } else {
            let start = text[..a].rfind('\n').map_or(0, |i| i + 1);
            let end = text[a..].find('\n').map_or(text.len(), |i| a + i);
            &text[start..end]
        };
        (!snippet.trim().is_empty()).then(|| snippet.to_string())
    }

    /// Puts Overflow text in the page at `at`. It stays in Overflow too, in case it's wanted again.
    pub fn use_overflow(&mut self, at: usize) -> bool {
        let Some(snippet) = self.overflow_snippet() else { return false };
        self.edit.set_selection(at, at);
        self.edit.insert(&snippet);
        self.sync_field();
        self.overflow_edit.focused = false;
        self.edit.focused = true;
        true
    }
}

/// Keys inside the Overflow field.
pub fn field_key(s: &mut State, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if ev.mods.ctrl && ev.key == Key::Enter {
        let at = s.edit.range().1;
        return s.use_overflow(at);
    }
    if ev.mods.cmd && !matches!(&ev.key, Key::Text(k) if k.eq_ignore_ascii_case("z")) {
        return crate::editor::handle_key(s, ev, tx);
    }
    false
}

#[cfg(test)]
mod tests {
    use crate::State;
    use crate::model::{Alt, Doc, Level, Source, Target};

    fn state() -> State {
        State::with_doc(Doc::new("one two three"))
    }

    #[test]
    fn stashing_moves_the_text_and_closes_the_gap() {
        let mut s = state();
        s.stash(4..7);
        assert_eq!(s.edit.text, "one three");
        assert_eq!(s.doc.overflow, "two");
        assert_eq!(s.overflow_edit.text, "two");
        assert!(s.overflow_open);
    }

    #[test]
    fn stashing_again_adds_a_line() {
        let mut s = state();
        s.stash(4..7);
        s.stash(0..3);
        assert_eq!(s.doc.overflow, "two\none");
    }

    #[test]
    fn undo_restores_both_panes_and_redo_moves_it_again() {
        let mut s = state();
        s.stash(4..7);
        s.undo();
        assert_eq!(s.edit.text, "one two three");
        assert_eq!(s.doc.text, s.edit.text);
        assert_eq!(s.doc.overflow, "");
        assert_eq!(s.overflow_edit.text, "");
        s.redo();
        assert_eq!(s.edit.text, "one three");
        assert_eq!(s.overflow_edit.text, "two");
    }

    #[test]
    fn undo_leaves_what_was_typed_in_overflow_since() {
        let mut s = state();
        s.stash(4..7);
        s.overflow_edit.set_selection(3, 3);
        s.overflow_edit.insert(" more");
        s.overflow_changed();
        s.undo();
        assert_eq!(s.edit.text, "one two three");
        assert_eq!(s.doc.overflow, "two more");
    }

    #[test]
    fn stashing_a_whole_target_removes_it_cleanly() {
        let mut doc = Doc::new("one two three");
        let id = doc.next_id();
        doc.targets.push(Target { id, range: 4..7, level: Level::Word, original: "two".into(), alts: vec![Alt { text: "two".into(), source: Source::Mine }, Alt { text: "2".into(), source: Source::Mine }], active: 0 });
        let mut s = State::with_doc(doc);
        s.stash(4..7);
        assert!(s.doc.targets.is_empty());
        s.undo();
        assert_eq!(s.doc.targets.len(), 1);
        assert_eq!(&s.doc.text[s.doc.targets[0].range.clone()], "two");
    }

    #[test]
    fn using_overflow_inserts_the_selection_at_the_page_caret() {
        let mut s = state();
        s.set_overflow("alpha\nbeta".to_string());
        s.overflow_edit.set_selection(6, 10);
        assert!(s.use_overflow(3));
        assert_eq!(s.edit.text, "onebeta two three");
        assert_eq!(s.doc.text, s.edit.text);
        assert_eq!(s.doc.overflow, "alpha\nbeta");
    }

    #[test]
    fn with_no_selection_the_caret_line_is_used() {
        let mut s = state();
        s.set_overflow("alpha\nbeta".to_string());
        s.overflow_edit.set_selection(2, 2);
        s.use_overflow(0);
        assert!(s.edit.text.starts_with("alphaone"));
    }

    #[test]
    fn an_empty_overflow_inserts_nothing() {
        let mut s = state();
        assert!(!s.use_overflow(0));
        assert_eq!(s.edit.text, "one two three");
    }
}
