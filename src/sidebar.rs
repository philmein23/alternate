//! The alternatives panel: rows you write and browse, shown in the page as you type.

use rune::{Key, KeyEvent, TextEdit, Tx};

use crate::editor::Mode;
use crate::model::{Alt, Level, Source, Target, TargetId};
use crate::segment;
use crate::State;

pub struct Panel {
    pub id: TargetId,
    pub level: Level,
    /// One per alternative, plus an empty one to write in.
    pub rows: Vec<TextEdit>,
    pub focus: usize,
    /// Where in the text the panel was opened, for switching levels.
    pub pos: usize,
    /// The document caret when the panel last looked, to notice a click in the page.
    pub caret: usize,
}

fn is_blank(s: &str) -> bool {
    s.trim().is_empty()
}

impl State {
    /// The target with exactly this range, making one if there is none.
    fn target_for(&mut self, level: Level, range: std::ops::Range<usize>) -> TargetId {
        if let Some(t) = self.doc.targets.iter().find(|t| t.range == range) {
            return t.id;
        }
        let text = self.doc.text[range.clone()].to_string();
        let id = self.doc.next_id();
        self.doc.targets.push(Target { id, range, level, original: text.clone(), alts: vec![Alt { text, source: Source::Mine }], active: 0 });
        id
    }

    /// Opens the panel on what the caret or selection means. False when it means nothing.
    pub fn open_panel(&mut self) -> bool {
        let (a, b) = self.edit.range();
        let found = if a != b {
            segment::target_for_selection(&self.doc.text, a..b)
        } else {
            let word = segment::word_at(&self.doc.text, a).map(|r| (Level::Word, r));
            word.or_else(|| segment::sentence_at(&self.doc.text, a).map(|r| (Level::Sentence, r)))
        };
        let Some((level, range)) = found else { return false };
        let pos = range.start;
        self.open_on(level, range, pos);
        true
    }

    /// Opens the panel on the word at `pos`, for the page's right-click menu.
    pub fn open_panel_at(&mut self, pos: usize) -> bool {
        let Some(range) = segment::word_at(&self.doc.text, pos) else { return false };
        self.open_on(Level::Word, range, pos);
        true
    }

    fn open_on(&mut self, level: Level, range: std::ops::Range<usize>, pos: usize) {
        self.close_panel();
        self.mode = Mode::Active;
        let id = self.target_for(level, range);
        let t = self.doc.targets.iter().find(|t| t.id == id).expect("just found or made");
        let mut rows: Vec<TextEdit> = t.alts.iter().map(|a| TextEdit::new(a.text.clone())).collect();
        rows.push(TextEdit::new(""));
        let focus = rows.len() - 1;
        self.edit.focused = false;
        self.hover = None;
        self.panel = Some(Panel { id, level: t.level, rows, focus, pos, caret: self.edit.focus });
        self.focus_row(focus);
        self.touch();
    }

    /// Leaves the panel. Blank rows go, and so does a target left with nothing to swap to.
    pub fn close_panel(&mut self) {
        let Some(panel) = self.panel.take() else { return };
        if let Some(i) = self.doc.targets.iter().position(|t| t.id == panel.id) {
            let t = &mut self.doc.targets[i];
            let current = t.active;
            let mut kept = 0;
            let mut active = 0;
            let mut index = 0;
            t.alts.retain(|a| {
                let keep = !is_blank(&a.text);
                if index == current {
                    active = kept;
                }
                index += 1;
                kept += usize::from(keep);
                keep
            });
            t.active = active.min(t.alts.len().saturating_sub(1));
            if t.alts.len() < 2 {
                self.doc.targets.remove(i);
            }
        }
        self.edit.focused = true;
        self.touch();
    }

    pub fn focus_row(&mut self, i: usize) {
        let Some(panel) = &mut self.panel else { return };
        let i = i.min(panel.rows.len() - 1);
        panel.focus = i;
        for (j, row) in panel.rows.iter_mut().enumerate() {
            row.focused = i == j;
        }
        let end = panel.rows[i].text.len();
        panel.rows[i].set_selection(end, end);
        let id = panel.id;
        self.preview_row(id, i);
    }

    /// Puts a row in the page, unless it's blank or already showing.
    fn preview_row(&mut self, id: TargetId, i: usize) {
        let Some(t) = self.doc.targets.iter().find(|t| t.id == id) else { return };
        if t.alts.get(i).is_none_or(|a| is_blank(&a.text)) || (t.active == i && self.doc.text[t.range.clone()] == t.alts[i].text) {
            return;
        }
        self.show_alt(id, i);
        self.fade = None;
        if let Some(panel) = &mut self.panel {
            panel.caret = self.edit.focus;
        }
    }

    /// Writes row `i` through to the alternative and the page.
    pub fn row_changed(&mut self, i: usize) {
        let Some(panel) = &mut self.panel else { return };
        let id = panel.id;
        let Some(text) = panel.rows.get(i).map(|r| r.text.clone()) else { return };
        let Some(t) = self.doc.targets.iter_mut().find(|t| t.id == id) else { return };
        if i == t.alts.len() {
            if text.is_empty() {
                return;
            }
            t.alts.push(Alt { text: text.clone(), source: Source::Mine });
            panel.rows.push(TextEdit::new(""));
        } else if let Some(alt) = t.alts.get_mut(i) {
            alt.text = text.clone();
        }
        if t.active == i {
            // Rewriting the row on the page: force it through by treating it as new text.
            if is_blank(&text) {
                let other = t.alts.iter().position(|a| !is_blank(&a.text));
                if let Some(other) = other {
                    self.preview_row(id, other);
                }
                self.touch();
                return;
            }
        }
        self.preview_row(id, i);
        self.touch();
    }

    /// Switches the panel to the word, sentence or paragraph at the same place.
    pub fn set_level(&mut self, level: Level) {
        let Some(pos) = self.panel.as_ref().map(|p| p.pos) else { return };
        let Some(range) = segment::range_at(&self.doc.text, pos, level) else { return };
        self.open_on(level, range, pos);
    }

    /// Notices a click in the page or a row while the panel is open.
    pub fn track_panel(&mut self) -> bool {
        let Some(panel) = &self.panel else { return false };
        if self.edit.focused {
            let (level, pos) = (panel.level, self.edit.focus);
            self.edit.focused = false;
            if pos != panel.caret
                && let Some(range) = segment::range_at(&self.doc.text, pos, level)
            {
                self.open_on(level, range, pos);
                return true;
            }
            let focus = panel.focus;
            self.focus_row(focus);
            return true;
        }
        if let Some(i) = panel.rows.iter().position(|r| r.focused)
            && i != panel.focus
        {
            self.focus_row(i);
            return true;
        }
        false
    }
}

/// Keys inside a row.
pub fn row_key(s: &mut State, i: usize, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if ev.mods.cmd {
        return crate::editor::handle_key(s, ev, tx);
    }
    let Some(panel) = &s.panel else { return false };
    let last = panel.rows.len() - 1;
    match ev.key {
        Key::Up => s.focus_row(i.saturating_sub(1)),
        Key::Down => s.focus_row((i + 1).min(last)),
        Key::Enter if !ev.mods.shift => s.focus_row(last),
        Key::Escape => s.close_panel(),
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Doc;

    fn opened() -> State {
        let mut s = State::with_doc(Doc::new("The cat sat. Then it left."));
        s.edit.set_selection(5, 5);
        assert!(s.open_panel());
        s
    }

    fn type_in(s: &mut State, i: usize, text: &str) {
        let panel = s.panel.as_mut().unwrap();
        panel.rows[i].text = text.to_string();
        s.row_changed(i);
    }

    #[test]
    fn opening_makes_a_target_with_an_empty_row_to_write_in() {
        let s = opened();
        let panel = s.panel.as_ref().unwrap();
        assert_eq!(panel.rows.len(), 2);
        assert_eq!(panel.rows[0].text, "cat");
        assert_eq!(panel.focus, 1);
        assert_eq!(s.doc.targets[0].range, 4..7);
    }

    #[test]
    fn typing_in_the_empty_row_adds_a_row_and_previews_it() {
        let mut s = opened();
        type_in(&mut s, 1, "?");
        assert_eq!(s.doc.text, "The ? sat. Then it left.");
        assert_eq!(s.panel.as_ref().unwrap().rows.len(), 3);
        type_in(&mut s, 1, "dog");
        assert_eq!(s.doc.text, "The dog sat. Then it left.");
        assert_eq!(s.edit.text, s.doc.text);
    }

    #[test]
    fn leaving_keeps_the_choice() {
        let mut s = opened();
        type_in(&mut s, 1, "dog");
        s.close_panel();
        assert_eq!(s.doc.text, "The dog sat. Then it left.");
        assert_eq!(s.doc.targets[0].alts.len(), 2);
        assert_eq!(s.doc.targets[0].active, 1);
        assert!(s.panel.is_none() && s.edit.focused);
    }

    #[test]
    fn closing_with_only_the_original_removes_the_mark() {
        let mut s = opened();
        s.close_panel();
        assert!(s.doc.targets.is_empty());
        assert_eq!(s.doc.text, "The cat sat. Then it left.");
    }

    #[test]
    fn a_whitespace_row_does_not_reach_the_page_and_goes_on_close() {
        let mut s = opened();
        type_in(&mut s, 1, "dog");
        s.focus_row(2);
        type_in(&mut s, 2, "   ");
        assert_eq!(s.doc.text, "The dog sat. Then it left.");
        s.close_panel();
        assert_eq!(s.doc.targets[0].alts.len(), 2);
    }

    #[test]
    fn clearing_the_row_on_the_page_falls_back_to_another() {
        let mut s = opened();
        type_in(&mut s, 1, "dog");
        type_in(&mut s, 1, "");
        assert_eq!(s.doc.text, "The cat sat. Then it left.");
        s.close_panel();
        assert!(s.doc.targets.is_empty(), "nothing left to swap to");
    }

    #[test]
    fn moving_between_rows_shows_each_in_the_page() {
        let mut s = opened();
        type_in(&mut s, 1, "dog");
        s.focus_row(0);
        assert_eq!(s.doc.text, "The cat sat. Then it left.");
        s.focus_row(1);
        assert_eq!(s.doc.text, "The dog sat. Then it left.");
    }

    #[test]
    fn switching_level_retargets_at_the_same_place() {
        let mut s = opened();
        s.set_level(Level::Sentence);
        let panel = s.panel.as_ref().unwrap();
        assert_eq!(panel.level, Level::Sentence);
        assert_eq!(panel.rows[0].text, "The cat sat.");
        assert_eq!(s.doc.targets.len(), 1, "the unused word target went");
    }

    #[test]
    fn a_click_in_the_page_retargets_at_the_same_level() {
        let mut s = opened();
        s.edit.set_selection(19, 19);
        s.edit.focused = true;
        assert!(s.track_panel());
        assert_eq!(s.panel.as_ref().unwrap().rows[0].text, "it");
        assert!(!s.edit.focused);
    }

    #[test]
    fn a_click_at_the_same_place_stays_put() {
        let mut s = opened();
        let id = s.panel.as_ref().unwrap().id;
        s.edit.focused = true;
        s.track_panel();
        assert_eq!(s.panel.as_ref().unwrap().id, id);
    }

    #[test]
    fn nothing_under_the_caret_opens_nothing() {
        let mut s = State::with_doc(Doc::new(""));
        assert!(!s.open_panel());
    }

    #[test]
    fn a_long_alternative_keeps_the_page_in_step() {
        let mut s = opened();
        let long = "an enormously long replacement for a single small word ".repeat(20);
        type_in(&mut s, 1, &long);
        assert_eq!(&s.doc.text[s.doc.targets[0].range.clone()], long);
        assert_eq!(s.edit.text, s.doc.text);
    }
}
