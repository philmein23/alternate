//! The pen marks over the page, drawn from each target's line boxes.

use rune::{Color, Cx, El, Shape, canvas};

use crate::State;
use crate::editor::Mode;
use crate::model::{Level, TargetId};
use crate::ui::theme::THEME;

/// How far left of the text the margin marks reach.
pub const MARGIN: f32 = 28.0;
const STROKE: f32 = 1.2;
const DOT: f32 = 1.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dot {
    pub center: [f32; 2],
    pub lit: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MarkShape {
    pub strokes: Vec<Vec<[f32; 2]>>,
    pub dots: Vec<Dot>,
    pub margin: bool,
}

fn seeded(id: TargetId, salt: u64) -> f32 {
    let mut v = id.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ salt.wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    v ^= v >> 29;
    v = v.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    (v >> 40) as f32 / (1u64 << 24) as f32
}

/// A slightly uneven pen line along `len` points. The same id always draws the same line.
fn wave(id: TargetId, salt: u64, len: f32) -> Vec<(f32, f32)> {
    let phase = seeded(id, salt) * std::f32::consts::TAU;
    let amp = 0.9 + seeded(id, salt + 1) * 0.5;
    let freq = 0.32 + seeded(id, salt + 2) * 0.1;
    let steps = (len / 3.0).ceil().max(2.0) as usize;
    (0..=steps)
        .map(|i| {
            let d = i as f32 / steps as f32 * len;
            (d, (d * freq + phase).sin() * amp + (d * freq * 0.37 + phase * 2.0).sin() * amp * 0.4)
        })
        .collect()
}

fn three_dots(from: [f32; 2], step: [f32; 2], changed: bool) -> Vec<Dot> {
    (0..3).map(|i| Dot { center: [from[0] + step[0] * i as f32, from[1] + step[1] * i as f32], lit: changed && i == 1 }).collect()
}

/// The mark for a target whose line boxes are `boxes`, in the canvas's own points.
pub fn mark_shape(level: Level, id: TargetId, boxes: &[[f32; 4]], changed: bool) -> Option<MarkShape> {
    let (first, last) = (boxes.first()?, boxes.last()?);
    if level == Level::Paragraph {
        let x = boxes.iter().map(|b| b[0]).fold(f32::MAX, f32::min) - 16.0;
        let (top, bottom) = (first[1] + 4.0, last[1] + last[3] - 4.0);
        let line = wave(id, 0, bottom - top).into_iter().map(|(d, o)| [x + o, top + d]).collect();
        let mid = (top + bottom) / 2.0;
        let dots = three_dots([x - 9.0, mid - 6.0], [0.0, 6.0], changed);
        return Some(MarkShape { strokes: vec![line], dots, margin: true });
    }
    let strokes = boxes
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let y = b[1] + b[3] - 1.0;
            wave(id, i as u64 + 1, b[2]).into_iter().map(|(d, o)| [b[0] + d, y + o * 0.7]).collect()
        })
        .collect();
    let dots = three_dots([last[0] + last[2] + 5.0, last[1] + last[3] - 2.0], [5.0, 0.0], changed);
    Some(MarkShape { strokes, dots, margin: false })
}

fn dot_points(c: [f32; 2]) -> Vec<[f32; 2]> {
    (0..8)
        .map(|i| {
            let a = i as f32 / 8.0 * std::f32::consts::TAU;
            [c[0] + a.cos() * DOT, c[1] + a.sin() * DOT]
        })
        .collect()
}

fn paint(mut shape: Shape, mark: MarkShape, line: Color) -> Shape {
    for points in mark.strokes {
        shape = shape.stroke(points, STROKE, line);
    }
    for dot in mark.dots {
        shape = shape.fill(dot_points(dot.center), if dot.lit { THEME.marker.color() } else { THEME.faint.color() });
    }
    shape
}

/// The canvas of marks for the page, or nothing while they are hidden.
pub fn overlay(s: &State, cx: &mut Cx<State>) -> Option<El> {
    if s.mode != Mode::Active || s.marks_hidden || s.doc.targets.is_empty() {
        return None;
    }
    let ranges: Vec<_> = s.doc.targets.iter().map(|t| t.range.clone()).collect();
    let boxes = cx.text_rects("doc", &ranges);
    let [fx, fy, ..] = cx.layout("doc")?;
    let mut shape = Shape::new();
    for (t, lines) in s.doc.targets.iter().zip(boxes) {
        let local: Vec<_> = lines.iter().map(|b| [b[0] - fx + MARGIN, b[1] - fy, b[2], b[3]]).collect();
        if let Some(mark) = mark_shape(t.level, t.id, &local, !t.is_original(&s.doc.text)) {
            let line = if mark.margin { THEME.marker.color() } else { THEME.accent.alpha(0.75) };
            shape = paint(shape, mark, line);
        }
    }
    Some(canvas(shape).pin(Some(-MARGIN), Some(0.0), Some(0.0), Some(0.0)).key("marks"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOX: [f32; 4] = [10.0, 20.0, 90.0, 28.0];

    #[test]
    fn the_same_target_draws_the_same_line_every_time() {
        assert_eq!(mark_shape(Level::Word, 7, &[BOX], false), mark_shape(Level::Word, 7, &[BOX], false));
    }

    #[test]
    fn different_targets_draw_different_lines() {
        assert_ne!(mark_shape(Level::Word, 7, &[BOX], false), mark_shape(Level::Word, 8, &[BOX], false));
    }

    #[test]
    fn a_wrapped_target_gets_a_line_per_line_and_dots_at_its_end() {
        let second = [10.0, 48.0, 50.0, 28.0];
        let m = mark_shape(Level::Sentence, 1, &[BOX, second], false).unwrap();
        assert_eq!(m.strokes.len(), 2);
        assert!(m.dots.iter().all(|d| d.center[0] > 60.0 && d.center[1] > 48.0));
    }

    #[test]
    fn a_paragraph_mark_runs_down_the_margin() {
        let second = [10.0, 48.0, 50.0, 28.0];
        let m = mark_shape(Level::Paragraph, 1, &[BOX, second], false).unwrap();
        assert!(m.margin);
        assert_eq!(m.strokes.len(), 1);
        assert!(m.strokes[0].iter().all(|p| p[0] < 10.0));
    }

    #[test]
    fn the_middle_dot_lights_when_the_page_has_left_the_original() {
        let lit = |changed| mark_shape(Level::Word, 1, &[BOX], changed).unwrap().dots.iter().map(|d| d.lit).collect::<Vec<_>>();
        assert_eq!(lit(false), [false, false, false]);
        assert_eq!(lit(true), [false, true, false]);
    }

    #[test]
    fn a_target_with_no_boxes_draws_nothing() {
        assert_eq!(mark_shape(Level::Word, 1, &[], false), None);
    }
}
