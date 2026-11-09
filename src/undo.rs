//! One undo for text and alternatives together. The field keeps its own text history. Beside it, a
//! stack remembers what the lists looked like around each change, tied to the text after that change.

use rune::{Key, KeyEvent, Tx};

use crate::model::{Ghost, Target, TargetId};
use crate::State;

const LIMIT: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Lists {
    text: String,
    targets: Vec<Target>,
    ghosts: Vec<Ghost>,
}

#[derive(Clone, Debug)]
pub struct ListChange {
    before: Lists,
    after: Lists,
    /// The field's history position around the change, which says exactly which text step this is.
    depth_before: usize,
    depth_after: usize,
}

#[derive(Debug, Default)]
pub struct History {
    undo: Vec<ListChange>,
    redo: Vec<ListChange>,
}

impl History {
    pub fn clear_redo(&mut self) {
        self.redo.clear();
    }
}

impl State {
    fn lists(&self) -> Lists {
        Lists { text: self.doc.text.clone(), targets: self.doc.targets.clone(), ghosts: self.doc.ghosts.clone() }
    }

    /// Runs a change to the lists so that undo can bring back what was there before it.
    pub fn with_undo<R>(&mut self, f: impl FnOnce(&mut State) -> R) -> R {
        let before = self.lists();
        let depth_before = self.edit.undo_depth();
        let out = f(self);
        let after = self.lists();
        if before != after {
            let text_changed = before.text != after.text;
            self.history.undo.push(ListChange { before, after, depth_before, depth_after: self.edit.undo_depth() });
            if self.history.undo.len() > LIMIT {
                self.history.undo.remove(0);
            }
            // A change that leaves the text alone leaves the field's redo alone too, so the two
            // histories have to keep the same steps.
            if text_changed {
                self.history.redo.clear();
            }
        }
        out
    }

    fn restore(&mut self, lists: &Lists) {
        self.doc.targets = lists.targets.clone();
        for t in &mut self.doc.targets {
            let active_text = t.alts.get(t.active).map(|a| a.text.clone());
            t.alts.retain(|a| !a.text.trim().is_empty());
            t.active = active_text.and_then(|text| t.alts.iter().position(|a| a.text == text)).unwrap_or(0);
        }
        self.doc.ghosts = lists.ghosts.clone();
        self.doc.text = self.edit.text.clone();
        self.hover = None;
        self.fade = None;
        self.touch();
    }

    /// Applies saved lists if the field landed on their text. If it didn't, the ranges in them
    /// mean nothing, so the lists are moved to match the field instead.
    fn settle(&mut self, lists: &Lists) {
        debug_assert_eq!(self.edit.text, lists.text, "the field and the list history fell out of step");
        if self.edit.text == lists.text {
            self.restore(lists);
        } else {
            self.sync_after_history();
        }
    }

    pub fn undo(&mut self) {
        let matches = self.history.undo.last().is_some_and(|c| c.depth_after == self.edit.undo_depth() && c.after.text == self.edit.text);
        if !matches {
            self.edit.undo();
            self.sync_after_history();
            return;
        }
        let change = self.history.undo.pop().expect("just checked");
        if change.before.text != change.after.text {
            self.edit.undo();
        }
        self.settle(&change.before);
        self.history.redo.push(change);
    }

    pub fn redo(&mut self) {
        let matches = self.history.redo.last().is_some_and(|c| c.depth_before == self.edit.undo_depth() && c.before.text == self.edit.text);
        if !matches {
            self.edit.redo();
            self.sync_after_history();
            return;
        }
        let change = self.history.redo.pop().expect("just checked");
        if change.before.text != change.after.text {
            self.edit.redo();
        }
        self.settle(&change.after);
        self.history.undo.push(change);
    }

    fn sync_after_history(&mut self) {
        if self.doc.sync_text(&self.edit.text, Some(self.edit.focus)).is_some() {
            self.touch();
        }
    }

    /// Removes the alternative on the page and shows the next. False when there's no other to show.
    pub fn delete_alt(&mut self, id: TargetId) -> bool {
        let Some(t) = self.doc.targets.iter().find(|t| t.id == id) else { return false };
        if t.alts.len() < 2 {
            return false;
        }
        self.with_undo(|s| {
            let t = s.doc.targets.iter_mut().find(|t| t.id == id).expect("just found");
            t.alts.remove(t.active);
            let next = t.active.min(t.alts.len() - 1);
            s.show_alt(id, next);
            s.fade = None;
        });
        true
    }

    /// Drops the mark of a target with nothing left to swap to, once the pointer has moved off it.
    pub fn prune_target(&mut self, id: TargetId) {
        self.doc.targets.retain(|t| t.id != id || t.alts.len() >= 2);
        self.touch();
    }
}

pub fn handle_key(s: &mut State, ev: &KeyEvent, _tx: &mut Tx) -> bool {
    let Key::Text(k) = &ev.key else { return false };
    if !ev.mods.cmd || !k.eq_ignore_ascii_case("z") || s.panel.is_some() {
        return false;
    }
    if ev.mods.shift {
        s.redo();
    } else {
        s.undo();
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Alt, Doc, Level, Source};

    fn state() -> State {
        let mut doc = Doc::new("the cat sat on a mat");
        for (word, alts) in [("cat", vec!["cat", "dog", "owl"]), ("mat", vec!["mat", "rug"])] {
            let at = doc.text.find(word).unwrap();
            let id = doc.next_id();
            doc.targets.push(Target { id, range: at..at + word.len(), level: Level::Word, original: word.to_string(), alts: alts.iter().map(|a| Alt { text: a.to_string(), source: Source::Mine }).collect(), active: 0 });
        }
        State::with_doc(doc)
    }

    fn consistent(s: &State) {
        assert_eq!(s.doc.text, s.edit.text);
        for t in &s.doc.targets {
            assert_eq!(s.doc.text[t.range.clone()], t.alts[t.active].text, "target {} in {:?}", t.id, s.doc.text);
        }
    }

    #[test]
    fn delete_shows_the_next_and_undo_restores_both() {
        let mut s = state();
        assert!(s.delete_alt(1));
        assert_eq!(s.edit.text, "the dog sat on a mat");
        assert_eq!(s.doc.targets[0].alts.len(), 2);
        s.undo();
        assert_eq!(s.edit.text, "the cat sat on a mat");
        assert_eq!(s.doc.targets[0].alts.len(), 3);
        consistent(&s);
    }

    #[test]
    fn redo_deletes_again() {
        let mut s = state();
        s.delete_alt(1);
        s.undo();
        s.redo();
        assert_eq!(s.edit.text, "the dog sat on a mat");
        assert_eq!(s.doc.targets[0].alts.len(), 2);
        consistent(&s);
    }

    #[test]
    fn typing_then_undoing_past_a_swap_keeps_text_and_list_in_step() {
        let mut s = state();
        s.with_undo(|s| s.step_alt(1, 1));
        s.edit.set_selection(0, 0);
        s.edit.insert("x");
        s.sync_field();
        s.undo();
        assert_eq!(s.edit.text, "the dog sat on a mat");
        consistent(&s);
        s.undo();
        assert_eq!(s.edit.text, "the cat sat on a mat");
        assert_eq!(s.doc.targets[0].active, 0);
        consistent(&s);
    }

    #[test]
    fn typing_after_undo_clears_redo() {
        let mut s = state();
        s.with_undo(|s| s.step_alt(1, 1));
        s.undo();
        s.edit.set_selection(0, 0);
        s.edit.insert("x");
        s.sync_field();
        s.redo();
        assert_eq!(s.edit.text, "xthe cat sat on a mat");
        consistent(&s);
    }

    #[test]
    fn the_last_alternative_cannot_be_deleted_by_this_path() {
        let mut s = state();
        s.delete_alt(2);
        assert!(!s.delete_alt(2));
        assert_eq!(s.edit.text, "the cat sat on a rug");
    }

    #[test]
    fn a_target_with_one_left_loses_its_mark_when_the_pointer_leaves() {
        let mut s = state();
        s.delete_alt(2);
        s.prune_target(2);
        assert_eq!(s.doc.targets.len(), 1);
        s.undo();
        assert_eq!(s.doc.targets.len(), 2);
        consistent(&s);
    }

    #[test]
    fn any_mix_of_edits_swaps_deletes_undos_and_redos_stays_consistent() {
        let mut seed = 0x1234_5678_9ABC_DEF1_u64;
        let mut next = move |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed as usize % n
        };
        for _ in 0..60 {
            let mut s = state();
            for _ in 0..120 {
                match next(6) {
                    0 => {
                        let at = next(s.edit.text.len() + 1);
                        if s.edit.text.is_char_boundary(at) {
                            s.edit.set_selection(at, at);
                            s.edit.insert("z");
                            s.sync_field();
                        }
                    }
                    1 => {
                        if let Some(id) = s.doc.targets.first().map(|t| t.id) {
                            s.with_undo(|s| s.step_alt(id, 1));
                        }
                    }
                    2 => {
                        if let Some(id) = s.doc.targets.last().map(|t| t.id) {
                            s.delete_alt(id);
                        }
                    }
                    3 | 4 => s.undo(),
                    _ => s.redo(),
                }
                consistent(&s);
            }
        }
    }
}
