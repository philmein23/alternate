//! A `--scene palette` render: the same writing in each candidate palette and every page state.

use rune::{El, Family, Shape, Span, TextStyle, canvas, col, row, text};

use crate::State;
use crate::ui::theme::{self, THEME, THEMES, Theme};

const GHOST_LINE: &str = "Some mornings the words arrive in the wrong order.";
const SENTENCE: &str = "She wrote the first line four times and kept none of them.";

fn wave(x: f32, y: f32, w: f32) -> Vec<[f32; 2]> {
    let steps = (w / 3.0).ceil().max(2.0) as usize;
    (0..=steps).map(|i| [x + i as f32 / steps as f32 * w, y + (i as f32 * 1.05).sin() * 1.1]).collect()
}

fn label(t: &Theme, s: &str) -> El {
    text(s.to_string()).font(theme::small()).color(t.faint.color())
}

fn state(t: &Theme, name: &str, body: El) -> El {
    col().w_full().gap(6.0).child(label(t, name)).child(body)
}

fn panel(t: Theme) -> El {
    let plain = text(SENTENCE.to_string()).font(theme::body()).color(t.ink.color()).w_full().wrap();
    let ghost = text(GHOST_LINE.to_string()).font(theme::body()).color(t.ink.color()).w_full().wrap().spans(vec![Span::new(0, GHOST_LINE.len()).color(t.ink.alpha(t.ghost))]);
    let word = SENTENCE.find("first").unwrap();
    let dimmed = text(SENTENCE.to_string())
        .font(theme::body())
        .color(t.ink.alpha(t.dimmed)).wrap()
        .w_full()
        .spans(vec![Span::new(word, word + 5).color(t.ink.color())]);
    let selected = text(SENTENCE.to_string()).font(theme::body()).color(t.ink.color()).w_full().wrap().text_highlights([rune::TextHighlight::new(word..word + 5, t.accent.alpha(0.3))]);
    let marks = canvas(Shape::new().stroke(wave(0.0, 6.0, 180.0), 1.2, t.accent.color()).stroke(wave(200.0, 6.0, 90.0), 1.2, t.marker.color())).w_full().h(14.0);
    let title = text(t.name.to_string()).font(theme::hand()).color(t.accent.color());
    let tag = col().w_full().h(40.0).fill(t.panel.color()).center().child(label(&t, "panel surface"));
    col()
        .w(400.0)
        .h_full()
        .fill(t.page.color())
        .padding(theme::GUTTER)
        .gap(20.0)
        .child(title)
        .child(state(&t, "plain", plain))
        .child(state(&t, "marks", marks))
        .child(state(&t, "selected", selected))
        .child(state(&t, "dimmed, target lit", dimmed))
        .child(state(&t, "ghosted", ghost))
        .child(tag)
}

pub fn view(_s: &State) -> El {
    THEMES.into_iter().fold(row().w_full().h_full(), |r, t| r.child(panel(t)))
}

/// The app icon, drawn at 512 points in the middle of the window for `scripts` to crop.
pub fn icon() -> El {
    let accent = THEME.accent.color();
    let wave: Vec<[f32; 2]> = (0..=60).map(|i| { let x = i as f32 / 60.0 * 280.0; [x, 12.0 + (x * 0.075).sin() * 7.0] }).collect();
    let line = canvas(Shape::new().view_box(280.0, 24.0).stroke(wave, 7.0, accent)).w(280.0).h(24.0);
    let letter = text("A".to_string()).font(TextStyle::new(360.0, 400.0).lh(330.0).family(Family::Named(crate::fonts::HAND))).color(THEME.ink.color());
    col().w_full().h_full().fill(THEME.page.color()).items_center().justify_center().child(col().w(512.0).h(512.0).fill(THEME.panel.color()).items_center().justify_center().child(letter).child(line))
}
