//! The right-click menu on the page.

use std::ops::Range;

use rune::{El, PointerPress, Tx, el};
use rune_kit::{MenuItem, context_menu};

use crate::State;
use crate::segment;

#[derive(Clone, Debug)]
pub struct MenuContext {
    pub range: Range<usize>,
    /// The word the labels name. `None` means they say "selection".
    pub word: Option<String>,
    pub in_ghost: bool,
    pub point: PointerPress,
}

impl State {
    /// Works out what a right-click at text position `pos` is about. The pointer's word wins
    /// unless the pointer is on the selection; with no position, the selection or caret word.
    pub fn menu_context(&self, pos: Option<usize>, point: PointerPress) -> Option<MenuContext> {
        let (a, b) = self.edit.range();
        let on_selection = a != b && pos.is_none_or(|p| (a..=b).contains(&p));
        let ghost = pos.and_then(|p| self.ghost_at(p));
        let word_range = if on_selection { None } else { pos.and_then(|p| segment::word_at(&self.doc.text, p)).or_else(|| ghost.clone()) };
        let (range, word) = match word_range {
            Some(r) => {
                let word = self.doc.text[r.clone()].to_string();
                (r, Some(word))
            }
            None if a != b => (a..b, None),
            None => {
                let r = segment::word_at(&self.doc.text, a)?;
                let word = self.doc.text[r.clone()].to_string();
                (r, Some(word))
            }
        };
        let in_ghost = ghost.is_some() || self.ghost_at(range.start).is_some();
        Some(MenuContext { range, word, in_ghost, point })
    }

    pub fn open_menu(&mut self, pos: Option<usize>, point: PointerPress) {
        self.menu = self.menu_context(pos, point);
    }
}

/// Asks what's under the pointer, then opens the menu there.
pub fn request(point: PointerPress, tx: &mut Tx) {
    tx.element_at_in("doc", point.x, point.y, move |s: &mut State, picked, _| s.open_menu(picked.and_then(|p| p.text_index()), point));
}

pub fn subject(ctx: &MenuContext) -> String {
    match &ctx.word {
        Some(w) => format!("\u{201c}{w}\u{201d}"),
        None => "selection".to_string(),
    }
}

/// What the menu's Alternatives item does.
pub fn alternatives(s: &mut State, ctx: &MenuContext) {
    match ctx.word {
        Some(_) => s.open_panel_at(ctx.range.start),
        None => s.open_panel_on(ctx.range.clone()),
    };
}

/// Mounts the menu into the window's top layer.
pub fn mount(s: &State, layer: &mut Vec<El>) {
    let Some(ctx) = &s.menu else { return };
    let (a, ghost, stash) = (ctx.clone(), ctx.range.clone(), ctx.range.clone());
    let trigger = el().w(0.0).h(0.0);
    let keep = context_menu("page", trigger, Some(ctx.point))
        .standard()
        .min_width(260.0)
        .on_close(|s: &mut State| s.menu = None)
        .item(MenuItem::new(format!("Alternatives for {}", subject(ctx))).shortcut("\u{2303}\u{21e7}A").on_select(move |s: &mut State| {
            s.menu = None;
            alternatives(s, &a);
        }))
        .item(MenuItem::new(if ctx.in_ghost { "Revive it" } else { "Ghost it" }).shortcut("\u{2303}/").on_select(move |s: &mut State| {
            s.menu = None;
            s.toggle_ghost(ghost.clone());
        }))
        .item(MenuItem::new("Stash this in Overflow").shortcut("\u{2303}\u{21e7}X").on_select(move |s: &mut State| {
            s.menu = None;
            s.stash(stash.clone());
        }))
        .build(&s.kit, layer);
    layer.push(keep);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Doc;

    const AT: PointerPress = PointerPress { x: 0.0, y: 0.0 };

    fn state() -> State {
        State::with_doc(Doc::new("one two three"))
    }

    #[test]
    fn the_word_under_the_pointer_wins_over_the_caret() {
        let mut s = state();
        s.edit.set_selection(0, 0);
        let c = s.menu_context(Some(9), AT).unwrap();
        assert_eq!(c.word.as_deref(), Some("three"));
        assert_eq!(c.range, 8..13);
    }

    #[test]
    fn a_click_on_the_selection_means_the_selection() {
        let mut s = state();
        s.edit.set_selection(0, 7);
        let c = s.menu_context(Some(2), AT).unwrap();
        assert_eq!(c.word, None);
        assert_eq!(c.range, 0..7);
        assert_eq!(subject(&c), "selection");
    }

    #[test]
    fn a_click_off_the_selection_means_the_word_there() {
        let mut s = state();
        s.edit.set_selection(0, 3);
        assert_eq!(s.menu_context(Some(9), AT).unwrap().word.as_deref(), Some("three"));
    }

    #[test]
    fn without_geometry_the_caret_word_is_used() {
        let mut s = state();
        s.edit.set_selection(5, 5);
        assert_eq!(s.menu_context(None, AT).unwrap().word.as_deref(), Some("two"));
    }

    #[test]
    fn the_menu_offers_revive_inside_a_ghost() {
        let mut s = state();
        s.toggle_ghost(4..7);
        assert!(s.menu_context(Some(5), AT).unwrap().in_ghost);
        assert!(!s.menu_context(Some(9), AT).unwrap().in_ghost);
    }

    #[test]
    fn opening_leaves_the_selection_alone() {
        let mut s = state();
        s.edit.set_selection(0, 3);
        s.open_menu(Some(9), AT);
        assert_eq!(s.edit.range(), (0, 3));
    }
}
