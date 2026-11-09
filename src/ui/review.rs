//! The review bar, and clickable runs over the text being cut.

use rune::{Cx, El, Span, TextHighlight, col, el, row, text};

use crate::State;
use crate::lab::trim::Kind;
use crate::ui::theme::{self, THEME};

pub fn spans(s: &State) -> Vec<Span> {
    let Some(r) = s.review.as_ref().filter(|r| !r.is_fix()) else { return Vec::new() };
    r.cuts.iter().filter(|c| !c.kept).map(|c| Span::new(c.range.start, c.range.end).color(THEME.ink.alpha(THEME.ghost))).collect()
}

pub fn highlights(s: &State) -> Vec<TextHighlight> {
    let Some(r) = &s.review else { return Vec::new() };
    let mut out = Vec::new();
    if r.is_fix() {
        out.extend(r.cuts.iter().filter(|c| !c.kept).map(|c| TextHighlight::new(c.range.clone(), THEME.marker.alpha(0.26))));
    }
    out.extend(r.walk.and_then(|i| r.cuts.get(i)).map(|c| TextHighlight::new(c.range.clone(), THEME.accent.alpha(0.3))));
    out
}

/// One invisible pressable box per line of each run, placed over the field.
pub fn overlays(s: &State, cx: &mut Cx<State>) -> Vec<El> {
    let Some(r) = &s.review else { return Vec::new() };
    let ranges: Vec<_> = r.cuts.iter().map(|c| c.range.clone()).collect();
    let boxes = cx.text_rects("doc", &ranges);
    let Some([fx, fy, ..]) = cx.layout("doc") else { return Vec::new() };
    let mut out = Vec::new();
    for (i, lines) in boxes.iter().enumerate() {
        for (n, b) in lines.iter().enumerate() {
            let key = if n == 0 { format!("cut-{i}") } else { format!("cut-{i}-{n}") };
            out.push(el().w(b[2]).h(b[3]).pin(Some(b[0] - fx), Some(b[1] - fy), None, None).key(key).on_press_tx(move |s: &mut State, _| s.toggle_cut(i)));
        }
    }
    out
}

fn action(label: &str, key: &'static str, press: fn(&mut State, &mut rune::Tx), strong: bool) -> El {
    text(label.to_string()).font(theme::small()).color(if strong { THEME.accent.color() } else { THEME.ink.color() }).key(key).on_press_tx(move |s: &mut State, tx| press(s, tx))
}

fn walking_line(s: &State, i: usize) -> String {
    let r = s.review.as_ref().expect("only called during a review");
    let cut = &r.cuts[i];
    let quoted = s.doc.text.get(cut.range.clone()).unwrap_or("");
    let total = r.cuts.len();
    match &cut.fix {
        Some(fix) => format!("{} of {total}: \u{201c}{quoted}\u{201d} \u{2192} \u{201c}{fix}\u{201d}", i + 1),
        None => format!("Cut {} of {total}", i + 1),
    }
}

/// The bar above the LAB button while a review is open.
pub fn bar(s: &State) -> Option<El> {
    let r = s.review.as_ref()?;
    let card = col().fill(THEME.panel.color()).radius(10.0).px(18.0).py(12.0).gap(6.0).items_center().key("review-bar");
    if let Some(i) = r.walk {
        let actions = row().gap(20.0).child(action(if r.is_fix() { "Leave it" } else { "Keep" }, "walk-keep", |s, tx| s.walk_step(true, tx), false)).child(action(if r.is_fix() { "Fix it" } else { "Cut" }, "walk-cut", |s, tx| s.walk_step(false, tx), true));
        return Some(card.child(text(walking_line(s, i)).font(theme::small()).color(THEME.ink.color()).wrap()).child(actions));
    }
    let (title, line, apply) = match r.kind {
        Kind::Trim(level) => (format!("{} \u{b7} {}", level.label(), r.summary()), "Faded words would go. Click one to keep it.", "Make the cuts"),
        Kind::Fixes => (format!("{} proposed fixes", r.cuts.iter().filter(|c| !c.kept).count()), "Tinted text would change. Click one to leave it.", "Apply the fixes"),
    };
    let actions = row().gap(20.0).child(action(apply, "review-apply", |s, _| s.make_cuts(), true)).child(action("Walk through", "review-walk", |s, tx| s.walk_start(tx), false)).child(action("Done", "review-done", |s, _| s.close_review(), false));
    Some(card.child(text(title).font(theme::small()).color(THEME.ink.color()).key("review-summary")).child(text(line.to_string()).font(theme::small()).color(THEME.faint.color())).child(actions))
}
