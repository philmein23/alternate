//! The left panel.

use rune::{Edge, El, Shape, Spring, TextStyle, Transition, canvas, col, row, text, text_input};

use crate::model::{Level, Source};
use crate::sidebar::{self, Panel};
use crate::State;
use crate::ui::theme::{self, THEME};

const WIDTH: f32 = 280.0;

fn tab(panel: &Panel, name: &str, key: &'static str, level: Level) -> El {
    let on = panel.level == level;
    text(name.to_string()).font(theme::hand()).color(if on { THEME.accent.color() } else { THEME.faint.color() }).key(key).on_press_tx(move |s: &mut State, _| s.set_level(level))
}

/// A small square-headed robot, for rows the AI wrote.
fn robot() -> El {
    let c = THEME.accent.color();
    let shape = Shape::new()
        .view_box(12.0, 12.0)
        .stroke(vec![[2.0, 4.0], [10.0, 4.0], [10.0, 11.0], [2.0, 11.0], [2.0, 4.0]], 1.0, c)
        .stroke(vec![[6.0, 1.0], [6.0, 4.0]], 1.0, c)
        .stroke(vec![[4.2, 7.0], [4.8, 7.0]], 1.4, c)
        .stroke(vec![[7.2, 7.0], [7.8, 7.0]], 1.4, c)
        .stroke(vec![[4.5, 9.3], [7.5, 9.3]], 0.8, c);
    canvas(shape).w(12.0).h(12.0).mt(7.0)
}

fn bullet(on: bool) -> El {
    text("•".to_string()).font(row_style()).color(if on { THEME.accent.color() } else { THEME.faint.color() }).w(12.0)
}

fn row_style() -> TextStyle {
    TextStyle::new(14.0, 400.0).lh(24.0).family(rune::Family::Mono)
}

/// The robot, hopping while it waits.
fn dancing_robot(s: &State) -> El {
    let hop = if s.still.get() { 0.0 } else { (s.dance_now as f32 * 10.0).sin() * 2.5 };
    robot().offset(0.0, hop)
}

fn alt_row(s: &State, panel: &Panel, i: usize) -> El {
    let target = s.doc.targets.iter().find(|t| t.id == panel.id);
    let alt = target.and_then(|t| t.alts.get(i));
    let on = target.is_some_and(|t| t.active == i && alt.is_some());
    let waiting = (s.pending.is_some() || s.demo_asking) && i == panel.rows.len() - 1;
    let mark = match alt {
        _ if waiting => dancing_robot(s),
        Some(a) if a.source == Source::Ai => robot(),
        _ => bullet(on),
    };
    let field = text_input(&panel.rows[i], row_style())
        .on_edit(rune::edit_access(move |s: &mut State| match s.panel.as_mut().and_then(|p| p.rows.get_mut(i)) {
            Some(row) => row,
            None => &mut s.spare_edit,
        }))
        .on_input_change(move |s: &mut State, _| s.row_changed(i))
        .on_input_key(move |s: &mut State, ev, tx| sidebar::row_key(s, i, ev, tx))
        .placeholder("Add another", THEME.faint.color())
        .selection_color(THEME.accent.alpha(0.3))
        .color(if on { THEME.ink.color() } else { THEME.faint.color() })
        .key(format!("row-{i}"))
        .wrap_anywhere()
        .w_full();
    row().w_full().gap(8.0).child(mark).child(col().grow(1.0).min_w(0.0).child(field))
}

pub fn view(s: &State) -> Option<El> {
    let panel = s.panel.as_ref()?;
    let tabs = row().gap(14.0).child(tab(panel, "Word", "tab-word", Level::Word)).child(tab(panel, "Sentence", "tab-sentence", Level::Sentence)).child(tab(panel, "Paragraph", "tab-paragraph", Level::Paragraph));
    let rows = (0..panel.rows.len()).fold(col().w_full().gap(10.0), |c, i| c.child(alt_row(s, panel, i)));
    let list = col().w_full().grow(1.0).min_h(0.0).scroll(rune::ScrollSpec { stick_bottom: false }).child(rows).key("rows");
    let footer = text("return adds · ?? asks claude · esc closes".to_string()).font(theme::small()).color(THEME.faint.color()).wrap();
    let mut card = col().w(WIDTH).h_full().fill(THEME.panel.color()).pt(52.0).px(20.0).pb(16.0).gap(16.0).child(tabs).child(list);
    if let Some(message) = &panel.message {
        card = card.child(text(message.clone()).font(theme::small()).color(THEME.marker.color()).wrap().key("panel-message"));
    }
    Some(card.child(footer).key("panel").transition(Transition::move_edge(Edge::Leading)).motion(Spring::new(0.28, 0.0)))
}
