//! Trying alternatives in place: hovering a target and pressing an arrow.

use std::ops::Range;
use std::time::Duration;

use rune::{Key, KeyEvent, Tx};

use crate::article;
use crate::editor::Mode;
use crate::model::TargetId;
use crate::rebase::Edit;
use crate::ui::theme;
use crate::State;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hover {
    pub id: TargetId,
    /// The pointer has moved since the last keystroke, so arrows and delete mean the target.
    pub armed: bool,
}

/// New text fading in after a swap.
#[derive(Clone, Debug, PartialEq)]
pub struct Fade {
    pub range: Range<usize>,
    pub started: Option<f64>,
}

impl Fade {
    /// How visible the new text is, from `now` in seconds.
    pub fn alpha(&self, now: f64) -> f32 {
        self.started.map_or(0.0, |s| (((now - s) / theme::SWAP_FADE as f64) as f32).clamp(0.0, 1.0))
    }
}

/// Where a caret at `p` lands after `edit`.
fn carried(p: usize, edit: Edit) -> usize {
    let end = edit.start + edit.removed;
    if p <= edit.start {
        p
    } else if p >= end {
        p + edit.inserted - edit.removed
    } else {
        edit.start + edit.inserted
    }
}

impl State {
    /// Puts alternative `next` on the page, in one undo step with any a/an fix, and keeps the caret.
    pub fn show_alt(&mut self, id: TargetId, next: usize) -> bool {
        let Some(i) = self.doc.targets.iter().position(|t| t.id == id) else { return false };
        let t = &self.doc.targets[i];
        let Some(new) = t.alts.get(next).map(|a| a.text.clone()) else { return false };
        let range = t.range.clone();
        let fix = article::fix(&self.doc.text, range.start, &new);
        let (anchor, focus) = (self.edit.anchor, self.edit.focus);

        self.edit.undo_group(|e| {
            e.set_selection(range.start, range.end);
            e.insert(&new);
            if let Some((r, fixed)) = &fix {
                e.set_selection(r.start, r.end);
                e.insert(fixed);
            }
        });

        let mut edits = vec![self.doc.splice(range.clone(), &new, Some(id))];
        let t = &mut self.doc.targets[i];
        t.range = range.start..range.start + new.len();
        t.active = next;
        if let Some((r, fixed)) = &fix {
            edits.push(self.doc.splice(r.clone(), fixed, None));
        }
        let carry = |p: usize| edits.iter().fold(p, |p, e| carried(p, *e));
        self.edit.set_selection(carry(anchor), carry(focus));
        self.fade = self.doc.targets.iter().find(|t| t.id == id).map(|t| Fade { range: t.range.clone(), started: None });
        self.touch();
        true
    }

    /// Moves a target to its next or previous alternative, wrapping at the ends.
    pub fn step_alt(&mut self, id: TargetId, dir: i32) -> bool {
        let Some(t) = self.doc.targets.iter().find(|t| t.id == id) else { return false };
        let n = t.alts.len() as i32;
        if n < 2 {
            return false;
        }
        let next = (t.active as i32 + dir).rem_euclid(n) as usize;
        self.with_undo(|s| s.show_alt(id, next))
    }

    /// The smallest target under a point in the field's own coordinates.
    fn target_at(&self, x: f32, y: f32) -> Option<TargetId> {
        let hit = self.hit.borrow();
        let area = |boxes: &[[f32; 4]]| boxes.iter().map(|b| b[2] * b[3]).sum::<f32>();
        hit.iter()
            .filter(|(_, boxes)| boxes.iter().any(|b| x >= b[0] && x <= b[0] + b[2] && y >= b[1] && y <= b[1] + b[3]))
            .min_by(|a, b| area(&a.1).total_cmp(&area(&b.1)))
            .map(|(id, _)| *id)
    }

    /// Follows the pointer. Any movement arms the target under it.
    pub fn pointer_moved(&mut self, x: f32, y: f32, tx: &mut Tx) {
        if self.mode != Mode::Active || self.panel.is_some() {
            return;
        }
        let hover = self.target_at(x, y).map(|id| Hover { id, armed: true });
        let old = self.hover.map(|h| h.id);
        if hover.map(|h| h.id) == old {
            tx.live_only();
        } else if let Some(id) = old {
            self.prune_target(id);
        }
        self.hover = hover;
    }

    pub fn pointer_left(&mut self) {
        if let Some(h) = self.hover.take() {
            self.prune_target(h.id);
        }
    }
}

/// Arrows on an armed target swap it. Typing disarms it, so arrows go back to the caret.
pub fn handle_key(s: &mut State, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let plain = !ev.mods.cmd && !ev.mods.ctrl && !ev.mods.alt && !ev.mods.shift;
    if plain
        && matches!(ev.key, Key::Backspace | Key::Delete)
        && let Some(Hover { id, armed: true }) = s.hover
        && s.panel.is_none()
        && s.delete_alt(id)
    {
        tx.wake_after(Duration::ZERO);
        return true;
    }
    let dir = match &ev.key {
        Key::Left if plain => -1,
        Key::Right if plain => 1,
        Key::Text(_) | Key::Enter | Key::Backspace | Key::Delete if !ev.mods.cmd => {
            if let Some(h) = &mut s.hover {
                h.armed = false;
            }
            return false;
        }
        _ => return false,
    };
    let Some(Hover { id, armed: true }) = s.hover else { return false };
    if !s.step_alt(id, dir) {
        return false;
    }
    tx.wake_after(Duration::ZERO);
    true
}

/// Advances the fade. True while the page needs redrawing.
pub fn poll(s: &mut State, now: f64, tx: &mut Tx) -> bool {
    let Some(fade) = &mut s.fade else { return false };
    let started = *fade.started.get_or_insert(now);
    if now - started >= theme::SWAP_FADE as f64 {
        s.fade = None;
    } else {
        s.fade_now = now;
        tx.wake_after(Duration::from_millis(8));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Alt, Doc, Level, Source, Target};

    fn state(text: &str, word: &str, alts: &[&str]) -> State {
        let mut doc = Doc::new(text);
        let at = text.find(word).unwrap();
        let id = doc.next_id();
        doc.targets.push(Target {
            id,
            range: at..at + word.len(),
            level: Level::Word,
            original: word.to_string(),
            alts: alts.iter().map(|a| Alt { text: a.to_string(), source: Source::Mine }).collect(),
            active: 0,
        });
        State::with_doc(doc)
    }

    #[test]
    fn stepping_cycles_and_wraps() {
        let mut s = state("the cat sat", "cat", &["cat", "dog", "owl"]);
        s.step_alt(1, 1);
        assert_eq!(s.edit.text, "the dog sat");
        s.step_alt(1, 1);
        assert_eq!(s.edit.text, "the owl sat");
        s.step_alt(1, 1);
        assert_eq!(s.edit.text, "the cat sat");
        s.step_alt(1, -1);
        assert_eq!(s.edit.text, "the owl sat");
        assert_eq!(s.doc.text, s.edit.text);
    }

    #[test]
    fn a_longer_alternative_moves_later_ranges() {
        let mut s = state("the cat sat", "cat", &["cat", "hippopotamus"]);
        s.doc.add_ghost(8..11);
        s.step_alt(1, 1);
        assert_eq!(&s.doc.text[s.doc.ghosts[0].range.clone()], "sat");
        assert_eq!(&s.doc.text[s.doc.targets[0].range.clone()], "hippopotamus");
    }

    #[test]
    fn the_caret_stays_with_its_text() {
        let mut s = state("the cat sat", "cat", &["cat", "hippopotamus"]);
        s.edit.set_selection(10, 10);
        s.step_alt(1, 1);
        assert_eq!(&s.edit.text[s.edit.focus..], "t");
    }

    #[test]
    fn the_caret_before_the_target_does_not_move() {
        let mut s = state("the cat sat", "cat", &["cat", "dog"]);
        s.edit.set_selection(2, 2);
        s.step_alt(1, 1);
        assert_eq!(s.edit.focus, 2);
    }

    #[test]
    fn one_undo_takes_back_the_swap_and_its_article() {
        let mut s = state("Take a pen now", "pen", &["pen", "eraser"]);
        s.step_alt(1, 1);
        assert_eq!(s.edit.text, "Take an eraser now");
        s.edit.undo();
        assert_eq!(s.edit.text, "Take a pen now");
    }

    #[test]
    fn the_article_fix_keeps_ranges_right() {
        let mut s = state("Take a pen now", "pen", &["pen", "eraser"]);
        s.step_alt(1, 1);
        assert_eq!(&s.doc.text[s.doc.targets[0].range.clone()], "eraser");
    }

    #[test]
    fn one_alternative_has_nothing_to_step_to() {
        let mut s = state("the cat", "cat", &["cat"]);
        assert!(!s.step_alt(1, 1));
    }

    #[test]
    fn polling_fades_the_new_text_in_and_then_stops() {
        let mut s = state("the cat", "cat", &["cat", "dog"]);
        s.step_alt(1, 1);
        let mut tx = rune::Tx::default();
        assert!(poll(&mut s, 10.0, &mut tx));
        assert_eq!(s.fade.as_ref().unwrap().started, Some(10.0));
        assert!(poll(&mut s, 10.08, &mut tx));
        let alpha = s.fade.as_ref().unwrap().alpha(s.fade_now);
        assert!((0.3..0.7).contains(&alpha), "{alpha}");
        assert!(poll(&mut s, 10.2, &mut tx));
        assert!(s.fade.is_none());
        assert!(!poll(&mut s, 10.3, &mut tx), "nothing left to draw");
    }

    #[test]
    fn the_fade_runs_from_invisible_to_full() {
        let fade = Fade { range: 0..1, started: Some(1.0) };
        assert_eq!(fade.alpha(1.0), 0.0);
        assert_eq!(fade.alpha(2.0), 1.0);
    }
}
