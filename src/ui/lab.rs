//! The LAB button, its popover, and the tinted marks a pass leaves.

use rune::{Color, Cx, El, TextHighlight, Transition, col, row, text};

use crate::State;
use crate::lab::trim::{Kind, LEVELS, Level};
use crate::lab::{PASSES, Pass};
use crate::ui::theme::{self, THEME};

fn tint(pass: Pass) -> Color {
    match pass {
        Pass::Typos | Pass::Weakest => THEME.marker.alpha(0.24),
        Pass::Long | Pass::Convoluted => THEME.accent.alpha(0.22),
        Pass::Tone | Pass::Hedges => THEME.faint.alpha(0.3),
    }
}

/// The marks that still fit the text.
pub fn highlights(s: &State) -> Vec<TextHighlight> {
    let fits = |r: &std::ops::Range<usize>| r.end <= s.doc.text.len() && s.doc.text.is_char_boundary(r.start) && s.doc.text.is_char_boundary(r.end);
    s.doc.marks.iter().filter(|m| fits(&m.range)).map(|m| TextHighlight::new(m.range.clone(), tint(m.pass))).collect()
}

/// Keeps each mark's boxes for pointer tests, in the field's own points.
pub fn read_rects(s: &State, cx: &mut Cx<State>) {
    let mut hit = s.lab_hit.borrow_mut();
    hit.clear();
    if s.doc.marks.is_empty() {
        return;
    }
    let ranges: Vec<_> = s.doc.marks.iter().map(|m| m.range.clone()).collect();
    let boxes = cx.text_rects("doc", &ranges);
    let Some([fx, fy, ..]) = cx.layout("doc") else { return };
    hit.extend(boxes.into_iter().map(|lines| lines.iter().map(|b| [b[0] - fx, b[1] - fy, b[2], b[3]]).collect()));
}

fn pass_row(s: &State, pass: Pass) -> El {
    let running = s.lab.run.as_ref().is_some_and(|r| r.pass == pass);
    let color = if running { THEME.accent.color() } else { THEME.ink.color() };
    let mut item = col().w_full().gap(2.0).child(text(pass.label().to_string()).font(theme::small()).color(color));
    if s.lab.guide {
        item = item.child(text(pass.guide().to_string()).font(theme::small()).color(THEME.faint.color()).wrap());
    }
    item.key(pass.key()).on_press_tx(move |s: &mut State, _| s.start_pass(pass))
}

fn chip(s: &State, level: Level) -> El {
    let on = s.review.as_ref().is_some_and(|r| r.kind == Kind::Trim(level));
    text(level.label().to_string()).font(theme::small()).color(if on { THEME.accent.color() } else { THEME.ink.color() }).key(level.key()).on_press_tx(move |s: &mut State, _| s.start_trim(level))
}

fn chips(s: &State) -> El {
    let original = text("Original".to_string()).font(theme::small()).color(if s.review.is_none() { THEME.accent.color() } else { THEME.ink.color() }).key("chip-original").on_press_tx(|s: &mut State, _| s.show_original());
    let first = row().w_full().justify_between().child(original).child(chip(s, LEVELS[0]));
    let second = row().w_full().justify_between().child(chip(s, LEVELS[1])).child(chip(s, LEVELS[2]));
    col().w_full().gap(6.0).pt(6.0).child(first).child(second).child(chip(s, LEVELS[3]))
}

fn popover(s: &State) -> El {
    let guide = text(if s.lab.guide { "Hide the guide" } else { "What these do" }.to_string()).font(theme::small()).color(THEME.faint.color()).key("lab-guide").on_press_tx(|s: &mut State, _| s.lab.guide = !s.lab.guide);
    let mut card = col().w(320.0).fill(THEME.panel.color()).radius(10.0).padding(16.0).gap(10.0).key("lab-popover").child(guide);
    for pass in PASSES {
        card = card.child(pass_row(s, pass));
    }
    card = card.child(chips(s));
    if !s.doc.marks.is_empty() {
        card = card.child(text("Clear marks".to_string()).font(theme::small()).color(THEME.accent.color()).key("lab-clear").on_press_tx(|s: &mut State, _| s.clear_marks()));
    }
    if s.lab.run.is_some() || s.lab.trim.is_some() {
        card = card.child(text("Reading\u{2026}".to_string()).font(theme::small()).color(THEME.faint.color()).key("lab-running"));
    }
    if let Some(message) = &s.lab.message {
        card = card.child(text(message.clone()).font(theme::small()).color(THEME.marker.color()).wrap().key("lab-message"));
    }
    card
}

/// The button, the popover above it, and the reason for the mark under the pointer.
pub fn dock(s: &State) -> Option<El> {
    if s.mode != crate::editor::Mode::Active {
        return None;
    }
    let button = text("LAB".to_string()).font(theme::small()).color(if s.lab.open { THEME.accent.color() } else { THEME.faint.color() });
    let button = row().px(14.0).py(4.0).radius(6.0).border(1.0, THEME.faint.alpha(0.6)).border_dash(4.0, 3.0).child(button).key("toggle-lab").on_press_tx(|s: &mut State, _| s.lab.open = !s.lab.open);
    let reason = s.lab.hover.and_then(|i| s.doc.marks.get(i)).filter(|m| !m.reason.is_empty());
    let mut column = col().w_full().items_center().gap(8.0).pb(12.0);
    if s.lab.open {
        column = column.child(popover(s).transition(Transition::opacity()));
    }
    if let Some(bar) = crate::ui::review::bar(s) {
        column = column.child(bar);
    }
    if let Some(mark) = reason {
        column = column.child(text(mark.reason.clone()).font(theme::small()).color(THEME.accent.color()).key("lab-reason"));
    }
    Some(column.child(button).pin(Some(0.0), None, Some(0.0), Some(0.0)))
}
