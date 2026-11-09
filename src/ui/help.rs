//! The keyboard icon and the list behind it, built from the shortcut table.

use rune::{El, Shape, Transition, canvas, col, row, text};

use crate::State;
use crate::shortcuts::{IN_PLACE, SHORTCUTS, VIM};
use crate::ui::theme::{self, THEME};

fn icon(on: bool) -> El {
    let c = if on { THEME.accent.color() } else { THEME.faint.color() };
    let key = |x: f32, y: f32, w: f32| vec![[x, y], [x + w, y]];
    let shape = Shape::new()
        .view_box(20.0, 14.0)
        .stroke(vec![[1.0, 1.0], [19.0, 1.0], [19.0, 13.0], [1.0, 13.0], [1.0, 1.0]], 1.0, c)
        .stroke(key(4.0, 5.0, 1.5), 1.4, c)
        .stroke(key(9.0, 5.0, 1.5), 1.4, c)
        .stroke(key(14.0, 5.0, 1.5), 1.4, c)
        .stroke(key(6.0, 9.5, 8.0), 1.2, c);
    canvas(shape).w(20.0).h(14.0)
}

fn line(keys: &str, does: &str) -> El {
    row().w_full().gap(12.0).child(text(keys.to_string()).font(theme::small()).color(THEME.accent.color()).w(112.0)).child(col().grow(1.0).min_w(0.0).child(text(does.to_string()).font(theme::small()).color(THEME.ink.color()).wrap()))
}

pub fn view(s: &State) -> Option<El> {
    if s.mode != crate::editor::Mode::Active {
        return None;
    }
    let button = col().padding(4.0).child(icon(s.help_open)).key("toggle-help").on_press_tx(|s: &mut State, _| s.help_open = !s.help_open);
    let mut column = col().items_end().gap(8.0).px(20.0).pt(14.0).child(button);
    if s.help_open {
        let card = SHORTCUTS.iter().fold(col().w(360.0).fill(THEME.panel.color()).radius(10.0).padding(16.0).gap(6.0).key("help"), |c, k| c.child(line(k.keys, k.does)));
        let card = IN_PLACE.iter().fold(card, |c, (keys, does)| c.child(line(keys, does)));
        let card = if s.vim.on { VIM.iter().fold(card, |c, (keys, does)| c.child(line(keys, does))) } else { card };
        column = column.child(card.transition(Transition::opacity()));
    }
    Some(column.pin(None, Some(0.0), Some(0.0), None))
}
