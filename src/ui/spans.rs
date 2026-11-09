//! How parts of the page are styled: faded text, the swap fade, and the hover tint.

use rune::{Span, TextHighlight};

use crate::State;
use crate::editor::Mode;
use crate::ui::theme::THEME;

pub fn spans(s: &State) -> Vec<Span> {
    let mut out: Vec<Span> = s.doc.ghosts.iter().map(|g| Span::new(g.range.start, g.range.end).color(THEME.ink.alpha(THEME.ghost))).collect();
    if let Some(fade) = &s.fade {
        out.push(Span::new(fade.range.start, fade.range.end).color(THEME.ink.alpha(fade.alpha(s.fade_now))));
    }
    out
}

pub fn highlights(s: &State) -> Vec<TextHighlight> {
    let hovered = s.hover.filter(|_| s.mode == Mode::Active).and_then(|h| s.doc.targets.iter().find(|t| t.id == h.id));
    hovered.map(|t| TextHighlight::new(t.range.clone(), THEME.accent.alpha(0.16))).into_iter().collect()
}
