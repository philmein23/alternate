//! The right panel.

use rune::{Drag, Edge, El, Phase, Spring, Transition, Tx, col, row, text, text_input};

use crate::overflow;
use crate::State;
use crate::ui::theme::{self, THEME};

const WIDTH: f32 = 280.0;

/// Dropping the grip on the page puts the Overflow text where it landed.
fn dropped(_: &mut State, d: &mut Drag, tx: &mut Tx) {
    if d.phase != Phase::Up {
        return;
    }
    let [x, y] = d.window_point();
    tx.element_at_in("doc", x, y, |s: &mut State, picked, _| {
        if let Some(at) = picked.and_then(|p| p.text_index()) {
            s.use_overflow(at);
        }
    });
}

pub fn view(s: &State) -> Option<El> {
    if !s.overflow_open {
        return None;
    }
    let field = text_input(&s.overflow_edit, theme::body())
        .on_edit(|s: &mut State| &mut s.overflow_edit)
        .on_input_change(|s: &mut State, _| s.overflow_changed())
        .on_input_key(overflow::field_key)
        .on_submit(|s: &mut State, _| {
            s.overflow_edit.insert("\n");
            s.overflow_changed();
        })
        .placeholder("Set things aside here.", THEME.faint.color())
        .selection_color(THEME.accent.alpha(0.3))
        .color(THEME.ink.color())
        .key("overflow-field")
        .wrap_anywhere()
        .w_full();
    let grip = text("\u{22ee}\u{22ee}".to_string()).font(theme::small()).color(THEME.faint.color()).key("overflow-grip").on_drag(dropped);
    let title = row().w_full().items_center().justify_between().child(text("Overflow".to_string()).font(theme::hand()).color(THEME.accent.color())).child(grip);
    let body = col().w_full().grow(1.0).min_h(0.0).scroll(rune::ScrollSpec { stick_bottom: false }).child(field).key("overflow-scroll");
    let footer = text("ctrl+return or drag into the page to use".to_string()).font(theme::small()).color(THEME.faint.color()).wrap();
    Some(col().w(WIDTH).h_full().fill(THEME.panel.color()).pt(52.0).px(20.0).pb(16.0).gap(16.0).child(title).child(body).child(footer).key("overflow").transition(Transition::move_edge(Edge::Trailing)).motion(Spring::new(0.28, 0.0)))
}
