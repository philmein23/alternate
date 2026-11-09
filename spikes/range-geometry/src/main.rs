//! Phase 1 spike. One `text_input` with keyed, invisible highlights, and a canvas above it drawing a
//! wavy line under each highlight's box. `cargo run -- --snap out.png --scene wrapped` renders it.

use std::cell::RefCell;
use std::ops::Range;

use rune::{AppSpec, Color, Cx, El, ScrollSpec, Shape, TextEdit, TextHighlight, TextStyle, canvas, col, oklch, text_input};

const BODY: &str = "The ferry left before the lantern was lit, and nobody on the harbor wall seemed to mind. \
A gull worked at something in the shallows while the tide came in over the slipway, slow and without any sign of hurry.\n\n\
By noon the fog had gone from the channel and the far shore showed its row of pale houses, each one a little crooked, \
each one with a boat pulled up beside it as if it had only stopped for a moment and meant to carry on.\n\n\
She wrote the first line four times and kept none of them, then wrote a fifth that said almost the same thing and left it alone. \
Some mornings the words arrive in the wrong order and the only cure is to go on putting them down until they settle.\n\n\
The kettle ticked on the stove. Outside, someone was dragging a chain along the quay, stopping, and dragging it again.";

pub struct Target {
    key: String,
    range: Range<usize>,
}

pub struct S {
    pub edit: TextEdit,
    pub targets: Vec<Target>,
    pub boxes: RefCell<Vec<Option<[f32; 4]>>>,
    pub window_boxes: RefCell<Vec<Option<[f32; 4]>>>,
    pub field: RefCell<Option<[f32; 4]>>,
    pub caret: RefCell<Option<[f32; 4]>>,
}

fn range_of(body: &str, needle: &str) -> Range<usize> {
    let start = body.find(needle).unwrap_or_else(|| panic!("{needle} is in the sample"));
    start..start + needle.len()
}

impl S {
    pub fn new(words: &[&str]) -> S {
        let targets = words.iter().enumerate().map(|(i, w)| Target { key: format!("target-{i}"), range: range_of(BODY, w) }).collect();
        S { edit: TextEdit::new(BODY), targets, boxes: RefCell::default(), window_boxes: RefCell::default(), field: RefCell::default(), caret: RefCell::default() }
    }

    /// Inserts `s` at byte `at` and shifts the ranges after it, which the real app does by diffing.
    pub fn insert_at(&mut self, at: usize, s: &str) {
        self.edit.set_selection(at, at);
        self.edit.insert(s);
        for t in &mut self.targets {
            if t.range.start >= at {
                t.range = t.range.start + s.len()..t.range.end + s.len();
            }
        }
    }
}

fn wave(x: f32, y: f32, w: f32) -> Vec<[f32; 2]> {
    let steps = (w / 3.0).ceil().max(2.0) as usize;
    (0..=steps)
        .map(|i| {
            let f = i as f32 / steps as f32;
            [x + f * w, y + (f * w * 0.35).sin() * 1.1]
        })
        .collect()
}

fn view(s: &S, cx: &mut Cx<S>) -> El {
    let style = TextStyle::new(16.0, 400.0).lh(26.0);
    let boxes: Vec<_> = s.targets.iter().map(|t| cx.layout(&t.key)).collect();
    *s.window_boxes.borrow_mut() = s.targets.iter().map(|t| cx.window_layout(&t.key)).collect();
    let field = cx.layout("doc");
    *s.field.borrow_mut() = field;
    *s.caret.borrow_mut() = cx.caret("doc");

    let mut shape = Shape::new();
    if let Some([fx, fy, ..]) = field {
        for [x, y, w, h] in boxes.iter().flatten().copied() {
            shape = shape.stroke(wave(x - fx, y - fy + h + 1.0, w), 1.2, oklch(0.75, 0.14, 55.0));
        }
    }
    *s.boxes.borrow_mut() = boxes;

    let highlights: Vec<_> = s.targets.iter().map(|t| TextHighlight::new(t.range.clone(), Color::TRANSPARENT).key(t.key.clone())).collect();
    let input = text_input(&s.edit, style).on_edit(|s: &mut S| &mut s.edit).key("doc").w_full().color(oklch(0.9, 0.03, 70.0)).text_highlights(highlights);
    let page = col().w_full().max_w(560.0).padding(40.0).child(col().w_full().child(input).child(canvas(shape).pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))));
    col().w_full().h_full().fill(oklch(0.16, 0.03, 270.0)).key("scroller").scroll(ScrollSpec { stick_bottom: false }).child(page)
}

fn init() -> S {
    S::new(&["lantern", "harbor", "row of pale houses, each one a little crooked, each one with a boat", "kettle"])
}

fn main() {
    let spec = AppSpec::<S> {
        scenes: &[("wrapped", |_| {})],
        ..AppSpec::new("Range geometry spike", (760.0, 640.0), init, view)
    };
    rune::App::new(spec).run();
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use rune::{Env, Runtime};

    use super::*;

    const WORDS: &[&str] = &["lantern", "harbor", "kettle"];

    fn runtime(words: &[&str]) -> Runtime<S> {
        let mut rt = Runtime::new(S::new(words), view, (760.0, 500.0), Env::new(true, 1.0));
        rt.frame(0.0);
        settle(&mut rt);
        rt
    }

    fn settle(rt: &mut Runtime<S>) {
        for _ in 0..600 {
            if !rt.frame(1.0 / 60.0) {
                return;
            }
        }
        panic!("never settled");
    }

    fn boxes(rt: &Runtime<S>) -> Vec<[f32; 4]> {
        rt.state.boxes.borrow().iter().map(|b| b.expect("every target has a box")).collect()
    }

    fn caret_at(rt: &mut Runtime<S>, at: usize) -> [f32; 4] {
        let (x, y) = rt.find("doc").expect("the field is on screen");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        settle(rt);
        rt.update(|s, _| s.edit.set_selection(at, at));
        settle(rt);
        rt.state.caret.borrow().expect("the focused field shows a caret")
    }

    #[test]
    fn keyed_highlights_report_boxes_on_a_text_input() {
        let rt = runtime(WORDS);
        let field = rt.state.field.borrow().expect("the field has a rect");
        for b in boxes(&rt) {
            assert!(b[2] > 10.0 && b[3] > 10.0, "a real size: {b:?}");
            assert!(b[0] >= field[0] && b[0] + b[2] <= field[0] + field[2] + 1.0, "inside the field: {b:?} in {field:?}");
            assert!(b[1] >= field[1] && b[1] + b[3] <= field[1] + field[3] + 1.0, "inside the field: {b:?} in {field:?}");
        }
    }

    #[test]
    fn a_box_starts_where_the_caret_at_that_byte_sits() {
        let mut rt = runtime(WORDS);
        let targets: Vec<_> = rt.state.targets.iter().map(|t| t.range.start).collect();
        let seen = boxes(&rt);
        for (at, b) in targets.into_iter().zip(seen) {
            let caret = caret_at(&mut rt, at);
            assert!((caret[0] - b[0]).abs() < 3.0, "x matches the caret: box {b:?}, caret {caret:?}");
            assert!((caret[1] - b[1]).abs() < 6.0, "y matches the caret: box {b:?}, caret {caret:?}");
        }
    }

    #[test]
    fn boxes_follow_edits_before_them() {
        let mut rt = runtime(WORDS);
        let before = boxes(&rt);
        rt.update(|s, _| s.insert_at(0, "Long before any of this began and for no good reason at all, "));
        settle(&mut rt);
        let after = boxes(&rt);
        assert_ne!(before[0], after[0], "the first target moved");
        let at = rt.state.targets[0].range.start;
        let caret = caret_at(&mut rt, at);
        let now = boxes(&rt)[0];
        assert!((caret[0] - now[0]).abs() < 3.0 && (caret[1] - now[1]).abs() < 6.0, "box {now:?}, caret {caret:?}");
    }

    #[test]
    fn boxes_follow_a_resize_that_rewraps_the_text() {
        let mut rt = runtime(WORDS);
        let before = boxes(&rt);
        rt.set_viewport((300.0, 500.0), 1.0);
        settle(&mut rt);
        let after = boxes(&rt);
        assert_ne!(before[2], after[2], "the last target rewrapped");
        let at = rt.state.targets[2].range.start;
        let caret = caret_at(&mut rt, at);
        assert!((caret[0] - after[2][0]).abs() < 3.0 && (caret[1] - after[2][1]).abs() < 6.0, "box {:?}, caret {caret:?}", after[2]);
    }

    #[test]
    fn layout_ignores_scrolling_and_window_layout_follows_it() {
        let mut rt = runtime(WORDS);
        let page = boxes(&rt);
        let window = rt.state.window_boxes.borrow().clone();
        rt.wheel(200.0, 200.0, -200.0);
        settle(&mut rt);
        assert_eq!(page, boxes(&rt), "document coordinates hold still");
        assert_ne!(window, *rt.state.window_boxes.borrow(), "window coordinates moved");
    }

    #[test]
    fn a_wrapped_target_reports_one_union_box() {
        let rt = runtime(&["row of pale houses, each one a little crooked, each one with a boat"]);
        let [_, _, w, h] = boxes(&rt)[0];
        println!("wrapped target box: {w} x {h}");
        assert!(h > 40.0, "spans more than one line: {h}");
    }

    #[test]
    fn typing_cost_with_many_keys() {
        let all: Vec<String> = BODY.split_whitespace().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string()).filter(|w| w.len() > 3).collect();
        for n in [0, 1, 10, 40, 100] {
            let mut unique: Vec<&str> = Vec::new();
            for w in &all {
                if !unique.contains(&w.as_str()) && unique.len() < n {
                    unique.push(w);
                }
            }
            let mut rt = runtime(&unique);
            let start = Instant::now();
            for _ in 0..30 {
                rt.update(|s, _| s.insert_at(0, "x"));
                rt.frame(1.0 / 60.0);
            }
            println!("{} keys: {:?} per edit", unique.len(), start.elapsed() / 30);
        }
    }
}
